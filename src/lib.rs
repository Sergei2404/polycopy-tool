mod api;
mod state;
mod types;

use types::{BotAction, ToolOutput};

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
                    "enum": ["scan", "add_wallet", "remove_wallet"],
                    "description": "Operation to perform"
                },
                "wallet": {
                    "type": "string",
                    "description": "Ethereum address (0x-prefixed). Required for: add_wallet, remove_wallet"
                },
                "clob_base": {
                    "type": "string",
                    "description": "Override CLOB base URL (optional)"
                }
            }
        }"#
        .to_string()
    }

    fn description() -> String {
        "Polymarket trade scanner. Tracks wallets and returns new trades since last scan. \
         Watched wallets and timestamp are returned in `context` and must be passed back on \
         every subsequent call. First scan only records the timestamp; picks appear from the \
         second call onward."
            .to_string()
    }
}

fn execute_inner(params: &str, context: Option<&str>) -> Result<String, String> {
    let action: BotAction =
        serde_json::from_str(params).map_err(|e| format!("invalid params: {e}"))?;

    let mut ctx = state::load(context)?;

    let result_json = match action {
        BotAction::Scan { clob_base } => {
            let base = clob_base.as_deref().unwrap_or(api::DEFAULT_CLOB_BASE);

            // First run: no prior timestamp — record now and return no picks.
            if ctx.last_synced_ms == 0 {
                ctx.last_synced_ms = near::agent::host::now_millis();
                serde_json::to_string(&ToolOutput {
                    result: serde_json::json!({ "new_picks": [], "first_run": true }),
                    context: ctx,
                })
                .map_err(|e| e.to_string())?
            } else {
                let after_ms = ctx.last_synced_ms;
                let wallets: Vec<String> = ctx.watch_wallets.clone();
                let mut new_picks = vec![];

                for wallet in &wallets {
                    match api::fetch_trades(wallet, after_ms, base) {
                        Ok(trades) => new_picks.extend(trades),
                        Err(e) => near::agent::host::log(
                            near::agent::host::LogLevel::Error,
                            &format!("fetch_trades {wallet}: {e}"),
                        ),
                    }
                }

                ctx.last_synced_ms = near::agent::host::now_millis();

                serde_json::to_string(&ToolOutput {
                    result: serde_json::json!({ "new_picks": new_picks }),
                    context: ctx,
                })
                .map_err(|e| e.to_string())?
            }
        }

        BotAction::AddWallet { wallet } => {
            let wallet = wallet.to_lowercase();
            if !ctx.watch_wallets.contains(&wallet) {
                ctx.watch_wallets.push(wallet.clone());
                near::agent::host::log(
                    near::agent::host::LogLevel::Info,
                    &format!("added wallet {wallet}"),
                );
            }
            serde_json::to_string(&ToolOutput {
                result: serde_json::json!({ "watch_wallets": ctx.watch_wallets }),
                context: ctx,
            })
            .map_err(|e| e.to_string())?
        }

        BotAction::RemoveWallet { wallet } => {
            let wallet = wallet.to_lowercase();
            ctx.watch_wallets.retain(|w| w != &wallet);
            near::agent::host::log(
                near::agent::host::LogLevel::Info,
                &format!("removed wallet {wallet}"),
            );
            serde_json::to_string(&ToolOutput {
                result: serde_json::json!({ "watch_wallets": ctx.watch_wallets }),
                context: ctx,
            })
            .map_err(|e| e.to_string())?
        }
    };

    Ok(result_json)
}

export!(PolycopyTool);
