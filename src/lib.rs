//! Polymarket copy-trade WASM tool for the IronClaw Claude Agent SDK.
//!
//! # How it works
//!
//! The agent calls this tool on a schedule (e.g. every 30 s).
//! `params`   — JSON matching the `BotAction` schema (required).
//! `context`  — JSON state from the previous call's response (null on first call).
//!
//! Every response contains `{ "result": {...}, "state": {...} }`.
//! The agent must store `state` and pass it back as `context` next time.
//!
//! On the very first call (no context), the tool reads `polycopy-state.json`
//! from the workspace root as a bootstrap — fill in credentials there once.
//!
//! # Supported actions
//!
//! - `sync`                         — scan + copy new trades
//! - `check`                        — scan for new trades, return them, do NOT place orders
//! - `add_wallet    { wallet }`     — add address to watch list
//! - `remove_wallet { wallet }`     — remove address from watch list
//! - `set_max_bid   { usdc }`       — update per-trade USDC budget
//! - `get_status`                   — return state read-only

mod api;
mod auth;
mod state;
mod types;

use types::{BotAction, CheckResult, SyncResult, ToolOutput};

wit_bindgen::generate!({
    world: "sandboxed-tool",
    path: "wit/tool.wit",
});

struct PolycopyTool;

impl exports::near::agent::tool::Guest for PolycopyTool {
    fn execute(req: exports::near::agent::tool::Request) -> exports::near::agent::tool::Response {
        match execute_inner(&req.params, req.context.as_deref()) {
            Ok(output) => exports::near::agent::tool::Response {
                output: Some(output),
                error: None,
            },
            Err(e) => exports::near::agent::tool::Response {
                output: None,
                error: Some(e),
            },
        }
    }

    fn schema() -> String {
        r#"{
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["sync", "check", "add_wallet", "remove_wallet", "set_max_bid", "get_status"],
                    "description": "Operation to perform"
                },
                "wallet": {
                    "type": "string",
                    "description": "Ethereum address (0x-prefixed). Required for: add_wallet, remove_wallet"
                },
                "usdc": {
                    "type": "number",
                    "description": "Maximum USDC to spend per copied trade. Required for: set_max_bid"
                }
            }
        }"#
        .to_string()
    }

    fn description() -> String {
        "Polymarket copy-trade tool. Scans watched wallets for new trades and copies them \
         as FOK orders. Call `sync` on a schedule (e.g. every 30 s). \
         State is returned with every response and must be passed back as `context`. \
         First-run bootstrap: place filled `polycopy-state.json` in the workspace root."
            .to_string()
    }
}

fn execute_inner(params: &str, context: Option<&str>) -> Result<String, String> {
    let action: BotAction =
        serde_json::from_str(params).map_err(|e| format!("invalid params: {e}"))?;

    let mut s = state::load(context)?;

    let result_json = match action {
        BotAction::Sync => {
            let result = do_sync(&mut s)?;
            serde_json::to_string(&ToolOutput { result, state: s })
                .map_err(|e| e.to_string())?
        }

        BotAction::Check => {
            let result = do_check(&mut s)?;
            serde_json::to_string(&ToolOutput { result, state: s })
                .map_err(|e| e.to_string())?
        }

        BotAction::AddWallet { wallet } => {
            let wallet = wallet.to_lowercase();
            if !s.config.watch_wallets.contains(&wallet) {
                s.config.watch_wallets.push(wallet.clone());
                near::agent::host::log(
                    near::agent::host::LogLevel::Info,
                    &format!("added wallet {wallet}"),
                );
            }
            serde_json::to_string(&ToolOutput {
                result: serde_json::json!({ "added": wallet }),
                state: s,
            })
            .map_err(|e| e.to_string())?
        }

        BotAction::RemoveWallet { wallet } => {
            let wallet = wallet.to_lowercase();
            let before = s.config.watch_wallets.len();
            s.config.watch_wallets.retain(|w| w != &wallet);
            let removed = before - s.config.watch_wallets.len();
            near::agent::host::log(
                near::agent::host::LogLevel::Info,
                &format!("removed {removed} entry for {wallet}"),
            );
            serde_json::to_string(&ToolOutput {
                result: serde_json::json!({ "removed": removed }),
                state: s,
            })
            .map_err(|e| e.to_string())?
        }

        BotAction::SetMaxBid { usdc } => {
            if usdc <= 0.0 {
                return Err(format!("usdc must be positive, got {usdc}"));
            }
            s.config.max_bid_usdc = usdc;
            near::agent::host::log(
                near::agent::host::LogLevel::Info,
                &format!("max_bid_usdc set to {usdc}"),
            );
            serde_json::to_string(&ToolOutput {
                result: serde_json::json!({ "max_bid_usdc": usdc }),
                state: s,
            })
            .map_err(|e| e.to_string())?
        }

        BotAction::GetStatus => serde_json::to_string(&ToolOutput {
            result: serde_json::json!({
                "watch_wallets":   s.config.watch_wallets,
                "max_bid_usdc":    s.config.max_bid_usdc,
                "last_synced_ms":  s.last_synced_ms,
                "copied_count":    s.copied_orders.len(),
            }),
            state: s,
        })
        .map_err(|e| e.to_string())?,
    };

    Ok(result_json)
}

fn do_sync(s: &mut types::State) -> Result<SyncResult, String> {
    if s.config.watch_wallets.is_empty() {
        return Ok(SyncResult {
            new_trades_found: 0,
            orders_placed: 0,
            failed: 0,
            skipped_dedup: 0,
        });
    }

    let after_ms = s.last_synced_ms;
    let clob_base = s.config.clob_base.clone();
    let creds = s.credentials.clone();

    let mut new_trades_found = 0usize;
    let mut orders_placed = 0usize;
    let mut failed = 0usize;
    let mut skipped_dedup = 0usize;

    // Collect wallets to avoid borrow issues.
    let wallets: Vec<String> = s.config.watch_wallets.clone();

    for wallet in &wallets {
        let trades = match api::fetch_trades(wallet, after_ms, &creds, &clob_base) {
            Ok(t) => t,
            Err(e) => {
                near::agent::host::log(
                    near::agent::host::LogLevel::Error,
                    &format!("fetch_trades {wallet}: {e}"),
                );
                continue;
            }
        };

        new_trades_found += trades.len();

        for trade in &trades {
            if s.copied_orders.contains(&trade.id) {
                skipped_dedup += 1;
                continue;
            }

            match api::place_order(trade, s) {
                Ok(_order_id) => {
                    s.copied_orders.push(trade.id.clone());
                    orders_placed += 1;
                }
                Err(e) => {
                    near::agent::host::log(
                        near::agent::host::LogLevel::Error,
                        &format!("place_order trade={}: {e}", trade.id),
                    );
                    failed += 1;
                }
            }
        }
    }

    s.last_synced_ms = near::agent::host::now_millis();

    Ok(SyncResult {
        new_trades_found,
        orders_placed,
        failed,
        skipped_dedup,
    })
}

fn do_check(s: &mut types::State) -> Result<CheckResult, String> {
    if s.config.watch_wallets.is_empty() {
        return Ok(CheckResult {
            new_trades: vec![],
            already_copied: 0,
        });
    }

    let after_ms = s.last_synced_ms;
    let clob_base = s.config.clob_base.clone();
    let creds = s.credentials.clone();
    let wallets: Vec<String> = s.config.watch_wallets.clone();

    let mut new_trades = vec![];
    let mut already_copied = 0usize;

    for wallet in &wallets {
        let trades = match api::fetch_trades(wallet, after_ms, &creds, &clob_base) {
            Ok(t) => t,
            Err(e) => {
                near::agent::host::log(
                    near::agent::host::LogLevel::Error,
                    &format!("fetch_trades {wallet}: {e}"),
                );
                continue;
            }
        };

        for trade in trades {
            if s.copied_orders.contains(&trade.id) {
                already_copied += 1;
            } else {
                new_trades.push(trade);
            }
        }
    }

    // Advance the timestamp so the next check/sync doesn't re-fetch the same window.
    s.last_synced_ms = near::agent::host::now_millis();

    Ok(CheckResult { new_trades, already_copied })
}

// Required by wit-bindgen to register the implementation.
export!(PolycopyTool);
