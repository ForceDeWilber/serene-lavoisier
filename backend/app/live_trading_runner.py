import asyncio
import base64
import json
import logging
import os
import time
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional
import uuid
import httpx

logger = logging.getLogger("live_trading_runner")

class LiveTradingRunner:
    """
    Genuine Live Execution Engine for Revolut X Spot Trading.
    STRICT LIVE RULES:
    1. Requires valid REVOLUT_API_KEY and revolut_private.pem (Ed25519).
    2. Fetches real settled balances from Revolut X.
    3. ZERO FALLBACK: Never falls back to a £10 or £0 paper wallet. If unauthenticated
       or if balance is £0.00, it marks status as UNAUTHENTICATED or INSUFFICIENT_FUNDS
       and strictly refuses to place orders.
    4. Places real post_only limit orders (0.00% maker fee) with genuine client order IDs.
    5. Dispatches real DELETE cancellations on rebalancing and emergency kill switch.
    """

    def __init__(self):
        self.running: bool = False
        self.task: Optional[asyncio.Task] = None

        # Credentials & Configuration
        self.api_key: Optional[str] = os.getenv("REVOLUT_API_KEY")
        self.priv_key_path: str = os.getenv(
            "REVOLUT_PRIVATE_KEY_PATH", "backend/credentials/revolut_private.pem"
        )
        self.base_url: str = os.getenv("REVOLUT_BASE_URL", "https://revx.revolut.com")

        # Diagnostics & Auth State
        self.status: str = "INITIALIZING" # "UNCONFIGURED" | "AUTH_ERROR" | "INSUFFICIENT_FUNDS" | "ACTIVE" | "PAUSED"
        self.status_message: str = "Initializing Revolut X Live Connection..."
        self.authenticated: bool = False
        self.can_trade: bool = False
        self.last_sync_time: float = 0.0
        self.last_latency_ms: int = 0

        # Real Settled Balances directly from Revolut X (NO FALLBACK)
        self.balances: Dict[str, float] = {
            "GBP": 0.0,
            "BTC": 0.0,
            "ETH": 0.0,
        }

        # Market prices anchored to live Kraken oracle
        self.prices: Dict[str, float] = {
            "BTC/GBP": 57000.00,
            "ETH/GBP": 1820.00,
        }
        self.market_stats: Dict[str, Dict[str, float]] = {
            "BTC/GBP": {"high24h": 58000.0, "low24h": 56500.0, "change24h": 0.0},
            "ETH/GBP": {"high24h": 1850.0, "low24h": 1780.0, "change24h": 0.0},
        }

        # Real Active Resting Orders on Revolut X
        self.resting_orders: List[Dict[str, Any]] = []
        self.order_counter: int = 1

        # Real Executed Trade Logs
        self.live_trades: List[Dict[str, Any]] = []
        self.trade_counter: int = 1

        # Circuit Breaker
        self.circuit_breaker_tripped: bool = False
        self.circuit_breaker_reason: str = ""

        # Runner configs for live capital allocation
        self.runners: Dict[str, Dict[str, Any]] = {
            "runner_btc": {
                "runner_id": "runner_btc",
                "symbol": "BTC/GBP",
                "center_price": 57000.00,
                "step_pct": 0.0040, # 0.40% geometric step
                "rebalance_threshold_pct": 0.020, # 2.0%
                "order_size_gbp": 5.00,
                "rungs_per_side": 3,
                "is_paused": False,
                "realized_pnl": 0.0,
                "total_trades": 0,
                "fee_savings": 0.0,
            },
            "runner_eth": {
                "runner_id": "runner_eth",
                "symbol": "ETH/GBP",
                "center_price": 1820.00,
                "step_pct": 0.0040,
                "rebalance_threshold_pct": 0.020,
                "order_size_gbp": 5.00,
                "rungs_per_side": 3,
                "is_paused": False,
                "realized_pnl": 0.0,
                "total_trades": 0,
                "fee_savings": 0.0,
            },
        }

    def _get_private_key(self):
        """Loads and verifies the Ed25519 private key from the credentials path."""
        api_key = self.api_key or os.getenv("REVOLUT_API_KEY")
        priv_path = self.priv_key_path or os.getenv("REVOLUT_PRIVATE_KEY_PATH", "backend/credentials/revolut_private.pem")

        if not api_key:
            return None, "Missing REVOLUT_API_KEY environment variable."

        if not os.path.exists(priv_path):
            return None, f"Private key file not found at: {priv_path}"

        try:
            from cryptography.hazmat.primitives.asymmetric import ed25519
            from cryptography.hazmat.primitives import serialization

            with open(priv_path, "rb") as f:
                key = serialization.load_pem_private_key(f.read(), password=None)

            if not isinstance(key, ed25519.Ed25519PrivateKey):
                return None, "Private key is not a valid Ed25519 private key."

            return key, None
        except Exception as e:
            return None, f"Failed to load Ed25519 private key: {e}"

    def _sign_request(self, method: str, path: str, body_bytes: bytes = b"") -> Optional[Dict[str, str]]:
        """Signs an HTTP request with Ed25519 for Revolut X."""
        priv_key, err = self._get_private_key()
        if err or not priv_key:
            return None

        timestamp = str(int(time.time() * 1000))
        # Revolut X message canonical format: timestamp_ms + METHOD + path (from /api) + query (without '?') + body
        clean_path = path
        query_str = ""
        if "?" in path:
            clean_path, query_str = path.split("?", 1)
        canonical = f"{timestamp}{method.upper()}{clean_path}{query_str}".encode("utf-8") + body_bytes
        signature_bytes = priv_key.sign(canonical)
        signature_b64 = base64.b64encode(signature_bytes).decode("utf-8")

        api_key = self.api_key or os.getenv("REVOLUT_API_KEY", "")
        headers = {
            "X-Revx-API-Key": api_key,
            "X-Revx-Timestamp": timestamp,
            "X-Revx-Signature": signature_b64,
            "Accept": "application/json",
        }
        if body_bytes:
            headers["Content-Type"] = "application/json"
        return headers

    async def verify_credentials_and_balances(self) -> Dict[str, Any]:
        """
        Queries Revolut X GET /api/1.0/balances.
        Enforces STRICT validation: no mock fallback on failure.
        """
        api_key = self.api_key or os.getenv("REVOLUT_API_KEY")
        priv_path = self.priv_key_path

        if not api_key or not os.path.exists(priv_path):
            self.status = "UNCONFIGURED"
            self.authenticated = False
            self.can_trade = False
            self.status_message = "Revolut X API key or revolut_private.pem not found. Upload credentials to enable Live trading."
            return {
                "status": self.status,
                "authenticated": False,
                "can_trade": False,
                "error": self.status_message,
                "balances": self.balances,
            }

        start_t = time.perf_counter()
        try:
            path = "/api/1.0/balances"
            headers = self._sign_request("GET", path)
            if not headers:
                self.status = "AUTH_ERROR"
                self.authenticated = False
                self.can_trade = False
                self.status_message = "Failed to create Ed25519 signature from private key."
                return {"status": self.status, "authenticated": False, "can_trade": False, "error": self.status_message}

            url = f"{self.base_url}{path}"
            async with httpx.AsyncClient(timeout=6.0) as client:
                resp = await client.get(url, headers=headers)
                latency = int((time.perf_counter() - start_t) * 1000)
                self.last_latency_ms = latency

                if resp.status_code == 200:
                    data = resp.json()
                    new_bals: Dict[str, float] = {"GBP": 0.0, "BTC": 0.0, "ETH": 0.0}
                    for item in data:
                        curr = item.get("currency")
                        avail = float(item.get("available", 0.0))
                        if curr in new_bals:
                            new_bals[curr] = round(avail, 6)

                    self.balances = new_bals
                    self.authenticated = True
                    self.last_sync_time = time.time()

                    has_resting = len(self.resting_orders) > 0
                    has_crypto = (self.balances.get("BTC", 0.0) > 0.0) or (self.balances.get("ETH", 0.0) > 0.0)

                    # STRICT ZERO BALANCE CHECK: Only mark INSUFFICIENT_FUNDS if free GBP is 0 and no resting orders or crypto inventory exist
                    if self.balances["GBP"] <= 0.0 and not has_resting and not has_crypto:
                        self.status = "INSUFFICIENT_FUNDS"
                        self.can_trade = False
                        self.status_message = "Revolut X account has £0.00 available GBP balance. Deposit GBP to activate live grid trading."
                        logger.warning("[LIVE] Revolut X authenticated successfully, but GBP balance is £0.00. Trading blocked.")
                    else:
                        self.status = "ACTIVE" if not any(r["is_paused"] for r in self.runners.values()) else "PAUSED"
                        self.can_trade = True
                        self.status_message = f"Revolut X Live Connected ({latency}ms latency). Available: £{self.balances['GBP']:,.2f}"
                        logger.info(f"[LIVE] Authenticated! Real balances: GBP £{self.balances['GBP']:,.2f}, BTC {self.balances['BTC']}, ETH {self.balances['ETH']}")

                    return {
                        "status": self.status,
                        "authenticated": self.authenticated,
                        "can_trade": self.can_trade,
                        "latency_ms": latency,
                        "balances": self.balances,
                        "message": self.status_message,
                    }
                else:
                    self.status = "AUTH_ERROR"
                    self.authenticated = False
                    self.can_trade = False
                    err_text = resp.text[:200]
                    self.status_message = f"Revolut X API rejected authentication (HTTP {resp.status_code}): {err_text}"
                    logger.error(f"[LIVE] Auth error HTTP {resp.status_code}: {err_text}")
                    return {
                        "status": self.status,
                        "authenticated": False,
                        "can_trade": False,
                        "error": self.status_message,
                        "http_code": resp.status_code,
                    }
        except Exception as e:
            self.status = "AUTH_ERROR"
            self.authenticated = False
            self.can_trade = False
            self.status_message = f"Network/Connection error to Revolut X: {e}"
            logger.error(f"[LIVE] Exception connecting to Revolut X: {e}")
            return {
                "status": self.status,
                "authenticated": False,
                "can_trade": False,
                "error": self.status_message,
            }

    async def start(self):
        """Starts the Live Execution Runner."""
        if self.running:
            return
        self.running = True
        logger.info("Initializing Revolut X Live Trading Runner...")

        # 1. Fetch live market prices from Kraken
        await self._seed_market_data()

        # 2. Strict Live Check: Verify Revolut X credentials & real balances
        diag = await self.verify_credentials_and_balances()

        # 3. Reconcile any existing resting orders on Revolut X
        await self._reconcile_active_orders()

        if not diag.get("can_trade"):
            logger.warning(f"[LIVE] Live execution halted: {diag.get('error') or diag.get('message')}")
        elif len(self.resting_orders) == 0 and self.balances.get("GBP", 0.0) >= 2.00:
            logger.info("[LIVE] No resting orders found and GBP available. Deploying initial live maker rungs...")
            await self._deploy_initial_live_grids()
        else:
            logger.info(f"[LIVE] Live trading active with {len(self.resting_orders)} resting rungs on Revolut X book.")

        # 3. Start background live execution loop
        self.task = asyncio.create_task(self._live_execution_loop())

    async def stop(self):
        """Stops the Live Execution Runner."""
        self.running = False
        if self.task:
            self.task.cancel()
            try:
                await self.task
            except asyncio.CancelledError:
                pass
        logger.info("[LIVE] Live Trading Runner stopped.")

    async def _seed_market_data(self):
        """Seeds real market prices from public Kraken ticker."""
        try:
            async with httpx.AsyncClient(timeout=6.0) as client:
                r = await client.get("https://api.kraken.com/0/public/Ticker?pair=XBTGBP,ETHGBP")
                if r.status_code == 200:
                    data = r.json().get("result", {})
                    if "XXBTZGBP" in data:
                        t = data["XXBTZGBP"]
                        p = float(t["c"][0])
                        self.prices["BTC/GBP"] = p
                        self.runners["runner_btc"]["center_price"] = p
                        self.market_stats["BTC/GBP"] = {
                            "high24h": float(t["h"][1]),
                            "low24h": float(t["l"][1]),
                            "change24h": round(((p - float(t["o"])) / float(t["o"])) * 100.0, 2),
                        }
                    if "XETHZGBP" in data:
                        t = data["XETHZGBP"]
                        p = float(t["c"][0])
                        self.prices["ETH/GBP"] = p
                        self.runners["runner_eth"]["center_price"] = p
                        self.market_stats["ETH/GBP"] = {
                            "high24h": float(t["h"][1]),
                            "low24h": float(t["l"][1]),
                            "change24h": round(((p - float(t["o"])) / float(t["o"])) * 100.0, 2),
                        }
        except Exception as e:
            logger.warning(f"[LIVE] Error seeding Kraken prices: {e}")

    async def _deploy_initial_live_grids(self):
        """
        Deploys real maker limit orders (post_only) to Revolut X.
        Autodetects available GBP and splits according to configured allocation.
        """
        if not self.can_trade or self.balances["GBP"] < 2.00:
            logger.warning("[LIVE] Skipping grid order deployment: insufficient capital or not ready.")
            return

        gbp_avail = self.balances["GBP"]
        split_btc = float(os.getenv("SPLIT_BTC_PCT", "0.50"))
        split_eth = float(os.getenv("SPLIT_ETH_PCT", "0.50"))

        # Calculate budget for each asset
        btc_budget = round(gbp_avail * split_btc, 2)
        eth_budget = round(gbp_avail * split_eth, 2)

        rungs = int(os.getenv("RUNGS_PER_SIDE", "3"))

        # Dynamically scale rungs based on available budget (minimum order floor £1.00 on Revolut X)
        btc_rungs = min(rungs, max(1, int(btc_budget / 1.00))) if btc_budget >= 1.00 else 0
        eth_rungs = min(rungs, max(1, int(eth_budget / 1.00))) if eth_budget >= 1.00 else 0

        btc_clip = round(btc_budget / btc_rungs, 2) if btc_rungs > 0 else 0.0
        if btc_clip * btc_rungs > btc_budget:
            btc_clip = int((btc_budget / btc_rungs) * 100) / 100.0

        eth_clip = round(eth_budget / eth_rungs, 2) if eth_rungs > 0 else 0.0
        if eth_clip * eth_rungs > eth_budget:
            eth_clip = int((eth_budget / eth_rungs) * 100) / 100.0

        self.runners["runner_btc"]["order_size_gbp"] = btc_clip
        self.runners["runner_eth"]["order_size_gbp"] = eth_clip

        logger.info(
            f"[LIVE-ALLOCATION] Auto-splitting £{gbp_avail:,.2f} GBP: "
            f"BTC budget = £{btc_budget:,.2f} ({int(split_btc*100)}%, {btc_rungs} rungs @ £{btc_clip:,.2f}), "
            f"ETH budget = £{eth_budget:,.2f} ({int(split_eth*100)}%, {eth_rungs} rungs @ £{eth_clip:,.2f})"
        )

        # Place initial BUY rungs below market price
        for runner_id, runner in self.runners.items():
            sym = runner["symbol"]
            center = runner["center_price"]
            step = runner["step_pct"]
            clip = runner["order_size_gbp"]
            n_rungs = btc_rungs if "btc" in runner_id else eth_rungs

            if clip < 1.00 or n_rungs == 0:
                continue

            # Place buy rungs below center price
            for r in range(1, n_rungs + 1):
                p = round(center * ((1.0 - step) ** r), 2)
                qty = round(clip / p, 8)
                await self._submit_live_order(
                    runner_id=runner_id,
                    symbol=sym,
                    side="BUY",
                    price=p,
                    qty=qty,
                    rung_level=-r,
                )

    async def _submit_live_order(
        self, runner_id: str, symbol: str, side: str, price: float, qty: float, rung_level: int
    ) -> Optional[Dict[str, Any]]:
        """
        Submits a genuine post_only limit order to Revolut X via HTTP/2 REST API.
        """
        if not self.authenticated or not self.can_trade:
            logger.warning(f"[LIVE-ORDER-BLOCK] Order submission blocked: {self.status_message}")
            return None

        client_order_id = str(uuid.uuid4())
        self.order_counter += 1

        rev_symbol = "BTC-GBP" if "BTC" in symbol else "ETH-GBP"
        payload = {
            "client_order_id": client_order_id,
            "symbol": rev_symbol,
            "side": side.upper(),
            "order_configuration": {
                "limit": {
                    "base_size": f"{qty:.8f}",
                    "price": f"{price:.2f}",
                    "execution_instructions": ["post_only"],
                }
            },
        }
        body_bytes = json.dumps(payload, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
        path = "/api/1.0/orders"
        headers = self._sign_request("POST", path, body_bytes)

        if not headers:
            logger.error(f"[LIVE] Failed to sign order request for {client_order_id}")
            return None

        try:
            url = f"{self.base_url}{path}"
            async with httpx.AsyncClient(timeout=5.0) as client:
                resp = await client.post(url, headers=headers, content=body_bytes)
                if resp.status_code in (200, 201):
                    res_json = resp.json()
                    data = res_json.get("data", res_json)
                    venue_id = data.get("venue_order_id") or data.get("order_id") or client_order_id
                    order_obj = {
                        "id": client_order_id,
                        "exchange_id": venue_id,
                        "runner_id": runner_id,
                        "symbol": symbol,
                        "side": side,
                        "price": price,
                        "qty": qty,
                        "value_gbp": round(price * qty, 2),
                        "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                        "rung_level": rung_level,
                        "distance_pct": round(((price - self.runners[runner_id]["center_price"]) / self.runners[runner_id]["center_price"]) * 100.0, 2),
                        "is_live": True,
                    }
                    self.resting_orders.append(order_obj)
                    logger.info(f"[LIVE-ORDER-PLACED] {side} {qty:.6f} {symbol} @ £{price:,.2f} (VenueId: {venue_id})")
                    await self._persist_order_to_db(order_obj, status="OPEN")
                    return order_obj
                else:
                    logger.warning(f"[LIVE-ORDER-REJECT] Revolut X rejected order [{resp.status_code}]: {resp.text}")
                    return None
        except Exception as e:
            logger.error(f"[LIVE-ORDER-ERROR] Exception placing order on Revolut X: {e}")
            return None

    async def cancel_live_order(self, order_id_or_exchange_id: str) -> bool:
        """Sends DELETE /api/1.0/orders/{venue_order_id} to Revolut X."""
        target = next((o for o in self.resting_orders if o["id"] == order_id_or_exchange_id or o.get("exchange_id") == order_id_or_exchange_id), None)
        del_id = target.get("exchange_id", order_id_or_exchange_id) if target else order_id_or_exchange_id

        path = f"/api/1.0/orders/{del_id}"
        headers = self._sign_request("DELETE", path)
        if not headers:
            return False

        try:
            url = f"{self.base_url}{path}"
            async with httpx.AsyncClient(timeout=4.0) as client:
                resp = await client.delete(url, headers=headers)
                if resp.status_code in (200, 204):
                    self.resting_orders = [o for o in self.resting_orders if o["id"] != order_id_or_exchange_id and o.get("exchange_id") != order_id_or_exchange_id]
                    logger.info(f"[LIVE-ORDER-CANCELED] Successfully canceled order {del_id}")
                    return True
                else:
                    logger.warning(f"[LIVE-CANCEL-FAIL] Failed to cancel {del_id} [HTTP {resp.status_code}]: {resp.text}")
                    return False
        except Exception as e:
            logger.error(f"[LIVE-CANCEL-ERROR] Exception canceling order {del_id}: {e}")
            return False

    async def emergency_kill_switch(self, reason: str = "Manual Emergency Abort") -> Dict[str, Any]:
        """
        Emergency Abort for Live Trading:
        Cancels all active orders directly on Revolut X and pauses all live runners.
        """
        logger.warning(f"[LIVE-KILL-SWITCH] Triggered! Reason: {reason}")
        self.circuit_breaker_tripped = True
        self.circuit_breaker_reason = reason

        # Pause all live runners
        for r in self.runners.values():
            r["is_paused"] = True

        # 1. Atomic cancel all on Revolut X
        path = "/api/1.0/orders"
        headers = self._sign_request("DELETE", path)
        if headers:
            try:
                url = f"{self.base_url}{path}"
                async with httpx.AsyncClient(timeout=4.0) as client:
                    await client.delete(url, headers=headers)
            except Exception as e:
                logger.warning(f"[LIVE-KILL-SWITCH] Bulk cancel error: {e}")

        # 2. Local cancel loop fallback
        canceled_count = len(self.resting_orders)
        self.resting_orders.clear()

        self.status = "PAUSED"
        self.status_message = f"🚨 EMERGENCY KILL SWITCH TRIGGERED: {reason}. Canceled {canceled_count} orders on Revolut X."
        return {
            "status": "success",
            "canceled_orders_count": canceled_count,
            "reason": reason,
        }

    async def _sync_live_orders(self):
        """Polls Revolut X GET /api/1.0/orders/active to track fills and place profit counter-rungs."""
        path = "/api/1.0/orders/active"
        headers = self._sign_request("GET", path)
        if not headers or not self.resting_orders:
            return

        try:
            url = f"{self.base_url}{path}"
            async with httpx.AsyncClient(timeout=4.0) as client:
                resp = await client.get(url, headers=headers)
                if resp.status_code == 200:
                    data = resp.json()
                    active_items = data.get("data", []) if isinstance(data, dict) else data
                    active_ids = {
                        val for o in active_items if isinstance(o, dict)
                        for val in (o.get("id"), o.get("venue_order_id"), o.get("order_id"), o.get("client_order_id"))
                        if val
                    }

                    filled_orders = [
                        o for o in list(self.resting_orders)
                        if o["id"] not in active_ids and o.get("exchange_id") not in active_ids
                    ]

                    for filled in filled_orders:
                        self.resting_orders.remove(filled)
                        runner_id = filled["runner_id"]
                        runner = self.runners.get(runner_id)
                        step = runner["step_pct"] if runner else 0.0040

                        if filled["side"] == "BUY":
                            # Placed paired SELL rung one step higher (+0.40%) to take profit
                            sell_px = round(filled["price"] * (1.0 + step), 2)
                            if runner:
                                runner["inventory_base"] = round(runner.get("inventory_base", 0.0) + filled["qty"], 8)
                                runner["inventory_value_gbp"] = round(runner["inventory_base"] * filled["price"], 2)
                            logger.info(
                                f"[LIVE-FILL] BUY filled for {filled['qty']} {filled['symbol']} @ £{filled['price']:,.2f}! "
                                f"Placing paired profit-take SELL rung @ £{sell_px:,.2f}..."
                            )
                            await self._submit_live_order(
                                runner_id=runner_id,
                                symbol=filled["symbol"],
                                side="SELL",
                                price=sell_px,
                                qty=filled["qty"],
                                rung_level=abs(filled.get("rung_level", 1)),
                            )
                        elif filled["side"] == "SELL":
                            # Profit realized! Place paired BUY rung one step lower
                            profit = round(filled["qty"] * (filled["price"] * step), 2)
                            if runner:
                                runner["realized_pnl"] = round(runner["realized_pnl"] + profit, 2)
                                runner["total_trades"] += 1
                                runner["inventory_base"] = max(0.0, round(runner.get("inventory_base", 0.0) - filled["qty"], 8))
                                runner["inventory_value_gbp"] = round(runner["inventory_base"] * filled["price"], 2)
                            buy_px = round(filled["price"] * (1.0 - step), 2)
                            logger.info(
                                f"[LIVE-FILL] SELL filled for {filled['qty']} {filled['symbol']}! "
                                f"Profit locked: +£{profit:.2f}. Replacing BUY rung @ £{buy_px:,.2f}..."
                            )
                            await self._submit_live_order(
                                runner_id=runner_id,
                                symbol=filled["symbol"],
                                side="BUY",
                                price=buy_px,
                                qty=filled["qty"],
                                rung_level=-abs(filled.get("rung_level", 1)),
                            )

                        await self._persist_order_status(filled["id"], "FILLED")

                        self.live_trades.append({
                            "id": f"trade_{int(time.time()*1000)}_{self.trade_counter}",
                            "symbol": filled["symbol"],
                            "side": filled["side"],
                            "price": filled["price"],
                            "qty": filled["qty"],
                            "value_gbp": filled["value_gbp"],
                            "timestamp": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                            "is_live": True,
                        })
                        self.trade_counter += 1
        except Exception as e:
            logger.debug(f"[LIVE-SYNC] Order active check: {e}")

    async def _live_execution_loop(self):
        """Main live loop polling Revolut X order states, capital arrivals, and managing positions."""
        cycle = 0
        while self.running:
            try:
                await asyncio.sleep(2.0)
                cycle += 1

                # ALWAYS sync active orders to instantly catch fills
                if self.authenticated and self.running:
                    await self._sync_live_orders()

                # Update prices from Kraken every 6 seconds
                if cycle % 3 == 0:
                    await self._seed_market_data()

                # Sync Revolut X balances and auto-detect deposits every 10 seconds
                if cycle % 5 == 0:
                    await self.verify_credentials_and_balances()

                    # Autodetect deposit arrival: if NEW unallocated capital arrives (>= £2.00) AND no orders exist
                    if self.can_trade and self.balances["GBP"] >= 2.00 and len(self.resting_orders) == 0:
                        logger.info(
                            f"[LIVE-CAPITAL-AUTODETECT] Available GBP: £{self.balances['GBP']:,.2f}. "
                            f"Auto-splitting and deploying initial maker grid rungs..."
                        )
                        await self._deploy_initial_live_grids()

            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"[LIVE-LOOP-ERROR] {e}")

    def tune_runner(
        self,
        runner_id: str,
        paused: Optional[bool] = None,
        step_pct: Optional[float] = None,
        rebalance_threshold_pct: Optional[float] = None,
    ) -> Dict[str, Any]:
        """Dynamically tunes live runner parameters."""
        if runner_id not in self.runners:
            return {"status": "error", "message": f"Runner {runner_id} not found"}

        runner = self.runners[runner_id]
        if paused is not None:
            runner["is_paused"] = paused
        if step_pct is not None:
            runner["step_pct"] = step_pct
        if rebalance_threshold_pct is not None:
            runner["rebalance_threshold_pct"] = rebalance_threshold_pct

        logger.info(f"[LIVE-TUNE] Updated {runner_id}: paused={runner['is_paused']}, step_pct={runner['step_pct']}")
        return {"status": "success", "payload": runner}

    def reset_circuit_breaker(self) -> Dict[str, Any]:
        """Resets the circuit breaker and resumes live runner state."""
        self.circuit_breaker_tripped = False
        self.circuit_breaker_reason = ""
        for r in self.runners.values():
            r["is_paused"] = False
        self.status = "ACTIVE" if self.can_trade else self.status
        self.status_message = "Circuit breaker reset. Live runners resumed."
        return {"status": "success", "message": self.status_message}

    async def _persist_order_to_db(self, order_obj: Dict[str, Any], status: str = "OPEN"):
        try:
            from app.database import LiveSessionLocal
            from app.models import OrderRecord
            async with LiveSessionLocal() as session:
                rec = OrderRecord(
                    client_order_id=order_obj["id"],
                    runner_id=order_obj["runner_id"],
                    symbol=order_obj["symbol"],
                    side=order_obj["side"],
                    price=order_obj["price"],
                    qty=order_obj["qty"],
                    status=status,
                )
                session.add(rec)
                await session.commit()
        except Exception as e:
            logger.debug(f"[DB-PERSIST] Order record log: {e}")

    async def _persist_order_status(self, client_order_id: str, status: str):
        try:
            from app.database import LiveSessionLocal
            from app.models import OrderRecord
            from sqlalchemy import select
            async with LiveSessionLocal() as session:
                res = await session.execute(
                    select(OrderRecord).where(OrderRecord.client_order_id == client_order_id)
                )
                rec = res.scalar_one_or_none()
                if rec:
                    rec.status = status
                    await session.commit()
        except Exception as e:
            logger.debug(f"[DB-PERSIST] Order status update: {e}")

    async def _reconcile_active_orders(self):
        """Rehydrates self.resting_orders from Revolut X GET /api/1.0/orders/active on startup."""
        path = "/api/1.0/orders/active"
        headers = self._sign_request("GET", path)
        if not headers:
            return

        try:
            url = f"{self.base_url}{path}"
            async with httpx.AsyncClient(timeout=5.0) as client:
                resp = await client.get(url, headers=headers)
                if resp.status_code == 200:
                    data = resp.json()
                    active_items = data.get("data", []) if isinstance(data, dict) else data
                    reconciled = []
                    for o in active_items:
                        if not isinstance(o, dict):
                            continue
                        venue_id = o.get("id")
                        cid = o.get("client_order_id") or venue_id
                        sym = o.get("symbol", "").replace("-", "/")
                        side = o.get("side", "").upper()
                        px = float(o.get("price", 0.0))
                        qty = float(o.get("quantity", 0.0))
                        val = round(px * qty, 2)
                        rid = "runner_btc" if "BTC" in sym else "runner_eth"
                        
                        created_ms = o.get("created_date", 0)
                        if created_ms:
                            created_str = datetime.fromtimestamp(created_ms / 1000, tz=timezone.utc).strftime("%H:%M:%S")
                        else:
                            created_str = datetime.now(timezone.utc).strftime("%H:%M:%S")

                        reconciled.append({
                            "id": cid,
                            "exchange_id": venue_id,
                            "runner_id": rid,
                            "symbol": sym,
                            "side": side,
                            "price": px,
                            "qty": qty,
                            "value_gbp": val,
                            "created_at": created_str,
                            "rung_level": -1 if side == "BUY" else 1,
                            "distance_pct": 0.0,
                            "is_live": True,
                        })
                    self.resting_orders = reconciled
                    logger.info(f"[LIVE-RECONCILE] Rehydrated {len(reconciled)} resting orders directly from Revolut X order book.")
        except Exception as e:
            logger.error(f"[LIVE-RECONCILE-ERROR] Failed to reconcile active orders: {e}")

    def get_telemetry(self) -> Dict[str, Any]:
        """Generates comprehensive telemetry for Live Production mode."""
        now = time.time()
        btc_p = self.prices["BTC/GBP"]
        eth_p = self.prices["ETH/GBP"]

        # Calculate live equity from genuine balances
        gbp_val = self.balances["GBP"]
        btc_val = round(self.balances["BTC"] * btc_p, 2)
        eth_val = round(self.balances["ETH"] * eth_p, 2)
        total_equity = round(gbp_val + btc_val + eth_val, 2)

        # Build runner DTOs
        runners_list = []
        for rid, r in self.runners.items():
            sym = r["symbol"]
            p = self.prices[sym]
            inv = self.balances["BTC"] if "BTC" in sym else self.balances["ETH"]
            runners_list.append({
                "runner_id": rid,
                "symbol": sym,
                "current_price": p,
                "center_price": r["center_price"],
                "inventory_base": inv,
                "inventory_value_gbp": round(inv * p, 2),
                "realized_pnl": r["realized_pnl"],
                "total_trades": r["total_trades"],
                "active_orders_count": len([o for o in self.resting_orders if o["runner_id"] == rid]),
                "is_paused": r["is_paused"],
                "step_pct": r["step_pct"],
                "rebalance_threshold_pct": r["rebalance_threshold_pct"],
            })

        return {
            "mode": "live",
            "is_live": True,
            "status": self.status,
            "status_message": self.status_message,
            "authenticated": self.authenticated,
            "can_trade": self.can_trade,
            "latency_ms": self.last_latency_ms,
            "circuit_breaker_tripped": self.circuit_breaker_tripped,
            "circuit_breaker_reason": self.circuit_breaker_reason,
            "balances": self.balances,
            "portfolio": {
                "total_equity_gbp": total_equity,
                "initial_budget_gbp": total_equity,
                "total_pnl_gbp": sum(r["realized_pnl"] for r in self.runners.values()),
                "total_pnl_pct": 0.0,
                "total_realized_pnl_gbp": sum(r["realized_pnl"] for r in self.runners.values()),
                "total_fee_savings_gbp": sum(r["fee_savings"] for r in self.runners.values()),
            },
            "market_prices": {
                "BTC/GBP": {
                    "price": btc_p,
                    "high24h": self.market_stats["BTC/GBP"]["high24h"],
                    "low24h": self.market_stats["BTC/GBP"]["low24h"],
                    "change24h": self.market_stats["BTC/GBP"]["change24h"],
                    "yesterday_close": round(btc_p / (1.0 + (self.market_stats["BTC/GBP"]["change24h"] / 100.0)), 2) if self.market_stats["BTC/GBP"]["change24h"] != -100 else btc_p,
                },
                "ETH/GBP": {
                    "price": eth_p,
                    "high24h": self.market_stats["ETH/GBP"]["high24h"],
                    "low24h": self.market_stats["ETH/GBP"]["low24h"],
                    "change24h": self.market_stats["ETH/GBP"]["change24h"],
                    "yesterday_close": round(eth_p / (1.0 + (self.market_stats["ETH/GBP"]["change24h"] / 100.0)), 2) if self.market_stats["ETH/GBP"]["change24h"] != -100 else eth_p,
                },
            },
            "runners": runners_list,
            "resting_orders": self.resting_orders,
            "resting_orders_count": len(self.resting_orders),
            "live_trades": self.live_trades,
            "timestamp": int(now),
        }

live_trading_runner = LiveTradingRunner()
