# Architectural Decisions & Findings

## 1. Engine Division of Responsibilities
The system currently operates a dual-language architecture:
- **Rust (`crates/engine-daemon`)**: Acts as the high-throughput "Muscle". It handles the massive data ingest from Kraken WebSockets, maintains the Central Risk Engine envelopes, and runs the high-performance Paper Trading sandbox. 
- **Python (`backend/app`)**: Acts as the orchestrator and Live API gateway. It holds the Ed25519 private keys, signs HTTP payloads, manages the SQLite database, and handles the Next.js Dashboard telemetry.

## 2. Why Live Trading is in Python (For Now)
Currently, the Live Engine (`live_trading_runner.py`) exclusively runs a **Maker Grid Strategy** (placing `post_only` limit orders). 
Because the bot rests on the order book waiting for the market to come to it (capturing 0.40% geometric swings), it does not need to race other bots. The ~20-50ms overhead of Python serializing JSON and calculating Ed25519 signatures doesn't impact profitability for resting maker orders. Python also allows for rapid iteration when dealing with messy exchange JSON and API quirks.

## 3. The Sniper Bot Latency Constraint
The **Stale Quote Sniper** relies on latency arbitrage—spotting a price spike on Kraken and aggressively crossing the Revolut X spread to grab cash before Revolut X can update its quotes (usually a 50-100ms window).

**Finding:** The Sniper is currently enabled *only* in the Paper Sandbox. It is intentionally disabled in the Python Live Engine. 
**Reasoning:** If we attempted a latency snipe in Python, the time taken to detect the spike, sign the cryptography, and dispatch the HTTP POST would often exceed the 50-100ms lag window. We would miss the quote or suffer severe slippage.

**Next Architectural Step:** Before the Sniper bot can be unleashed with Live Capital, the execution and cryptographic signing layer *must* be ported to Rust. Rust's sub-millisecond execution speeds are mandatory to successfully beat Revolut X's internal market makers.

## 4. Live Grid Tracking & Rehydration (Fixed)
We identified and patched a "haze" risk where the Python backend could lose track of open orders if it restarted. 
- **Rehydration:** On boot, the engine now unconditionally queries `GET /api/1.0/orders/active` and safely pulls any active resting orders directly back into memory. 
- **Decoupled Polling:** The live sync loop now aggressively checks for fills every 2 seconds regardless of free GBP balance (since free GBP drops to ~£0.00 when fully deployed on the order book). 
- **Persistence:** Every placed order and fill is now immutably logged to `trading_live.db` to guarantee purchase lot prices are never lost.
