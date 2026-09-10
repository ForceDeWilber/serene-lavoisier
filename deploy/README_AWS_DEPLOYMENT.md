# AWS Lightsail & Vercel Deployment Guide

This guide provides step-by-step instructions for deploying the **Trading Engine on AWS Lightsail (`eu-west-2` London)** and the **Production Dashboard on Vercel**.

---

## Architecture Overview

- **AWS Lightsail (`eu-west-2` London)**:
  - Runs the low-latency Python/FastAPI control plane & Rust execution core.
  - Sits $\approx 1 - 2\text{ ms}$ away from Revolut's London infrastructure and $\approx 9\text{ ms}$ from Kraken's Dublin infrastructure.
  - Holds `revolut_private.pem` securely on local disk (`chmod 600`).
  - Protected behind Caddy (automatic Let's Encrypt TLS) and an internal `X-Engine-Secret` token.
  - Direct port 8000 is closed to the outside world; only HTTPS (port 443) is open.
- **Vercel (Next.js Dashboard)**:
  - Gated behind a Master Access PIN with an encrypted session cookie.
  - Server-side BFF proxy (`/api/proxy/*`) securely injects the `ENGINE_SECRET_KEY` on the server so it is never exposed in client JavaScript.

---

## Part 1: AWS Lightsail Setup (London `eu-west-2`)

### 1. Launch Lightsail Instance
1. Log in to [AWS Lightsail Console](https://lightsail.aws.amazon.com/).
2. Click **Create instance**.
3. Select **Region & Zone**: **London (`eu-west-2`)**.
4. Select Platform: **Linux/Unix**.
5. Select Blueprint: **OS Only $\to$ Ubuntu 22.04 LTS or 24.04 LTS**.
6. Choose Instance Plan: **\$5.00 / month (1 GB RAM, 1 vCPU, 40 GB SSD, 1 TB transfer)** or **\$10.00 / month (2 GB RAM)**.
7. Name your instance (e.g., `trading-engine-london`) and click **Create instance**.

### 2. Attach a Static IP & Open Ports
1. In Lightsail, go to the **Networking** tab.
2. Click **Create static IP**, attach it to your new instance, and note the public IP (e.g., `52.56.x.x`).
3. Under the instance's **Networking $\to$ IPv4 Firewall**, ensure only:
   - **SSH (Port 22)**
   - **HTTP (Port 80)**
   - **HTTPS (Port 443)**
   are open. **Do NOT open port 8000.**

### 3. Point a Domain or Free Subdomain
- Point a domain or subdomain (e.g. `engine.yourdomain.com` or a free [DuckDNS](https://www.duckdns.org/) subdomain `mytrader.duckdns.org`) A-record to your Lightsail Static IP.

### 4. Run the Automated Provisioning Script
Connect to your Lightsail instance via SSH (using the Lightsail browser console or your local terminal):

```bash
# Clone the repository
git clone https://github.com/your-username/your-repo.git ~/serene-lavoisier
cd ~/serene-lavoisier

# Run the setup script
bash deploy/lightsail_setup.sh
```

### 5. Configure Revolut Credentials on Lightsail
1. Copy your `revolut_private.pem` to `~/serene-lavoisier/backend/credentials/revolut_private.pem`:
   ```bash
   chmod 600 ~/serene-lavoisier/backend/credentials/revolut_private.pem
   ```
2. Edit `~/serene-lavoisier/.env` with your Revolut API Key and public domain:
   ```bash
   nano ~/serene-lavoisier/.env
   ```
   Set:
   ```env
   REVOLUT_API_KEY=0fEG...your-64-char-key...
   REVOLUT_PRIVATE_KEY_PATH=backend/credentials/revolut_private.pem
   TRADING_MODE=PAPER  # Change to LIVE when ready
   ENGINE_DOMAIN=engine.yourdomain.com
   ```
3. Update Caddy with your domain in `/etc/caddy/Caddyfile`:
   ```caddyfile
   engine.yourdomain.com {
       reverse_proxy 127.0.0.1:8000
   }
   ```
   Reload Caddy:
   ```bash
   sudo systemctl restart caddy
   sudo systemctl restart trading-engine
   ```

---

## Part 2: Vercel Setup (Next.js Production Dashboard)

### 1. Import Repository into Vercel
1. Go to [Vercel Dashboard](https://vercel.com/) and click **Add New $\to$ Project**.
2. Select your repository.
3. In **Root Directory**, select `frontend`.

### 2. Configure Environment Variables in Vercel
In the Vercel project deployment settings, add the following Environment Variables:

| Variable | Value | Purpose |
| :--- | :--- | :--- |
| `ENGINE_URL` | `https://engine.yourdomain.com` | Public HTTPS endpoint of your Lightsail engine |
| `ENGINE_SECRET_KEY` | *(Value of `ENGINE_SECRET_KEY` from Lightsail `.env`)* | Authenticates Vercel server proxy calls to Lightsail |
| `DASHBOARD_PIN` | `1234` *(Choose your secure 4–8 digit PIN)* | Master PIN required to unlock the web terminal |
| `NEXT_PUBLIC_ENGINE_WS_URL` | `wss://engine.yourdomain.com/api/ws/stream?token=YOUR_SECRET` | (Optional) Direct WSS stream if using direct WebSockets |

### 3. Deploy
Click **Deploy**. 

---

## Part 3: Operational Verification Checklist

1. **Verify Lightsail Engine Health**:
   ```bash
   curl -i https://engine.yourdomain.com/api/health
   # Expected: HTTP 200 OK {"status":"healthy", ...}
   ```
2. **Verify Engine Rejects Unauthenticated Access**:
   ```bash
   curl -i https://engine.yourdomain.com/api/telemetry
   # Expected: HTTP 401 Unauthorized
   ```
3. **Verify Authenticated Engine Access**:
   ```bash
   curl -i -H "X-Engine-Secret: YOUR_SECRET" https://engine.yourdomain.com/api/telemetry
   # Expected: HTTP 200 OK with live telemetry payload
   ```
4. **Verify Vercel Production Dashboard**:
   - Open `https://your-project.vercel.app/dashboard`.
   - You will be greeted by the **Trading Desk Gate** PIN screen.
   - Enter your `DASHBOARD_PIN` to unlock the terminal.
   - Observe live ticker pricing, 4-card portfolio HUD, capital compounding HUD, and runner controls!
   - Click the **Lock** icon in the top header to lock the terminal at any time.

---

## Useful Lightsail Commands

```bash
# Check trading engine status
sudo systemctl status trading-engine

# View live trading engine logs
journalctl -u trading-engine -f

# Restart trading engine
sudo systemctl restart trading-engine

# View Caddy TLS / access logs
sudo journalctl -u caddy -f
```
