import os
from pathlib import Path
from dotenv import load_dotenv

# Load .env from project root
ROOT_DIR = Path(__file__).resolve().parent.parent.parent
load_dotenv(ROOT_DIR / ".env")
load_dotenv() # Fallback to local .env

ENGINE_UDS_PATH = os.getenv("ENGINE_UDS_PATH", "/tmp/trading_engine.sock")
DATABASE_URL = os.getenv("DATABASE_URL", f"sqlite+aiosqlite:///{ROOT_DIR}/trading.db")

DISCORD_BOT_TOKEN = os.getenv("DISCORD_BOT_TOKEN", "")
DISCORD_CHANNEL_ID = os.getenv("DISCORD_CHANNEL_ID", "")

# Revolut X Exchange Credentials (Ed25519)
REVOLUT_API_KEY = os.getenv("REVOLUT_API_KEY", "")
REVOLUT_PRIVATE_KEY_PATH = os.getenv("REVOLUT_PRIVATE_KEY_PATH", "backend/credentials/revolut_private.pem")
TRADING_MODE = os.getenv("TRADING_MODE", "PAPER") # "PAPER" or "LIVE"

# Lightsail Engine Authentication & CORS Protection
ENGINE_SECRET_KEY = os.getenv("ENGINE_SECRET_KEY", None)
ALLOWED_CORS_ORIGINS = [
    origin.strip() for origin in os.getenv("ALLOWED_CORS_ORIGINS", "*").split(",") if origin.strip()
]

