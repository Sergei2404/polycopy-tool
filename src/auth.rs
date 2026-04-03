//! Authentication helpers.
//!
//! L2: HMAC-SHA256 request signing — logic lifted from `bot/src/clob/auth.rs`.
//! Order: EIP-712 secp256k1 signing — pure Rust (k256 + sha3), WASM-safe.

use base64::{engine::general_purpose::URL_SAFE as B64, Engine};
use hmac::{Hmac, Mac};
use k256::ecdsa::SigningKey;
use sha2::Sha256;
use sha3::{Digest, Keccak256};

type HmacSha256 = Hmac<Sha256>;

// ─── L2 HMAC-SHA256 ───────────────────────────────────────────────────────────

/// Sign a CLOB REST request.
///
/// message = timestamp || METHOD || request_path_with_query || body
/// key     = base64url_decode(secret)
/// result  = base64url_encode(HMAC-SHA256(key, message))
pub fn sign_l2(
    secret: &str,
    method: &str,
    path: &str,
    body: &str,
    timestamp: u64,
) -> Result<String, String> {
    let message = format!("{}{}{}{}", timestamp, method.to_uppercase(), path, body);

    let key_bytes = B64
        .decode(secret)
        .map_err(|e| format!("base64 decode secret: {e}"))?;

    let mut mac =
        HmacSha256::new_from_slice(&key_bytes).map_err(|e| format!("HMAC init: {e}"))?;
    mac.update(message.as_bytes());

    Ok(B64.encode(mac.finalize().into_bytes()))
}

// ─── EIP-712 order signing ────────────────────────────────────────────────────

/// CTF Exchange on Polygon (binary YES/NO markets).
const CTF_EXCHANGE: &str = "4bfb41d5b3570defd03c39a9a4d8de6bd8b8982e";
/// NegRisk CTF Exchange on Polygon (multi-outcome markets).
const NEGRISK_EXCHANGE: &str = "c5d563a36ae78145c45a50134d48a1215220f80a";

const ORDER_TYPE: &str = "Order(uint256 salt,address maker,address signer,address taker,\
    uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,\
    uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)";

const DOMAIN_TYPE: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";

const CTF_NAME: &str = "Polymarket CTF Exchange";
const CTF_VERSION: &str = "1";
const POLYGON_CHAIN_ID: u64 = 137;

/// All fields needed to build and sign a Polymarket limit order.
pub struct OrderData<'a> {
    pub salt: u64,
    pub maker: &'a str,
    pub token_id: &'a str,
    pub maker_amount: u64,
    pub taker_amount: u64,
    pub side: u8,  // 0 = BUY, 1 = SELL
    pub neg_risk: bool,
}

/// Build, EIP-712 sign, and return the 65-byte hex signature for an order.
pub fn sign_order(private_key_hex: &str, order: &OrderData<'_>) -> Result<String, String> {
    let contract = if order.neg_risk {
        NEGRISK_EXCHANGE
    } else {
        CTF_EXCHANGE
    };

    let domain_sep = compute_domain_separator(contract);
    let struct_hash = compute_struct_hash(order);
    let signing_hash = eip712_digest(&domain_sep, &struct_hash);

    let key_hex = private_key_hex.trim_start_matches("0x");
    let key_bytes = hex::decode(key_hex).map_err(|e| format!("decode private key: {e}"))?;

    let signing_key =
        SigningKey::from_slice(&key_bytes).map_err(|e| format!("invalid private key: {e}"))?;

    let (sig, recid) = signing_key
        .sign_prehash_recoverable(&signing_hash)
        .map_err(|e| format!("sign_prehash: {e}"))?;

    let sig_bytes = sig.to_bytes();
    let v = recid.to_byte() + 27u8; // Polymarket uses v = 27 / 28

    let mut full = [0u8; 65];
    full[..64].copy_from_slice(&sig_bytes);
    full[64] = v;

    Ok(format!("0x{}", hex::encode(full)))
}

// ─── EIP-712 primitives ───────────────────────────────────────────────────────

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Keccak256::digest(data));
    out
}

/// ABI-encode a uint256 value (u64 fits all amounts and IDs we handle here).
fn enc_u64(v: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&v.to_be_bytes());
    out
}

/// ABI-encode a uint8 value padded to 32 bytes.
fn enc_u8(v: u8) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[31] = v;
    out
}

/// ABI-encode an Ethereum address (hex without 0x) padded to 32 bytes.
fn enc_addr(hex_addr: &str) -> [u8; 32] {
    let clean = hex_addr.trim_start_matches("0x");
    let bytes = hex::decode(clean).unwrap_or_default();
    let mut out = [0u8; 32];
    let start = 32usize.saturating_sub(bytes.len());
    out[start..].copy_from_slice(&bytes[bytes.len().saturating_sub(32)..]);
    out
}

/// Convert a decimal string (potentially 256-bit) to big-endian 32 bytes.
/// Used for token IDs that exceed u128.
fn enc_decimal(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        let digit = b - b'0';
        let mut carry = digit as u32;
        for byte in out.iter_mut().rev() {
            let v = (*byte as u32) * 10 + carry;
            *byte = (v & 0xff) as u8;
            carry = v >> 8;
        }
    }
    out
}

fn compute_domain_separator(contract_hex: &str) -> [u8; 32] {
    let domain_type_hash = keccak256(DOMAIN_TYPE.as_bytes());
    let name_hash = keccak256(CTF_NAME.as_bytes());
    let version_hash = keccak256(CTF_VERSION.as_bytes());

    let mut buf = Vec::with_capacity(5 * 32);
    buf.extend_from_slice(&domain_type_hash);
    buf.extend_from_slice(&name_hash);
    buf.extend_from_slice(&version_hash);
    buf.extend_from_slice(&enc_u64(POLYGON_CHAIN_ID));
    buf.extend_from_slice(&enc_addr(contract_hex));

    keccak256(&buf)
}

fn compute_struct_hash(order: &OrderData<'_>) -> [u8; 32] {
    let type_hash = keccak256(ORDER_TYPE.as_bytes());
    let taker_zero = "0000000000000000000000000000000000000000";

    let mut buf = Vec::with_capacity(13 * 32);
    buf.extend_from_slice(&type_hash);
    buf.extend_from_slice(&enc_u64(order.salt));
    buf.extend_from_slice(&enc_addr(order.maker));
    buf.extend_from_slice(&enc_addr(order.maker)); // signer == maker for EOA
    buf.extend_from_slice(&enc_addr(taker_zero));
    buf.extend_from_slice(&enc_decimal(order.token_id));
    buf.extend_from_slice(&enc_u64(order.maker_amount));
    buf.extend_from_slice(&enc_u64(order.taker_amount));
    buf.extend_from_slice(&enc_u64(0)); // expiration = 0 (no expiry)
    buf.extend_from_slice(&enc_u64(0)); // nonce = 0
    buf.extend_from_slice(&enc_u64(0)); // feeRateBps = 0
    buf.extend_from_slice(&enc_u8(order.side));
    buf.extend_from_slice(&enc_u8(0)); // signatureType = 0 (EOA)

    keccak256(&buf)
}

/// Final EIP-712 hash: keccak256(0x1901 || domainSeparator || structHash).
fn eip712_digest(domain_sep: &[u8; 32], struct_hash: &[u8; 32]) -> [u8; 32] {
    let mut msg = [0u8; 66];
    msg[0] = 0x19;
    msg[1] = 0x01;
    msg[2..34].copy_from_slice(domain_sep);
    msg[34..].copy_from_slice(struct_hash);
    keccak256(&msg)
}
