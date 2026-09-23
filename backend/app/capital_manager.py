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
    def __init__(self, starting_balance_gbp: float = 25.00):
        self.balance_source: str = "PAPER_WALLET"
        self.starting_balance_gbp: float = starting_balance_gbp
        self.total_deposited_cash_gbp: float = starting_balance_gbp
        self.settled_cash_gbp: float = starting_balance_gbp
        self.crypto_balances: Dict[str, float] = {
            "BTC": 0.0,
            "ETH": 0.0,
            "SOL": 0.0,
            "XRP": 0.0,
        }

        # Dynamic Profit Lock Configuration
        # Default: 0% profit lock (100% full compounding reinvestment mode)
        self.profit_lock_pct: float = 0.00
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

        # Auto Smart Detection of Transfers (In & Out of Account)
        self.deposits: list = []
        self.withdrawals: list = []
        self.total_deposits_gbp: float = 0.0
        self.total_withdrawals_gbp: float = 0.0
        self.net_deposited_cash_gbp: float = 0.0
        self.last_scanned_tx_timestamp: int = 0
        self.last_audit_timestamp: int = 0
        self.last_audit_status: str = "PENDING"
        self._audit_lock: Optional[Any] = None

        self._load_state()

    def _load_state(self):
        try:
            if STATE_FILE.exists():
                with open(STATE_FILE, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    if "total_deposited_cash_gbp" in data:
                        self.total_deposited_cash_gbp = float(data["total_deposited_cash_gbp"])
                        self.starting_balance_gbp = self.total_deposited_cash_gbp
                    elif "starting_balance_gbp" in data:
                        self.starting_balance_gbp = float(data["starting_balance_gbp"])
                        self.total_deposited_cash_gbp = self.starting_balance_gbp
                    if "net_deposited_cash_gbp" in data:
                        self.net_deposited_cash_gbp = float(data["net_deposited_cash_gbp"])
                        self.total_deposited_cash_gbp = self.net_deposited_cash_gbp
                        self.starting_balance_gbp = self.net_deposited_cash_gbp
                    if "total_deposits_gbp" in data:
                        self.total_deposits_gbp = float(data["total_deposits_gbp"])
                    if "total_withdrawals_gbp" in data:
                        self.total_withdrawals_gbp = float(data["total_withdrawals_gbp"])
                    if "deposits" in data and isinstance(data["deposits"], list):
                        self.deposits = data["deposits"]
                    if "withdrawals" in data and isinstance(data["withdrawals"], list):
                        self.withdrawals = data["withdrawals"]
                    if "last_scanned_tx_timestamp" in data:
                        self.last_scanned_tx_timestamp = int(data["last_scanned_tx_timestamp"])
                    if "last_audit_timestamp" in data:
                        self.last_audit_timestamp = int(data["last_audit_timestamp"])
                    if "last_audit_status" in data:
                        self.last_audit_status = str(data["last_audit_status"])
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
                logger.info(
                    f"Loaded capital state from {STATE_FILE} "
                    f"(cost basis: £{self.total_deposited_cash_gbp:.2f}, "
                    f"{len(self.deposits)} deposits, {len(self.withdrawals)} withdrawals)"
                )
        except Exception as e:
            logger.warning(f"Failed to load capital state from {STATE_FILE}: {e}")

    def _save_state(self):
        try:
            data = {
                "total_deposited_cash_gbp": self.total_deposited_cash_gbp,
                "starting_balance_gbp": self.starting_balance_gbp,
                "net_deposited_cash_gbp": self.net_deposited_cash_gbp,
                "total_deposits_gbp": self.total_deposits_gbp,
                "total_withdrawals_gbp": self.total_withdrawals_gbp,
                "deposits": self.deposits,
                "withdrawals": self.withdrawals,
                "last_scanned_tx_timestamp": self.last_scanned_tx_timestamp,
                "last_audit_timestamp": self.last_audit_timestamp,
                "last_audit_status": self.last_audit_status,
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
            self.crypto_balances["XRP"] = live_res.get("XRP", self.crypto_balances.get("XRP", 0.0))
            logger.info(f"Auto-detected Revolut X live balances: GBP £{self.settled_cash_gbp:,.2f}")
            return {
                "status": "success",
                "source": "REVOLUT_LIVE",
                "balances": {
                    "GBP": self.settled_cash_gbp,
                    "BTC": self.crypto_balances["BTC"],
                    "ETH": self.crypto_balances["ETH"],
                    "SOL": self.crypto_balances["SOL"],
                    "XRP": self.crypto_balances["XRP"],
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
                "XRP": self.crypto_balances["XRP"],
            },
        }

    def _sign_request(self, method: str, path: str) -> Optional[Dict[str, str]]:
        """
        Signs an HTTP request for Revolut X using Ed25519 authentication.
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
            clean_path = path.split("?")[0]
            query = path.split("?")[1] if "?" in path else ""
            payload_str = f"{timestamp}{method.upper()}{clean_path}{query}"
            signature_bytes = private_key.sign(payload_str.encode("utf-8"))
            signature_b64 = base64.b64encode(signature_bytes).decode("utf-8")

            return {
                "X-Revx-API-Key": api_key,
                "X-Revx-Timestamp": timestamp,
                "X-Revx-Signature": signature_b64,
                "Accept": "application/json",
            }
        except Exception as e:
            logger.error(f"Error signing Revolut X request: {e}")
            return None

    async def query_revolut_balances(self) -> Optional[Dict[str, float]]:
        """
        Queries Revolut X GET /api/1.0/balances using Ed25519 authentication.
        """
        headers = self._sign_request("GET", "/api/1.0/balances")
        if not headers:
            return None

        try:
            url = f"{self.revolut_base_url}/api/1.0/balances"
            async with httpx.AsyncClient(timeout=6.0) as client:
                resp = await client.get(url, headers=headers)
                if resp.status_code == 200:
                    data = resp.json()
                    balances: Dict[str, float] = {}
                    for item in data:
                        curr = item.get("currency")
                        avail = float(item.get("available", 0.0))
                        resvd = float(item.get("reserved", 0.0))
                        tot = float(item.get("total", avail + resvd))
                        if curr:
                            balances[curr] = tot
                    return balances
                else:
                    logger.warning(f"Revolut X balances query returned HTTP {resp.status_code}: {resp.text}")
                    return None
        except Exception as e:
            logger.error(f"Error querying Revolut X balances: {e}")
            return None

    async def _get_conversion_rates(self, client: httpx.AsyncClient) -> Dict[str, float]:
        """
        Dynamically fetches live ticker quotes from Revolut X to convert non-GBP deposits/withdrawals.
        Returns exchange rate or price in GBP for currencies (USD, EUR, SOL, BTC, ETH, etc.).
        """
        rates: Dict[str, float] = {
            "GBP": 1.0,
            "USD": 0.7513, # Fallback baseline FX
            "EUR": 0.8500, # Fallback baseline FX
            "SOL": 85.0,
            "XRP": 1.18,
            "BTC": 60000.0,
            "ETH": 2000.0,
        }
        try:
            headers = self._sign_request("GET", "/api/1.0/tickers")
            if headers:
                resp = await client.get(f"{self.revolut_base_url}/api/1.0/tickers", headers=headers, timeout=5.0)
                if resp.status_code == 200:
                    data = resp.json()
                    tickers_list = data.get("data", []) if isinstance(data, dict) else (data if isinstance(data, list) else [])
                    tickers: Dict[str, float] = {}
                    for t in tickers_list:
                        sym = t.get("symbol", "")
                        price = float(t.get("mid", t.get("last_price", 0.0)) or 0.0)
                        if sym and price > 0:
                            tickers[sym] = price

                    if "XRP/GBP" in tickers:
                        rates["XRP"] = tickers["XRP/GBP"]
                    if "SOL/GBP" in tickers:
                        rates["SOL"] = tickers["SOL/GBP"]
                    if "BTC/GBP" in tickers:
                        rates["BTC"] = tickers["BTC/GBP"]
                    if "ETH/GBP" in tickers:
                        rates["ETH"] = tickers["ETH/GBP"]

                    # Compute live USD/GBP cross-rate from crypto pairs
                    if "XRP/GBP" in tickers and "XRP/USD" in tickers and tickers["XRP/USD"] > 0:
                        rates["USD"] = round(tickers["XRP/GBP"] / tickers["XRP/USD"], 4)
                    elif "SOL/GBP" in tickers and "SOL/USD" in tickers and tickers["SOL/USD"] > 0:
                        rates["USD"] = round(tickers["SOL/GBP"] / tickers["SOL/USD"], 4)
                    elif "BTC/GBP" in tickers and "BTC/USD" in tickers and tickers["BTC/USD"] > 0:
                        rates["USD"] = round(tickers["BTC/GBP"] / tickers["BTC/USD"], 4)
                    elif "ETH/GBP" in tickers and "ETH/USD" in tickers and tickers["ETH/USD"] > 0:
                        rates["USD"] = round(tickers["ETH/GBP"] / tickers["ETH/USD"], 4)

                    # Compute live EUR/GBP cross-rate if EUR pairs present
                    if "EUR/GBP" in tickers:
                        rates["EUR"] = tickers["EUR/GBP"]
                    elif "SOL/GBP" in tickers and "SOL/EUR" in tickers and tickers["SOL/EUR"] > 0:
                        rates["EUR"] = round(tickers["SOL/GBP"] / tickers["SOL/EUR"], 4)

        except Exception as e:
            logger.warning(f"Failed to fetch live conversion rates for audit, using fallbacks: {e}")

        return rates

    def _convert_to_gbp(self, amount: float, currency: str, rates: Dict[str, float]) -> float:
        """
        Converts an incoming or outgoing transfer amount to GBP using dynamic market rates.
        """
        curr = currency.upper()
        if curr == "GBP":
            return amount
        rate = rates.get(curr, 1.0)
        return round(amount * rate, 2)

    async def audit_account_transfers(self, full_scan: bool = False) -> Dict[str, Any]:
        """
        Auto smart detection of account deposits ('receive') and withdrawals ('send').
        Maintains an audited ledger of capital transfers and synchronizes cost basis.
        """
        import asyncio
        if self._audit_lock is None:
            self._audit_lock = asyncio.Lock()

        async with self._audit_lock:
            known_deposit_ids = {d["id"] for d in self.deposits if "id" in d}
            known_withdrawal_ids = {w["id"] for w in self.withdrawals if "id" in w}

            new_deposits_found = 0
            new_withdrawals_found = 0
            cursor = None
            pages_read = 0

            # If full_scan or we have never scanned, scan from genesis back to the beginning of time
            stop_timestamp = 0 if (full_scan or self.last_scanned_tx_timestamp == 0) else self.last_scanned_tx_timestamp
            newest_tx_timestamp_seen = self.last_scanned_tx_timestamp

            try:
                async with httpx.AsyncClient(timeout=10.0) as client:
                    rates = await self._get_conversion_rates(client)

                    while True:
                        path = "/api/1.0/transactions?limit=100"
                        if cursor:
                            path += f"&cursor={cursor}"

                        headers = self._sign_request("GET", path)
                        if not headers:
                            self.last_audit_status = "AUTH_UNAVAILABLE"
                            break

                        resp = await client.get(f"{self.revolut_base_url}{path}", headers=headers)
                        if resp.status_code != 200:
                            logger.warning(f"Failed to query Revolut X transactions (HTTP {resp.status_code}): {resp.text}")
                            self.last_audit_status = f"HTTP_{resp.status_code}"
                            break

                        data = resp.json()
                        items = data.get("data", [])
                        pages_read += 1

                        reached_known_history = False

                        for tx in items:
                            tx_id = tx.get("id")
                            tx_type = tx.get("type")
                            status = tx.get("status")
                            created_date = tx.get("created_date", 0)

                            if created_date > newest_tx_timestamp_seen:
                                newest_tx_timestamp_seen = created_date

                            # If this is an incremental scan, stop once we hit transactions created before or at our last checkpoint
                            if stop_timestamp > 0 and created_date <= stop_timestamp:
                                reached_known_history = True
                                break

                            if status != "completed" or not tx_id:
                                continue

                            if tx_type == "receive":
                                if tx_id in known_deposit_ids:
                                    continue
                                dest = tx.get("destination", {})
                                curr = dest.get("currency", "GBP").upper()
                                try:
                                    amount = float(dest.get("amount", 0.0))
                                except (ValueError, TypeError):
                                    amount = 0.0

                                amount_gbp = self._convert_to_gbp(amount, curr, rates)

                                record = {
                                    "id": tx_id,
                                    "type": "receive",
                                    "status": status,
                                    "currency": curr,
                                    "amount": amount,
                                    "amount_gbp": amount_gbp,
                                    "created_date": created_date,
                                    "processed_date": tx.get("processed_date", 0),
                                }
                                self.deposits.append(record)
                                known_deposit_ids.add(tx_id)
                                new_deposits_found += 1
                                logger.info(
                                    f"[CAPITAL-AUDIT] Auto-detected deposit: +{amount} {curr} "
                                    f"(£{amount_gbp:.2f} GBP) | TX: {tx_id}"
                                )

                            elif tx_type == "send":
                                if tx_id in known_withdrawal_ids:
                                    continue
                                src = tx.get("source", {})
                                curr = src.get("currency", "GBP").upper()
                                try:
                                    amount = float(src.get("amount", 0.0))
                                except (ValueError, TypeError):
                                    amount = 0.0

                                amount_gbp = self._convert_to_gbp(amount, curr, rates)

                                record = {
                                    "id": tx_id,
                                    "type": "send",
                                    "status": status,
                                    "currency": curr,
                                    "amount": amount,
                                    "amount_gbp": amount_gbp,
                                    "created_date": created_date,
                                    "processed_date": tx.get("processed_date", 0),
                                }
                                self.withdrawals.append(record)
                                known_withdrawal_ids.add(tx_id)
                                new_withdrawals_found += 1
                                logger.info(
                                    f"[CAPITAL-AUDIT] Auto-detected withdrawal: -{amount} {curr} "
                                    f"(£{amount_gbp:.2f} GBP) | TX: {tx_id}"
                                )

                        if reached_known_history:
                            break

                        next_cursor = data.get("metadata", {}).get("next_cursor")
                        if not next_cursor or not items:
                            break

                        cursor = next_cursor
                        await asyncio.sleep(0.05)

                # Update timestamp checkpoint
                self.last_scanned_tx_timestamp = newest_tx_timestamp_seen

                # Recalculate net capital cost basis strictly from audited deposits and withdrawals
                self.total_deposits_gbp = round(sum(d.get("amount_gbp", 0.0) for d in self.deposits), 2)
                self.total_withdrawals_gbp = round(sum(w.get("amount_gbp", 0.0) for w in self.withdrawals), 2)
                self.net_deposited_cash_gbp = max(0.0, round(self.total_deposits_gbp - self.total_withdrawals_gbp, 2))

                # Update live working capital cost basis
                self.total_deposited_cash_gbp = self.net_deposited_cash_gbp
                self.starting_balance_gbp = self.net_deposited_cash_gbp

                self.last_audit_timestamp = int(time.time() * 1000)
                self.last_audit_status = "ACTIVE"
                self._save_state()

                if new_deposits_found > 0 or new_withdrawals_found > 0:
                    logger.info(
                        f"[CAPITAL-AUDIT-UPDATE] Discovered {new_deposits_found} new deposits, "
                        f"{new_withdrawals_found} new withdrawals across {pages_read} page(s). "
                        f"Updated Cost Basis: £{self.net_deposited_cash_gbp:.2f} GBP"
                    )

            except Exception as e:
                logger.error(f"[CAPITAL-AUDIT-ERROR] Error during transfer audit: {e}")
                self.last_audit_status = "ERROR"

            return {
                "status": self.last_audit_status,
                "deposits_count": len(self.deposits),
                "withdrawals_count": len(self.withdrawals),
                "new_deposits_found": new_deposits_found,
                "new_withdrawals_found": new_withdrawals_found,
                "total_deposits_gbp": self.total_deposits_gbp,
                "total_withdrawals_gbp": self.total_withdrawals_gbp,
                "net_deposited_cash_gbp": self.net_deposited_cash_gbp,
                "last_audit_timestamp": self.last_audit_timestamp,
            }

    def record_trade_profit(self, profit_gbp: float):
        """
        Records realized net profit and updates the high-water mark locked profit reserve vault.
        """
        if profit_gbp <= 0:
            return

        self.cumulative_profit_gbp = round(self.cumulative_profit_gbp + profit_gbp, 2)

        # High-water mark ratchet: profits once locked into the reserve vault can never decrease
        if self.profit_lock_pct <= 0.0:
            self.locked_profit_gbp = 0.0
        else:
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
            if self.profit_lock_pct <= 0.0:
                self.locked_profit_gbp = 0.0
            else:
                target = round(self.cumulative_profit_gbp * self.profit_lock_pct, 2)
                self.locked_profit_gbp = target

        if split_btc_pct is not None:
            self.split_btc_pct = max(0.10, min(0.90, float(split_btc_pct)))
            self.split_eth_pct = round(1.0 - self.split_btc_pct, 2)

        if split_eth_pct is not None and split_btc_pct is None:
            self.split_eth_pct = max(0.10, min(0.90, float(split_eth_pct)))
            self.split_btc_pct = round(1.0 - self.split_eth_pct, 2)

        if starting_balance_gbp is not None and starting_balance_gbp > 0:
            self.starting_balance_gbp = round(float(starting_balance_gbp), 2)
            self.total_deposited_cash_gbp = self.starting_balance_gbp
            if self.balance_source == "PAPER_WALLET":
                self.settled_cash_gbp = self.starting_balance_gbp

        if rungs_per_side is not None and rungs_per_side >= 1:
            self.rungs_per_side = int(rungs_per_side)

        self._save_state()
        return self.get_telemetry_dto()

    def update_deposited_cash(self, amount: float) -> Dict[str, Any]:
        """
        Updates the explicit total deposited cash (cost basis) from the user/UI.
        """
        if amount > 0:
            self.total_deposited_cash_gbp = round(float(amount), 2)
            self.starting_balance_gbp = self.total_deposited_cash_gbp
            if self.balance_source == "PAPER_WALLET":
                self.settled_cash_gbp = self.total_deposited_cash_gbp
            self._save_state()
            logger.info(f"Updated total deposited cash basis to £{self.total_deposited_cash_gbp:.2f}")
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
            "total_deposited_cash_gbp": self.total_deposited_cash_gbp,
            "net_deposited_cash_gbp": self.net_deposited_cash_gbp,
            "total_deposits_gbp": self.total_deposits_gbp,
            "total_withdrawals_gbp": self.total_withdrawals_gbp,
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
            "transfer_audit": {
                "status": self.last_audit_status,
                "deposits_count": len(self.deposits),
                "withdrawals_count": len(self.withdrawals),
                "total_deposits_gbp": self.total_deposits_gbp,
                "total_withdrawals_gbp": self.total_withdrawals_gbp,
                "net_deposited_cash_gbp": self.net_deposited_cash_gbp,
                "last_audit_timestamp": self.last_audit_timestamp,
                "recent_transfers": sorted(
                    self.deposits + self.withdrawals,
                    key=lambda x: x.get("created_date", 0),
                    reverse=True,
                )[:10],
            },
        }

capital_manager = CapitalManager()

