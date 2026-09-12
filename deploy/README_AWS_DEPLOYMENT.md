# AWS Lightsail & Domain Deployment Guide: trader.wilbergoose.com

This guide provides step-by-step instructions for deploying the **Dedicated Live Production Engine on AWS Lightsail (`eu-west-2` London)** and the **Web Dashboard on `trader.wilbergoose.com`** with complete host and workload isolation.

---

## Architecture & Workload Isolation

```
                                https://trader.wilbergoose.com
                                   (Vercel or Reverse Proxy)
                                               │
                      ┌────────────────────────┴────────────────────────┐
                      ▼                                                 ▼
             mode=paper / /paper                               mode=live / /live
        [🧪 PAPER STRATEGY SANDBOX]                       [🔴 LIVE REAL CAPITAL TERMINAL]
                      │                                                 │
                      ▼                                                 ▼
          Next.js BFF Proxy                                 Next.js BFF Proxy
                      │                                                 │
                      │ (PAPER_ENGINE_URL)                              │ (LIVE_ENGINE_URL)
                      ▼                                                 ▼
      ┌───────────────────────────────┐                 ┌───────────────────────────────┐
      │     Sandbox / Paper Host      │                 │    Dedicated Live Host (AWS)  │
      │  (Separate VM / Local / Dev)  │                 │    Lightsail eu-west-2 London │
      ├───────────────────────────────┤                 ├───────────────────────────────┤
      │ • Historical Candle Downloads │                 │ • ZERO Historical Downloads   │
      │ • Heavy Backtest Simulations  │                 │ • Sub-2ms Revolut X HTTP/2    │
      │ • In-memory Paper Simulator   │                 │ • Sub-9ms Kraken WS v2 Feeds  │
      │ • Strategy R&D Playground     │                 │ • Real Ed25519 Order Signing  │
      │                               │                 │ • Real Post-Only Limit Orders │
      │                               │                 │ • Strict No-£0 Fallback Rule  │
      └───────────────────────────────┘                 └───────────────────────────────┘
```

- **Dedicated Live Engine (`engine.wilbergoose.com` on AWS Lightsail `eu-west-2` London)**:
  - Sits $\approx 1 - 2\text{ ms}$ away from Revolut's London infrastructure and $\approx 9\text{ ms}$ from Kraken's Dublin infrastructure.
  - Runs with `TRADING_MODE=LIVE`. Backtesting and historical candle downloads are **strictly disabled** on this host to guarantee zero CPU/socket contention for live snipes.
  - Holds `revolut_private.pem` securely on local disk (`chmod 600`).
  - Protected behind Caddy (automatic Let's Encrypt TLS) and an internal `X-Engine-Secret` token.
  - Strict Live Rule: Never falls back to a £10 or £0 paper wallet. Refuses to trade if unauthenticated or balance is £0.00.
- **Web Dashboard (`trader.wilbergoose.com` on Vercel or Lightsail)**:
  - Gated behind a Master Access PIN with encrypted session cookies.
  - Server-side BFF proxy (`/api/proxy/*`) securely injects the `ENGINE_SECRET_KEY` on the server.
  - Dynamically routes Live requests to `LIVE_ENGINE_URL` and Paper/Backtest requests to `PAPER_ENGINE_URL`.

---

## Part 1: AWS Lightsail Setup (London `eu-west-2`)

### 1. Launch Lightsail Instance
1. Log in to [AWS Lightsail Console](https://lightsail.aws.amazon.com/).
2. Click **Create instance**.
3. Select **Region & Zone**: **London (`eu-west-2`)**.
4. Select Platform: **Linux/Unix**.
5. Select Blueprint: **OS Only $\to$ Ubuntu 22.04 LTS or 24.04 LTS**.
6. Choose Instance Plan: **\$5.00 / month (1 GB RAM, 1 vCPU)** or **\$10.00 / month (2 GB RAM)**.
7. Name your instance (e.g., `trading-engine-london`) and click **Create instance**.

### 2. Attach a Static IP & Open Ports
1. In Lightsail, go to the **Networking** tab.
2. Click **Create static IP**, attach it to your new instance, and note the public IP (e.g., `52.56.x.x`).
3. Under IPv4 Firewall, ensure only:
   - **SSH (Port 22)**
   - **HTTP (Port 80)**
   - **HTTPS (Port 443)**
   are open. **Do NOT open port 8000 to the public.**

### 3. Point DNS Records
In your DNS provider (e.g., Cloudflare, Route53, Namecheap for `wilbergoose.com`):
- Create an **A record**: `engine.wilbergoose.com` $\to$ Lightsail Static IP.
- If running the dashboard on Vercel:
  - Point `trader.wilbergoose.com` CNAME to `cname.vercel-dns.com`.
- If running the dashboard directly on Lightsail:
  - Point `trader.wilbergoose.com` A record to your Lightsail Static IP.

### 4. Run Automated Provisioning Script
SSH into your Lightsail instance:
```bash
# Clone the repository
git clone https://github.com/your-username/serene-lavoisier.git ~/serene-lavoisier
cd ~/serene-lavoisier

# Run setup
bash deploy/lightsail_setup.sh
```

### 5. Configure Revolut Credentials on Lightsail
1. Copy your `revolut_private.pem` to `~/serene-lavoisier/backend/credentials/revolut_private.pem`:
   ```bash
   chmod 600 ~/serene-lavoisier/backend/credentials/revolut_private.pem
   ```
2. Edit `~/serene-lavoisier/.env`:
   ```bash
   nano ~/serene-lavoisier/.env
   ```
   Set:
   ```env
   TRADING_MODE=LIVE
   REVOLUT_API_KEY=0fEG_your_revolut_api_key_here
   REVOLUT_PRIVATE_KEY_PATH=backend/credentials/revolut_private.pem
   ENGINE_SECRET_KEY=generate_a_secure_64_character_hex_secret
   ```
3. Restart Caddy and the engine service:
   ```bash
   sudo systemctl restart caddy
   sudo systemctl restart trading-engine
   ```

---

## Part 2: Dashboard Configuration for `trader.wilbergoose.com`

### If Deploying on Vercel:
In your Vercel Project Settings $\to$ Environment Variables:

| Variable | Value | Purpose |
| :--- | :--- | :--- |
| `LIVE_ENGINE_URL` | `https://engine.wilbergoose.com` | Dedicated AWS Lightsail London live engine |
| `PAPER_ENGINE_URL` | `https://sandbox.wilbergoose.com` *(or engine url)* | Sandbox / backtesting engine |
| `ENGINE_URL` | `https://engine.wilbergoose.com` | Fallback engine URL |
| `ENGINE_SECRET_KEY` | *(Same secret key as in Lightsail `.env`)* | Authenticates BFF proxy calls |
| `DASHBOARD_PIN` | `1234` *(Choose your secure 4–8 digit PIN)* | Master access PIN for dashboard |

Add custom domain `trader.wilbergoose.com` in Vercel.

---

## Part 3: Operational Verification

1. **Verify Dedicated Live Engine**:
   ```bash
   curl -i https://engine.wilbergoose.com/api/health
   # Expected: {"status":"healthy","host_mode":"LIVE", ...}
   ```
2. **Verify Backtests are Blocked on Live Host**:
   ```bash
   curl -X POST https://engine.wilbergoose.com/api/backtest/run
   # Expected: HTTP 403 Forbidden ("Backtest engine is disabled on Live Production host...")
   ```
3. **Verify Revolut X Live Credentials**:
   ```bash
   curl -H "X-Engine-Secret: YOUR_SECRET" https://engine.wilbergoose.com/api/live/diagnostics
   # Expected: {"status":"ACTIVE","authenticated":true,"can_trade":true,"latency_ms":...,"balances":{"GBP":...}}
   ```
4. **Visit `https://trader.wilbergoose.com`**:
   - Opens **Paper Sandbox** by default.
   - Click `[ 🔴 Live Capital ]` in the top header $\to$ confirmation modal appears $\to$ confirms $\to$ switches to Live Terminal with real Revolut X balances and active orders.
