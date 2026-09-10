# Multi-Venue Algorithmic Crypto Trading System (UK Spot)

A modular, low-latency cryptocurrency trading system operating in the UK spot market (GBP pairs) leveraging an asymmetric two-venue model:
- **Revolut X**: 0.00% maker fee limit order execution (`post_only`) for medium-frequency wide-range geometric grid and mean-reversion strategies.
- **Kraken Pro**: Sub-millisecond public WebSocket v2 data feeds for market intelligence, with execution reserved for higher-margin momentum breakouts.
- **FastAPI Control Plane**: Python 3.14 async orchestration daemon connected via Unix Domain Sockets (UDS) for runner lifecycle management and SQLite state persistence (`trading.db`).
- **Next.js Real-time Dashboard**: Modern Tailwind CSS interface for monitoring runner states, PnL, dynamic parameter tuning, and global emergency kill switch.
- **Discord Gateway Bot**: Read-only slash commands (`/status`, `/pnl`, `/runners`) and real-time alert embeds for fills and risk events.

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Next.js Web Dashboard                      │
│   - Real-time runner cards (PnL, active resting orders)         │
│   - Dynamic parameter tuning & global emergency kill switch     │
└────────────────────────────────┬────────────────────────────────┘
                                 │ HTTP / WebSockets (Port 8000)
┌────────────────────────────────▼────────────────────────────────┐
│                     FastAPI Control Plane                       │
│   - Runner lifecycle orchestration (spawn, pause, tune, stop)   │
│   - SQLite state persistence & Discord Gateway bot (/status)    │
└────────────────────────────────┬────────────────────────────────┘
                                 │ Unix Domain Socket (/tmp/trading_engine.sock)
┌────────────────────────────────▼────────────────────────────────┐
│                       Rust Core Daemon                          │
│                                                                 │
│  ┌───────────────────────┐             ┌─────────────────────┐  │
│  │ Kraken WS Multiplexer │             │ Central Risk Engine │  │
│  │ (L2 Books & Ticks)    │             │ - Capital Envelopes │  │
│  └──────────┬────────────┘             │ - Circuit Breakers  │  │
│             │ Broadcast                │ - Token-Bucket RL   │  │
│      ┌──────┴──────┐                   └──────────▲──────────┘  │
│      ▼             ▼                              │ Order Intent│
│  ┌──────────┐ ┌──────────┐                        │             │
│  │ Runner 1 │ │ Runner 2 │ ───────────────────────┘             │
│  │ BTC Grid │ │ ETH Grid │                                      │
│  └──────────┘ └──────────┘                                      │
└──────────────┬─────────────────────────────┬────────────────────┘
               │ HTTP/2 REST (Ed25519)       │ WS Order API
               ▼                             ▼
        Revolut X Spot                 Kraken Pro Spot
```

---

## Monorepo Layout

```
serene-lavoisier/
├── Cargo.toml                       # Root Cargo workspace definition
├── Makefile                         # Unified development targets
├── .env.example                     # Environment template
├── crates/
│   ├── trading-core/                # Domain models, Risk engine, Simulator, Grid strategy
│   ├── kraken-client/               # Kraken WS v2 multiplexer client with reconnect
│   ├── revolut-client/              # Revolut X HTTP/2 REST client + Ed25519 signer + Rate limiter
│   └── engine-daemon/               # Main Tokio execution binary + UDS IPC server
├── backend/                         # FastAPI Control Plane (Python 3.14)
│   ├── requirements.txt
│   ├── run.py                       # Launcher script
│   └── app/
│       ├── main.py                  # FastAPI REST and WebSocket server
│       ├── ipc_client.py            # Async UDS client to Rust engine
│       ├── database.py & models.py  # SQLite async persistence
│       └── discord_bot.py           # Discord Gateway Bot (Read-only slash commands)
└── frontend/                        # Next.js 15 Web Dashboard
    ├── package.json
    └── src/app/                     # Tailwind UI with real-time WebSocket telemetry
```

---

## Key Risk & Safety Safeguards

1. **Deterministic Hot Path (Zero AI)**: All trade sizing, grid math, and execution pathways use deterministic mathematical rules. Generative AI is strictly excluded from the low-latency execution path.
2. **Capital Envelopes**: Each runner is partitioned with an isolated capital pool (£500 default per runner). Runners cannot overdraw or draw from unallocated balances.
3. **Adverse Selection Lag Filter**: Real-time Kraken WS monitoring tracks downward price velocity. If a sudden plunge is detected ($\ge 0.60\%$ drop within 60 seconds), all resting Revolut X buy orders are canceled instantly to prevent toxic fills.
4. **Global Circuit Breaker**: Rolling 2-hour window portfolio drawdown monitor. If portfolio equity drops $> 5.0\%$, the breaker trips, automatically cancels all open resting orders across venues, and pauses all runners.
5. **Token-Bucket Rate Limiter**: Strictly caps Revolut X REST requests below 900 req/min (ceiling: 1,000 req/min).

---

## Quick Start & Verification

### 1. Run Unit Tests
```bash
make test
# Runs 8 tests covering math, risk engine, Ed25519 signer, rate limiter, and simulator
```

### 2. Launch Services

**Terminal 1 — Rust Execution Daemon (Paper Mode by default):**
```bash
make engine
```

**Terminal 2 — FastAPI Control Plane:**
```bash
make backend
```

**Terminal 3 — Next.js Web Dashboard:**
```bash
make frontend
```
Navigate to `http://localhost:3000` to view the live dashboard, monitor runner telemetry, tune grid parameters dynamically, or test the emergency kill switch.
