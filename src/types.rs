use serde::{Deserialize, Serialize};

// ─── Actions ──────────────────────────────────────────────────────────────────

/// Every call to the tool carries one of these actions in `params`.
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BotAction {
    /// Scan watched wallets for new trades and copy them.
    Sync,
    /// Scan for new trades but do NOT place orders. Advances last_synced_ms.
    Check,
    /// Add a wallet to the watch list.
    AddWallet { wallet: String },
    /// Remove a wallet from the watch list.
    RemoveWallet { wallet: String },
    /// Update the maximum USDC to spend per copied trade.
    SetMaxBid { usdc: f64 },
    /// Return current state without side effects.
    GetStatus,
}

// ─── Persistent state ─────────────────────────────────────────────────────────

/// Full tool state — serialized as the `state` field in every response,
/// and passed back by the agent as `context` on the next call.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct State {
    pub config: Config,
    pub credentials: Credentials,
    /// Unix timestamp (ms) of the last successful sync.
    /// Trades with timestamp > last_synced_ms / 1000 are considered new.
    #[serde(default)]
    pub last_synced_ms: u64,
    /// Trade IDs that have already been copied — the dedup set.
    #[serde(default)]
    pub copied_orders: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub watch_wallets: Vec<String>,
    /// USDC to spend per copied order.  size = ceil(max_bid_usdc / price).
    #[serde(default = "default_max_bid")]
    pub max_bid_usdc: f64,
    /// Token IDs that belong to the NegRisk CTF Exchange.
    /// Populate manually; auto-detection is a v2 feature.
    #[serde(default)]
    pub neg_risk_token_ids: Vec<String>,
    #[serde(default = "default_clob_base")]
    pub clob_base: String,
}

fn default_max_bid() -> f64 {
    1.0
}
fn default_clob_base() -> String {
    "https://clob.polymarket.com".into()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            watch_wallets: vec![],
            max_bid_usdc: default_max_bid(),
            neg_risk_token_ids: vec![],
            clob_base: default_clob_base(),
        }
    }
}

/// Pre-derived API credentials.  Run the native bot once to obtain these
/// via L1 EIP-712 key derivation, then paste into `polycopy-state.json`.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Credentials {
    pub api_key: String,
    /// Base64url-encoded HMAC secret (same encoding as Polymarket returns it).
    pub secret: String,
    pub passphrase: String,
    /// Hex-encoded 32-byte private key (0x-prefixed).  Used to EIP-712-sign orders.
    pub private_key_hex: String,
    /// Ethereum address corresponding to `private_key_hex` (0x-prefixed).
    /// Must match the Polymarket account holding funds.
    pub maker_address: String,
}

// ─── CLOB API types ───────────────────────────────────────────────────────────

/// A single trade record returned by `GET /trades`.
/// Fields are parsed permissively; unknown fields are ignored.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Trade {
    /// Unique trade identifier — used as the dedup key in `copied_orders`.
    pub id: String,
    /// Outcome token ID (decimal string, may be 256-bit large).
    pub asset_id: String,
    /// Price as a decimal string, e.g. "0.65".
    pub price: String,
    /// "BUY" or "SELL" from the perspective of the watched wallet (the maker).
    pub side: String,
}

// ─── Response types ───────────────────────────────────────────────────────────

/// Result payload for the `sync` action.
#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub new_trades_found: usize,
    pub orders_placed: usize,
    pub failed: usize,
    pub skipped_dedup: usize,
}

/// Result payload for the `check` action — no orders placed.
#[derive(Debug, Serialize)]
pub struct CheckResult {
    /// Trades found since last sync that are not yet in copied_orders.
    pub new_trades: Vec<Trade>,
    /// Trades already in copied_orders (would be skipped by sync).
    pub already_copied: usize,
}

/// Wrapper returned by every action: result + full updated state.
/// The agent stores `state` and passes it as `context` next call.
#[derive(Serialize)]
pub struct ToolOutput<T: Serialize> {
    pub result: T,
    pub state: State,
}
