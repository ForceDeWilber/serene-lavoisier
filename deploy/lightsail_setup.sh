#!/usr/bin/env bash
# ==============================================================================
# Serene Lavoisier: AWS Lightsail London (eu-west-2) Provisioning Script
# Sets up Ubuntu 22.04/24.04 LTS for low-latency, 24/7 trading execution.
# ==============================================================================

set -euo pipefail

echo ">>> [1/7] Updating apt repositories and installing core packages..."
sudo apt update -y
sudo apt install -y curl git ufw build-essential python3-venv python3-pip debian-keyring debian-archive-keyring apt-transport-https

echo ">>> [2/7] Installing Caddy Web Server (automatic Let's Encrypt TLS)..."
if ! command -v caddy &> /dev/null; then
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
    sudo apt update -y
    sudo apt install -y caddy
    echo "Caddy installed successfully."
else
    echo "Caddy already installed."
fi

echo ">>> [3/7] Setting up Rust toolchain for the execution core..."
if ! command -v cargo &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "Rust toolchain installed."
else
    echo "Rust toolchain already present."
fi

INSTALL_DIR="$HOME/serene-lavoisier"
if [ ! -d "$INSTALL_DIR" ]; then
    INSTALL_DIR="$(pwd)"
fi

echo ">>> [4/7] Setting up Python virtual environment in $INSTALL_DIR/backend/.venv..."
cd "$INSTALL_DIR"
if [ ! -d "backend/.venv" ]; then
    python3 -m venv backend/.venv
fi
./backend/.venv/bin/pip install --upgrade pip
./backend/.venv/bin/pip install -r backend/requirements.txt

echo ">>> [5/7] Configuring Environment Secrets & Permissions..."
if [ ! -f ".env" ]; then
    cp .env.example .env
    # Generate secure 64-char hex engine secret
    GENERATED_SECRET=$(openssl rand -hex 32)
    sed -i "s/ENGINE_SECRET_KEY=.*/ENGINE_SECRET_KEY=$GENERATED_SECRET/" .env
    echo "Created .env with generated ENGINE_SECRET_KEY: $GENERATED_SECRET"
fi

# Ensure credentials directory and private key permissions
mkdir -p backend/credentials
if [ -f "backend/credentials/revolut_private.pem" ]; then
    chmod 600 backend/credentials/revolut_private.pem
    echo "Secured backend/credentials/revolut_private.pem with chmod 600"
fi

echo ">>> [6/7] Installing and starting systemd trading service..."
sudo cp deploy/trading-engine.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable trading-engine
sudo systemctl restart trading-engine

echo ">>> [7/7] Configuring Caddy reverse proxy & UFW firewall..."
if [ -f "deploy/Caddyfile" ]; then
    sudo cp deploy/Caddyfile /etc/caddy/Caddyfile
    sudo systemctl restart caddy || true
fi

# Configure UFW
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow 22/tcp   # SSH
sudo ufw allow 80/tcp   # HTTP (ACME Let's Encrypt challenges)
sudo ufw allow 443/tcp  # HTTPS
# Note: Port 8000 remains strictly localhost only!
sudo ufw --force enable

echo "=============================================================================="
echo "✅ SERENE LAVOISIER PROVISIONING COMPLETE!"
echo "=============================================================================="
echo "Engine Service Status:  sudo systemctl status trading-engine"
echo "Live Service Logs:      journalctl -u trading-engine -f"
echo "Caddy Proxy Logs:       sudo journalctl -u caddy -f"
echo ""
echo "IMPORTANT: In Vercel Project Settings, add these Environment Variables:"
echo "  ENGINE_URL        = https://your-lightsail-domain.com"
ENGINE_KEY=$(grep '^ENGINE_SECRET_KEY=' .env | cut -d '=' -f2- || true)
echo "  ENGINE_SECRET_KEY = $ENGINE_KEY"
echo "  DASHBOARD_PIN     = <choose your 4-8 digit access PIN>"
echo "=============================================================================="
