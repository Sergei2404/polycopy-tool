use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BotAction {
    /// Fetch new trades for all watched wallets since `last_synced_ms`.
    Scan {
        last_synced_ms: u64,
        watch_wallets: Vec<String>,
        /// Polymarket API key (POLY_API_KEY). Optional for public endpoints.
        #[serde(default)]
        api_key: Option<String>,
        #[serde(default)]
        clob_base: Option<String>,
    },
    /// Add `wallet` to `watch_wallets` and return the updated list.
    AddWallet {
        watch_wallets: Vec<String>,
        wallet: String,
    },
    /// Remove `wallet` from `watch_wallets` and return the updated list.
    RemoveWallet {
        watch_wallets: Vec<String>,
        wallet: String,
    },
    /// Return the tool version string.
    Version,
}

/// A single trade record returned by `GET /trades`.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Trade {
    pub id: String,
    pub asset_id: String,
    /// Decimal string, e.g. "0.65".
    pub price: String,
    /// "BUY" or "SELL" from the watched wallet's perspective.
    pub side: String,
}
