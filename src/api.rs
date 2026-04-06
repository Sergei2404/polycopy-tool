use crate::near::agent::host;
use crate::types::Trade;

pub const DEFAULT_CLOB_BASE: &str = "https://clob.polymarket.com";

/// Fetch trades made by `wallet` (as maker) since `after_ms`.
///
/// Uses the public `GET /trades?maker_address={addr}&after={unix_seconds}` endpoint.
pub fn fetch_trades(wallet: &str, after_ms: u64, clob_base: &str) -> Result<Vec<Trade>, String> {
    let after_s = after_ms / 1000;
    let path = format!("/trades?maker_address={wallet}&after={after_s}");
    let url = format!("{clob_base}{path}");

    host::log(host::LogLevel::Debug, &format!("GET {url}"));

    let resp = host::http_request("GET", &url, "{}", None, Some(15_000))
        .map_err(|e| format!("GET {path}: {e}"))?;

    if resp.status < 200 || resp.status >= 300 {
        return Err(format!(
            "GET {path} returned HTTP {}: {}",
            resp.status,
            String::from_utf8_lossy(&resp.body)
        ));
    }

    let parsed: serde_json::Value =
        serde_json::from_slice(&resp.body).map_err(|e| format!("parse /trades: {e}"))?;

    // API may return `{ "data": [...] }` or a bare array.
    let arr = if parsed.is_array() {
        parsed
    } else {
        parsed
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]))
    };

    let trades: Vec<Trade> =
        serde_json::from_value(arr).map_err(|e| format!("deserialize trades: {e}"))?;

    host::log(
        host::LogLevel::Info,
        &format!("wallet {wallet}: {} trade(s) since ts={after_s}", trades.len()),
    );

    Ok(trades)
}
