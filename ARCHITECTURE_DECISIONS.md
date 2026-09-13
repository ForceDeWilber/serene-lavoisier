# Architectural Decisions & Strategy Specifications

## 1. Single Decision & Execution Engine (Pure Rust Core)
The system's execution and strategy decision-making are consolidated strictly into **Rust (`crates/engine-daemon`, `crates/trading-core`, `crates/revolut-client`)**:
- **Sole Authority:** All market data ingestion, lead-lag signal evaluation, risk engine calculations, and order placements/cancellations execute deterministically in Rust.
- **Microsecond Hot Path:** Utilizes Tokio async tasks, sub-millisecond JSON parsing, pre-warmed HTTP/2 TLS connection pools, and `ed25519-dalek` cryptographic signing (~15 microseconds) to achieve true low-latency execution.
- **Role of Python (`backend/app`):** Demoted to a lightweight telemetry interlayer between the Rust engine and the Next.js web dashboard. Python makes zero trading decisions, holds no live signing authority on execution hot paths, and merely relays telemetry and UI tuning commands over IPC (Unix Domain Socket / TCP).

---

## 2. Asymmetric Dual-Venue Model
The engine operates across two venues with distinct asymmetric roles:
1. **Kraken Pro WebSocket v2 (Informational Oracle):**
   - Ingests real-time tick-by-tick market data and top-of-book quotes for BTC/GBP and ETH/GBP.
   - **Zero trades are placed on Kraken.** Kraken serves purely as a high-speed price discovery oracle.
2. **Revolut X Spot (Execution Venue):**
   - All live trades, liquidity provision, and latency snipes execute exclusively on Revolut X.
   - Leverages Revolut X's fee structure:
     - **Maker Fee:** `0.00%` (`post_only` limit orders)
     - **Taker Fee:** `0.09%` (aggressive market / spread-crossing orders)

---

## 3. High-Frequency Strategies in Rust

### Strategy A: Stale Quote Lead-Lag Momentum Sniper
- **Concept:** Secondary exchanges (Revolut X) lag primary liquid venues (Kraken) by ~100ms to 1s during sudden price impulses. The Sniper detects a Kraken breakout and captures the stale quote on Revolut X before Revolut X's internal market makers adjust their quotes.
- **Fee Hurdle & Quantitative Threshold:**
  - Sniping a stale quote requires an aggressive **Taker order** (crossing the spread to lift the stale ask or hit the stale bid), incurring a **0.09% taker fee**.
  - **Threshold Rule:** The engine strictly enforces a minimum price dislocation hurdle:
    $$\Delta P_{\text{dislocation}} \ge P_{\text{entry}} \times (\text{Taker Fee [0.09\%]} + \text{Target Net Edge [}\ge 0.01\%\text{–}0.03\%\text{]})$$
    The signal requires a minimum **0.10% to 0.12%** price dislocation before triggering (e.g. $\ge £60\text{–}£72$ impulse on BTC at £60,000).
- **Two-Leg Execution Cycle:**
  1. **Leg 1 (Snipe Entry):** Instant Taker order lifting the stale Revolut X ask (pays 0.09% fee).
  2. **Leg 2 (Profit Exit):** Immediate `post_only` Maker limit sell placed at the new equilibrium price predicted by Kraken (pays 0.00% maker fee), capturing $\ge 0.10\% - 0.12\%$ gross swing.
  3. **Scratch / Timeout Guard:** If Revolut X's quote stalls and the maker exit does not fill within 500–1000ms, the engine scratches the position at market to prevent holding a reverse-moving knife.
- **Spot Inventory Constraints:**
  - *Bullish Spike:* Deploy available GBP to buy stale ask $\to$ sell at higher target.
  - *Bearish Drop:* Requires pre-existing crypto inventory to sell stale bid $\to$ buy back lower (no short-selling on spot).

### Strategy B: Geometric Maker Grid (Passive Liquidity Harvesting)
- **Concept:** Continuous resting `post_only` buy and sell rungs capturing 0.40% geometric volatility oscillations at **0.00% maker fees**.
- **Adverse Selection Protection (Lag Filter):**
  - Because resting bids are exposed to toxic flow during crashes, the Rust engine continuously monitors Kraken WS price velocity.
  - If Kraken drops $\ge 0.60\%$ within 1 second, the Rust engine issues sub-millisecond cancellations of all resting Revolut X buy orders before toxic flow sweeps the book.

---

## 4. Live Order Tracking, Rehydration & State Persistence
- **Rehydration:** On boot, the Rust engine unconditionally queries `GET /api/1.0/orders/active` to pull resting orders directly into the in-memory strategy state.
- **Fill Verification:** Orders missing from the active set are verified against execution fill history before assuming 100% fill, preventing phantom fills from external cancellations.
- **Persistence:** Every order lifecycle transition (OPEN, FILLED, CANCELED) is written to SQLite (`trading_live.db`) to ensure auditability and prevent lot price loss across process restarts.
- **Risk Envelopes:** The Rust `CentralRiskEngine` enforces capital limits (£500 default envelopes) and a rolling 5% portfolio drawdown circuit breaker.
