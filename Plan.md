# polycopy-tool — MVP

## What it is

WASM Claude Agent SDK tool. The agent calls it on a schedule (e.g. every 30 s).
Each call: scan watched wallets for new trades → copy any not yet seen → return updated state.
No native daemon. No WebSocket. Pure HTTP polling via `host::http_request`.

## Core loop (`sync`)

```
1. Load state: context JSON  →  polycopy-state.json (first run)
2. For each wallet in config.watch_wallets:
     GET /trades?maker_address={addr}&after={last_synced_s}
3. Skip trade IDs already in copied_orders
4. For each new trade:
     - Build FOK order (same token_id / side / price, size = ⌈max_bid_usdc / price⌉)
     - EIP-712 sign with private_key_hex
     - POST /order  (L2 HMAC-SHA256 headers)
     - Append trade.id to copied_orders
5. last_synced_ms = host::now_millis()
6. Return { result: { new, placed, failed, skipped }, state: <full updated state> }
```

Agent stores `state` from the response and passes it as `context` on the next call.

## State schema

```json
{
  "config": {
    "watch_wallets": ["0x..."],
    "max_bid_usdc": 5.0,
    "neg_risk_token_ids": [],
    "clob_base": "https://clob.polymarket.com"
  },
  "credentials": {
    "api_key":         "...",
    "secret":          "...",
    "passphrase":      "...",
    "private_key_hex": "0x...",
    "maker_address":   "0x..."
  },
  "last_synced_ms": 0,
  "copied_orders":  []
}
```

## Credential setup

Run the native bot once — it performs L1 EIP-712 key derivation and logs the resulting
`api_key / secret / passphrase`. Paste them into `polycopy-state.json` alongside the
private key and maker address. Commit the file to the workspace root so the tool can
read it on first run via `host::workspace_read("polycopy-state.json")`.

After the first call the agent owns the state through the context round-trip; the file
is only needed once.

## Auth

| Layer | Algorithm | When |
|-------|-----------|------|
| L2 request auth | HMAC-SHA256 | Every CLOB REST call (lifted from `bot/src/clob/auth.rs`) |
| Order signing   | EIP-712 secp256k1 | `POST /order` body signature |

Dependencies: `hmac + sha2` (L2), `k256 + sha3` (EIP-712) — all pure Rust, WASM-safe.

## Order amounts

```
size = ⌈max_bid_usdc / price⌉  tokens

BUY:   makerAmount = ⌈size × price × 10⁶⌉   (µUSDC)    takerAmount = size × 10⁶   (µtokens)
SELL:  makerAmount =  size × 10⁶              (µtokens)   takerAmount = ⌈size × price × 10⁶⌉  (µUSDC)
```

## Not in MVP (easy v2 additions)

- Daily spend cap — add `daily: { date, spent_usdc }` to state, check before placing
- NegRisk auto-detection — currently: list token IDs in `config.neg_risk_token_ids`
- L1 key derivation in WASM — needs secp256k1 + EIP-712; paste pre-derived creds for now

## File layout

```
polycopy-tool/
├── Plan.md
├── Cargo.toml
├── wit/tool.wit                     copy from ironclaw/wit/tool.wit
├── src/
│   ├── lib.rs                       WIT glue, action dispatch
│   ├── types.rs                     BotAction, State, Config, Credentials, Trade
│   ├── state.rs                     load() / to_json()
│   ├── auth.rs                      sign_l2(), sign_order(), EIP-712 primitives
│   └── api.rs                       fetch_trades(), place_order()
├── polycopy-state.json              bootstrap template (fill in before first run)
└── polycopy-tool.capabilities.json
```
