use serde::{Deserialize, Serialize};

// ─── Actions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BotAction {
    AddWallet { wallet: String },
    RemoveWallet { wallet: String },
    Scan {
        /// Override CLOB base URL; defaults to https://clob.polymarket.com
        #[serde(default)]
        clob_base: Option<String>,
    },
}

// ─── Context ──────────────────────────────────────────────────────────────────

/// Passed in by the agent as `context` and returned as `context` in every response.
/// The agent must store it and pass it back on every subsequent call.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Context {
    /// Unix timestamp (ms) of the last scan. Zero means "never scanned".
    #[serde(default)]
    pub last_synced_ms: u64,
    /// Ethereum addresses (lowercase) being watched.
    #[serde(default)]
    pub watch_wallets: Vec<String>,
}

// ─── CLOB trade record ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Trade {
    /// Unique trade ID.
    pub id: String,
    /// Outcome token ID (decimal string).
    pub asset_id: String,
    /// Price as a decimal string, e.g. "0.65".
    pub price: String,
    /// "BUY" or "SELL" from the watched wallet's perspective.
    pub side: String,
}

// ─── Response wrapper ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ToolOutput<T: Serialize> {
    pub result: T,
    /// Updated context — agent must pass this back on the next call.
    pub context: Context,
}
