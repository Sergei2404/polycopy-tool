mod api;
mod types;

use types::BotAction;

wit_bindgen::generate!({
    world: "sandboxed-tool",
    path: "wit/tool.wit",
});

struct PolycopyTool;

impl exports::near::agent::tool::Guest for PolycopyTool {
    fn execute(req: exports::near::agent::tool::Request) -> exports::near::agent::tool::Response {
        match execute_inner(&req.params) {
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
                    "enum": ["scan", "add_wallet", "remove_wallet", "version"]
                },
                "last_synced_ms": {
                    "type": "integer",
                    "description": "Timestamp (ms) of the previous scan. Required for: scan"
                },
                "watch_wallets": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Current wallet list. Required for all actions"
                },
                "wallet": {
                    "type": "string",
                    "description": "Ethereum address (0x-prefixed). Required for: add_wallet, remove_wallet"
                },
                "api_key": {
                    "type": "string",
                    "description": "Polymarket API key (POLY_API_KEY). Optional for public endpoints, required for private ones"
                },
                "clob_base": {
                    "type": "string",
                    "description": "Override CLOB base URL (optional, scan only)"
                }
            }
        }"#
        .to_string()
    }

    fn description() -> String {
        "Stateless Polymarket trade scanner. \
         Pass last_synced_ms and watch_wallets on every call — the tool has no storage. \
         scan returns new trades as JSONL plus an updated last_synced_ms (unchanged on error). \
         add_wallet / remove_wallet return the updated watch_wallets list."
            .to_string()
    }
}

fn execute_inner(params: &str) -> Result<String, String> {
    let action: BotAction =
        serde_json::from_str(params).map_err(|e| format!("invalid params: {e}"))?;

    match action {
        BotAction::Scan { last_synced_ms, watch_wallets, api_key, clob_base } => {
            let base = clob_base.as_deref().unwrap_or(api::DEFAULT_CLOB_BASE);
            let key = api_key.as_deref();
            let mut picks = vec![];

            for wallet in &watch_wallets {
                let trades = api::fetch_trades(wallet, last_synced_ms, base, key)
                    .map_err(|e| format!("fetch_trades {wallet}: {e}"))?;
                picks.extend(trades);
            }

            let new_ts = near::agent::host::now_millis();

            let picks_jsonl = picks
                .iter()
                .filter_map(|t| serde_json::to_string(t).ok())
                .collect::<Vec<_>>()
                .join("\n");

            serde_json::to_string(&serde_json::json!({
                "last_synced_ms": new_ts,
                "picks": picks_jsonl,
            }))
            .map_err(|e| e.to_string())
        }

        BotAction::AddWallet { mut watch_wallets, wallet } => {
            let wallet = wallet.to_lowercase();
            if !watch_wallets.contains(&wallet) {
                watch_wallets.push(wallet);
            }
            serde_json::to_string(&serde_json::json!({ "watch_wallets": watch_wallets }))
                .map_err(|e| e.to_string())
        }

        BotAction::RemoveWallet { mut watch_wallets, wallet } => {
            let wallet = wallet.to_lowercase();
            watch_wallets.retain(|w| w != &wallet);
            serde_json::to_string(&serde_json::json!({ "watch_wallets": watch_wallets }))
                .map_err(|e| e.to_string())
        }

        BotAction::Version => Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
        })
        .to_string()),
    }
}

export!(PolycopyTool);
