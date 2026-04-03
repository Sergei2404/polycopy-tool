//! Polymarket CLOB REST API calls.
//!
//! All HTTP goes through the host capability (`host::http_request`).
//! Authentication uses L2 HMAC-SHA256 headers on every request.

use crate::auth::{self, OrderData};
use crate::near::agent::host;
use crate::types::{Credentials, State, Trade};

// ─── HTTP helper ──────────────────────────────────────────────────────────────

/// Make an authenticated CLOB request.
///
/// `path` must include the leading slash and any query string,
/// e.g. `/trades?maker_address=0x...&after=1700000000`.
/// The same `path` (without base URL) is used in the L2 HMAC message.
fn clob_request(
    method: &str,
    path: &str,
    body: Option<&str>,
    creds: &Credentials,
    clob_base: &str,
) -> Result<Vec<u8>, String> {
    let timestamp = host::now_millis() / 1000;
    let body_str = body.unwrap_or("");

    let signature = auth::sign_l2(&creds.secret, method, path, body_str, timestamp)?;

    // Build JSON header object.
    let headers = if body.is_some() {
        format!(
            r#"{{"POLY_API_KEY":"{key}","POLY_SIGNATURE":"{sig}","POLY_TIMESTAMP":"{ts}","POLY_NONCE":"0","POLY_PASSPHRASE":"{pass}","Content-Type":"application/json"}}"#,
            key  = creds.api_key,
            sig  = signature,
            ts   = timestamp,
            pass = creds.passphrase,
        )
    } else {
        format!(
            r#"{{"POLY_API_KEY":"{key}","POLY_SIGNATURE":"{sig}","POLY_TIMESTAMP":"{ts}","POLY_NONCE":"0","POLY_PASSPHRASE":"{pass}"}}"#,
            key  = creds.api_key,
            sig  = signature,
            ts   = timestamp,
            pass = creds.passphrase,
        )
    };

    let url = format!("{}{}", clob_base, path);
    let body_bytes = body.map(|b| b.as_bytes().to_vec());

    host::log(
        host::LogLevel::Debug,
        &format!("CLOB {} {}", method, path),
    );

    let resp = host::http_request(method, &url, &headers, body_bytes.as_deref(), Some(15_000))
        .map_err(|e| format!("{method} {path}: {e}"))?;

    if resp.status < 200 || resp.status >= 300 {
        return Err(format!(
            "{method} {path} returned HTTP {}: {}",
            resp.status,
            String::from_utf8_lossy(&resp.body)
        ));
    }

    Ok(resp.body)
}

// ─── Trade fetching ───────────────────────────────────────────────────────────

/// Fetch trades made by `wallet` (as maker) since `after_ms`.
///
/// Uses `GET /trades?maker_address={addr}&after={unix_seconds}`.
/// Returns an empty vec if the response contains no trades.
pub fn fetch_trades(
    wallet: &str,
    after_ms: u64,
    creds: &Credentials,
    clob_base: &str,
) -> Result<Vec<Trade>, String> {
    let after_s = after_ms / 1000;
    let path = format!("/trades?maker_address={wallet}&after={after_s}");

    let body = clob_request("GET", &path, None, creds, clob_base)?;

    let parsed: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("parse /trades response: {e}"))?;

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

// ─── Order placement ──────────────────────────────────────────────────────────

/// Build, sign, and submit a FOK copy-order for the given trade.
///
/// Returns the order ID string on success.
pub fn place_order(trade: &Trade, state: &State) -> Result<String, String> {
    let creds = &state.credentials;
    let max_bid = state.config.max_bid_usdc;

    let price: f64 = trade
        .price
        .parse()
        .map_err(|_| format!("invalid price '{}'", trade.price))?;

    if !(0.001..=0.999).contains(&price) {
        return Err(format!("price {price} out of valid range [0.001, 0.999]"));
    }

    // Number of outcome tokens: at least ceil(max_bid / price).
    let size = (max_bid / price).ceil() as u64;

    let side_num: u8;
    let maker_amount: u64;
    let taker_amount: u64;

    match trade.side.to_uppercase().as_str() {
        "BUY" => {
            // Maker pays USDC (makerAmount), receives tokens (takerAmount).
            side_num = 0;
            taker_amount = size * 1_000_000;
            maker_amount = ((size as f64 * price) * 1_000_000.0).ceil() as u64;
        }
        "SELL" => {
            // Maker provides tokens (makerAmount), receives USDC (takerAmount).
            side_num = 1;
            maker_amount = size * 1_000_000;
            taker_amount = ((size as f64 * price) * 1_000_000.0).ceil() as u64;
        }
        other => return Err(format!("unknown trade side '{other}'")),
    }

    let neg_risk = state
        .config
        .neg_risk_token_ids
        .contains(&trade.asset_id);

    // Salt: timestamp XOR-mixed to reduce collision probability.
    let salt = host::now_millis() ^ 0xdead_beef_cafe_babe;

    let order = OrderData {
        salt,
        maker: &creds.maker_address,
        token_id: &trade.asset_id,
        maker_amount,
        taker_amount,
        side: side_num,
        neg_risk,
    };

    let signature = auth::sign_order(&creds.private_key_hex, &order)?;

    let body_value = serde_json::json!({
        "order": {
            "salt":          salt.to_string(),
            "maker":         creds.maker_address,
            "signer":        creds.maker_address,
            "taker":         "0x0000000000000000000000000000000000000000",
            "tokenId":       trade.asset_id,
            "makerAmount":   maker_amount.to_string(),
            "takerAmount":   taker_amount.to_string(),
            "expiration":    "0",
            "nonce":         "0",
            "feeRateBps":    "0",
            "side":          side_num.to_string(),
            "signatureType": "0",
            "signature":     signature
        },
        "owner":     creds.maker_address,
        "orderType": "FOK"
    });

    let body_str =
        serde_json::to_string(&body_value).map_err(|e| format!("serialize order body: {e}"))?;

    host::log(
        host::LogLevel::Info,
        &format!(
            "placing {} order: token={} price={} size={} makerAmt={} takerAmt={}",
            trade.side, trade.asset_id, price, size, maker_amount, taker_amount
        ),
    );

    let resp_body = clob_request("POST", "/order", Some(&body_str), creds, &state.config.clob_base)?;

    let resp: serde_json::Value = serde_json::from_slice(&resp_body)
        .map_err(|e| format!("parse /order response: {e}"))?;

    let order_id = resp
        .get("orderID")
        .or_else(|| resp.get("order_id"))
        .or_else(|| resp.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    host::log(
        host::LogLevel::Info,
        &format!("order placed: id={order_id} status={}", resp.get("status").and_then(|v| v.as_str()).unwrap_or("?")),
    );

    Ok(order_id)
}
