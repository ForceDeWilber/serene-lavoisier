import os
import time
import json
import base64
import logging
from pathlib import Path
from typing import Dict, Any, Optional
import httpx

logger = logging.getLogger("trading_system.capital_manager")
STATE_FILE = Path(__file__).resolve().parent.parent.parent / "capital_state.json"

class CapitalManager:
    """
    Manages account balance auto-detection, dynamic capital split allocation,
    compounded trading power, and high-water mark dynamic profit locking.
    """
    def __init__(self, starting_balance_gbp: float = 10.00):
        self.balance_source: str = "PAPER_WALLET"
        self.starting_balance_gbp: float = starting_balance_gbp
        self.settled_cash_gbp: float = starting_balance_gbp
        self.crypto_balances: Dict[str, float] = {
            "BTC": 0.0,
            "ETH": 0.0,
            "SOL": 0.0,
        }

        # Dynamic Profit Lock Configuration
        # Default: 30% of realized profits are locked permanently into a secure reserve vault
        self.profit_lock_pct: float = 0.30
        self.locked_profit_gbp: float = 0.0
        self.cumulative_profit_gbp: float = 0.0

        # Dynamic Runner Splits
        self.split_btc_pct: float = 0.50
        self.split_eth_pct: float = 0.50
        self.rungs_per_side: int = 3 # Default 3 rungs per side for micro accounts (e.g. £1.66 each on £5 envelope)
        self.min_order_clip_gbp: float = 1.00 # Revolut X micro-order floor

        # Revolut Credentials
        self.revolut_api_key: Optional[str] = os.getenv("REVOLUT_API_KEY")
        self.revolut_priv_key_path: str = os.getenv(
            "REVOLUT_PRIVATE_KEY_PATH", "backend/credentials/revolut_private.pem"
        )
        self.revolut_base_url: str = os.getenv("REVOLUT_BASE_URL", "https://revx.revolut.com")

        self._load_state()

    def _load_state(self):
        try:
            if STATE_FILE.exists():
                with open(STATE_FILE, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    if "starting_balance_gbp" in data:
                        self.starting_balance_gbp = float(data["starting_balance_gbp"])
                    if "profit_lock_pct" in data:
                        self.profit_lock_pct = float(data["profit_lock_pct"])
                    if "locked_profit_gbp" in data:
                        self.locked_profit_gbp = float(data["locked_profit_gbp"])
                    if "cumulative_profit_gbp" in data:
                        self.cumulative_profit_gbp = float(data["cumulative_profit_gbp"])
                    if "split_btc_pct" in data:
                        self.split_btc_pct = float(data["split_btc_pct"])
                    if "split_eth_pct" in data:
                        self.split_eth_pct = float(data["split_eth_pct"])
                    if "rungs_per_side" in data:
                        self.rungs_per_side = int(data["rungs_per_side"])
                logger.info(f"Loaded capital state from {STATE_FILE} (starting_balance_gbp: £{self.starting_balance_gbp:.2f})")
        except Exception as e:
            logger.warning(f"Failed to load capital state from {STATE_FILE}: {e}")

    def _save_state(self):
        try:
            data = {
                "starting_balance_gbp": self.starting_balance_gbp,
                "profit_lock_pct": self.profit_lock_pct,
                "locked_profit_gbp": self.locked_profit_gbp,
                "cumulative_profit_gbp": self.cumulative_profit_gbp,
                "split_btc_pct": self.split_btc_pct,
                "split_eth_pct": self.split_eth_pct,
                "rungs_per_side": self.rungs_per_side,
            }
            with open(STATE_FILE, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2)
        except Exception as e:
            logger.warning(f"Failed to save capital state to {STATE_FILE}: {e}")

    async def detect_balances(self) -> Dict[str, Any]:
        """
        Attempts to detect live balances from Revolut X.
        Falls back cleanly to paper wallet if credentials are not configured.
        """
        live_res = await self.query_revolut_balances()
        if live_res is not None:
            self.balance_source = "REVOLUT_LIVE"
            self.settled_cash_gbp = live_res.get("GBP", self.settled_cash_gbp)
            self.crypto_balances["BTC"] = live_res.get("BTC", self.crypto_balances["BTC"])
            self.crypto_balances["ETH"] = live_res.get("ETH", self.crypto_balances["ETH"])
            self.crypto_balances["SOL"] = live_res.get("SOL", self.crypto_balances.get("SOL", 0.0))
            logger.info(f"Auto-detected Revolut X live balances: GBP £{self.settled_cash_gbp:,.2f}")
            return {
                "status": "success",
                "source": "REVOLUT_LIVE",
                "balances": {
                    "GBP": self.settled_cash_gbp,
                    "BTC": self.crypto_balances["BTC"],
                    "ETH": self.crypto_balances["ETH"],
                    "SOL": self.crypto_balances["SOL"],
                },
            }

        self.balance_source = "PAPER_WALLET"
        return {
            "status": "paper_mode",
            "source": "PAPER_WALLET",
            "balances": {
                "GBP": self.settled_cash_gbp,
                "BTC": self.crypto_balances["BTC"],
                "ETH": self.crypto_balances["ETH"],
                "SOL": self.crypto_balances["SOL"],
            },
        }

    async def query_revolut_balances(self) -> Optional[Dict[str, float]]:
        """
        Queries Revolut X GET /api/1.0/balances using Ed25519 authentication.
        """
        api_key = self.revolut_api_key or os.getenv("REVOLUT_API_KEY")
        priv_path = self.revolut_priv_key_path

        if not api_key or not os.path.exists(priv_path):
            return None

        try:
            from cryptography.hazmat.primitives.asymmetric import ed25519
            from cryptography.hazmat.primitives import serialization

            with open(priv_path, "rb") as f:
                private_key = serialization.load_pem_private_key(f.read(), password=None)

            if not isinstance(private_key, ed25519.Ed25519PrivateKey):
                logger.warning("Private key is not an Ed25519 key")
                return None

            timestamp = str(int(time.time() * 1000))
            method = "GET"
            path = "/api/1.0/balances"
            payload_str = f"{timestamp}{method}{path}"
            signature_bytes = private_key.sign(payload_str.encode("utf-8"))
            signature_b64 = base64.b64encode(signature_bytes).decode("utf-8")

            headers = {
                "X-Revx-API-Key": api_key,
                "X-Revx-Timestamp": timestamp,
                "X-Revx-Signature": signature_b64,
                "Accept": "application/json",
            }

            url = f"{self.revolut_base_url}{path}"
            async with httpx.AsyncClient(timeout=6.0) as client:
                resp = await client.get(url, headers=headers)
                if resp.status_code == 200:
                    data = resp.json()
                    balances: Dict[str, float] = {}
                    for item in data:
                        curr = item.get("currency")
                        avail = float(item.get("available", 0.0))
                        if curr:
                            balances[curr] = avail
                    return balances
                else:
                    logger.warning(f"Revolut X balances query returned HTTP {resp.status_code}: {resp.text}")
                    return None
        except Exception as e:
            logger.error(f"Error querying Revolut X balances: {e}")
            return None

    def record_trade_profit(self, profit_gbp: float):
        """
        Records realized net profit and updates the high-water mark locked profit reserve vault.
        """
        if profit_gbp <= 0:
            return

        self.cumulative_profit_gbp = round(self.cumulative_profit_gbp + profit_gbp, 2)

        # High-water mark ratchet: profits once locked into the reserve vault can never decrease
        target_locked = round(self.cumulative_profit_gbp * self.profit_lock_pct, 2)
        if target_locked > self.locked_profit_gbp:
            self.locked_profit_gbp = target_locked
        self._save_state()

    def get_active_trading_power(self) -> float:
        """
        Calculates compounded active trading power:
        Trading Power = Starting Balance + Unlocked Reinvested Profits
        """
        unlocked_profit = max(0.0, round(self.cumulative_profit_gbp - self.locked_profit_gbp, 2))
        trading_power = round(self.starting_balance_gbp + unlocked_profit, 2)
        return max(self.min_order_clip_gbp * 2, trading_power)

    def get_allocations(self) -> Dict[str, Any]:
        """
        Computes dynamic runner envelopes, order clip sizes, and sniper allocations.
        """
        tp = self.get_active_trading_power()

        btc_env = round(tp * self.split_btc_pct, 2)
        eth_env = round(tp * self.split_eth_pct, 2)

        # Clip per rung: envelope / rungs_per_side (floored at min_order_clip_gbp)
        btc_clip = max(self.min_order_clip_gbp, round(btc_env / self.rungs_per_side, 2))
        eth_clip = max(self.min_order_clip_gbp, round(eth_env / self.rungs_per_side, 2))

        # Sniper clip: scaled with trading power (min clip up to £100 max)
        sniper_clip = max(self.min_order_clip_gbp, min(100.0, round(tp * 0.50, 2)))

        return {
            "trading_power_gbp": tp,
            "expansion_ratio": round(tp / max(0.01, self.starting_balance_gbp), 2),
            "runner_btc": {
                "envelope_gbp": btc_env,
                "split_pct": self.split_btc_pct,
                "rungs_per_side": self.rungs_per_side,
                "order_size_gbp": btc_clip,
            },
            "runner_eth": {
                "envelope_gbp": eth_env,
                "split_pct": self.split_eth_pct,
                "rungs_per_side": self.rungs_per_side,
                "order_size_gbp": eth_clip,
            },
            "sniper": {
                "order_size_gbp": sniper_clip,
            },
        }

    def update_config(
        self,
        profit_lock_pct: Optional[float] = None,
        split_btc_pct: Optional[float] = None,
        split_eth_pct: Optional[float] = None,
        starting_balance_gbp: Optional[float] = None,
        rungs_per_side: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Updates live capital and profit management parameters.
        """
        if profit_lock_pct is not None:
            self.profit_lock_pct = max(0.0, min(0.90, float(profit_lock_pct)))
            # Recalculate locked profit with new ratio (ratchet ensures never decreases below existing lock)
            target = round(self.cumulative_profit_gbp * self.profit_lock_pct, 2)
            if target > self.locked_profit_gbp:
                self.locked_profit_gbp = target

        if split_btc_pct is not None:
            self.split_btc_pct = max(0.10, min(0.90, float(split_btc_pct)))
            self.split_eth_pct = round(1.0 - self.split_btc_pct, 2)

        if split_eth_pct is not None and split_btc_pct is None:
            self.split_eth_pct = max(0.10, min(0.90, float(split_eth_pct)))
            self.split_btc_pct = round(1.0 - self.split_eth_pct, 2)

        if starting_balance_gbp is not None and starting_balance_gbp > 0:
            self.starting_balance_gbp = round(float(starting_balance_gbp), 2)
            if self.balance_source == "PAPER_WALLET":
                self.settled_cash_gbp = self.starting_balance_gbp

        if rungs_per_side is not None and rungs_per_side >= 1:
            self.rungs_per_side = int(rungs_per_side)

        self._save_state()
        return self.get_telemetry_dto()

    def get_telemetry_dto(self) -> Dict[str, Any]:
        """
        Serializes capital management metrics for UI streaming.
        """
        allocations = self.get_allocations()
        tp = allocations["trading_power_gbp"]
        unlocked_profit = max(0.0, round(self.cumulative_profit_gbp - self.locked_profit_gbp, 2))

        return {
            "balance_source": self.balance_source,
            "starting_balance_gbp": self.starting_balance_gbp,
            "settled_cash_gbp": self.settled_cash_gbp,
            "cumulative_profit_gbp": self.cumulative_profit_gbp,
            "profit_lock_pct": self.profit_lock_pct,
            "locked_profit_gbp": self.locked_profit_gbp,
            "unlocked_profit_gbp": unlocked_profit,
            "active_trading_power_gbp": tp,
            "expansion_ratio": allocations["expansion_ratio"],
            "rungs_per_side": self.rungs_per_side,
            "split_btc_pct": self.split_btc_pct,
            "split_eth_pct": self.split_eth_pct,
            "allocations": allocations,
        }

capital_manager = CapitalManager()

