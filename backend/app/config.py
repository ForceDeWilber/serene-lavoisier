import os
from pathlib import Path
from dotenv import load_dotenv

# Load .env from project root with override=True
ROOT_DIR = Path(__file__).resolve().parent.parent.parent
load_dotenv(ROOT_DIR / ".env", override=True)
load_dotenv(override=True) # Fallback to local .env

import sys

ENGINE_UDS_PATH = os.getenv("ENGINE_UDS_PATH", "127.0.0.1:9099" if sys.platform == "win32" else "/tmp/trading_engine.sock")

# Isolated Databases for Paper vs Live Execution
DATABASE_URL_PAPER = os.getenv("DATABASE_URL_PAPER", f"sqlite+aiosqlite:///{ROOT_DIR}/trading_paper.db")
DATABASE_URL_LIVE = os.getenv("DATABASE_URL_LIVE", f"sqlite+aiosqlite:///{ROOT_DIR}/trading_live.db")
DATABASE_URL = os.getenv("DATABASE_URL", DATABASE_URL_PAPER)

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

