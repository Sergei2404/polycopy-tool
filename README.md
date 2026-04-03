# polycopy-tool — local testing guide

## 1. Prerequisites

```bash
# Rust WASM target (wasip2 = WASI Preview 2, required by ironclaw)
rustup target add wasm32-wasip2
```

## 2. Fill in credentials

Open `polycopy-tool/polycopy-state.json` and fill in the five values:

```json
"credentials": {
  "api_key":         "...",   // from bot startup log: "API credentials derived"
  "secret":          "...",   // base64url string
  "passphrase":      "...",
  "private_key_hex": "0x...", // same key as POLYMARKET_PRIVATE_KEY
  "maker_address":   "0x..."  // your Polymarket wallet address
}
```

If you don't have `api_key / secret / passphrase` yet, run the native bot once — it
derives and logs them automatically on startup via EIP-712.

Also add at least one wallet to watch:

```json
"config": {
  "watch_wallets": ["0xTARGET_WALLET"],
  "max_bid_usdc": 1.0,
  ...
}
```

## 3. Build

```bash
cd polycopy-tool
cargo build --target wasm32-wasip2 --release
# output: target/wasm32-wasip2/release/polycopy_tool.wasm
```

## 4. Install

```bash
# From the ironclaw project root:
ironclaw tool install /path/to/polycopycniper/polycopy-tool/target/wasm32-wasip2/release/polycopy_tool.wasm \
  --capabilities /path/to/polycopycniper/polycopy-tool/polycopy-tool.capabilities.json
```

Copy `polycopy-state.json` to the ironclaw workspace root (where ironclaw runs):

```bash
cp polycopy-tool/polycopy-state.json ~/path/to/ironclaw-workspace/polycopy-state.json
```

## 5. Test manually

In the ironclaw chat, or via CLI:

```
# Check it loads and reads state
{"action": "get_status"}

# Add a wallet to watch
{"action": "add_wallet", "wallet": "0xWATCH_ADDRESS"}

# Run one sync cycle (scans trades, copies any new ones)
{"action": "sync"}
```

The first `sync` fetches all available trade history for the wallet (`last_synced_ms=0`).
Every subsequent call only looks at trades since the previous sync.

## 6. Schedule recurring syncs

Set up an ironclaw routine or schedule to call `sync` every 30 s:

```
/schedule "every 30 seconds" call polycopy-tool with {"action": "sync"}
```

Pass the `state` field from each response back as `context` on the next call.
Without context the tool re-reads `polycopy-state.json` and starts fresh.

## Troubleshooting

| Symptom | Likely cause |
|---------|-------------|
| `polycopy-state.json not found` | File not in ironclaw workspace root |
| `base64 decode secret` error | Wrong `secret` format — must be raw base64url as returned by Polymarket |
| HTTP 401 from CLOB | `api_key` / `passphrase` wrong, or credentials expired (re-derive via bot) |
| `price out of valid range` | Trade has price ≤ 0.001 or ≥ 0.999 — these are filtered, expected |
| Orders placed but not filled | FOK orders fail if no liquidity at that price — normal for thin markets |
