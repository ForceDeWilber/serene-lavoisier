import asyncio
import json
import logging
import time
from typing import Any, Dict, Optional, Tuple

import websockets

logger = logging.getLogger("kraken_ws_streamer")

class KrakenWsStreamer:
    """
    Direct WebSocket v2 streaming client for Kraken Pro market data.
    Maintains persistent connection, subscribes to real-time ticker feeds,
    and exposes live ticks without any synthetic caching or stale fallback data.
    """

    def __init__(self, ws_url: str = "wss://ws.kraken.com/v2", symbols: Optional[list] = None):
        self.ws_url = ws_url
        self.symbols = symbols or []
        self.live_ticks: Dict[str, Dict[str, Any]] = {}
        self.connected: bool = False
        self.last_connected_at: float = 0.0
        self.task: Optional[asyncio.Task] = None
        self._running: bool = False
        self.ws: Optional[Any] = None

    async def subscribe_symbol(self, symbol: str):
        if symbol not in self.symbols:
            self.symbols.append(symbol)
        if self.connected and self.ws:
            sub_payload = {
                "method": "subscribe",
                "params": {
                    "channel": "ticker",
                    "event_trigger": "bbo",
                    "symbol": [symbol],
                },
            }
            try:
                await self.ws.send(json.dumps(sub_payload))
                logger.info(f"Hot-subscribed Kraken WS v2 to symbol: {symbol}")
            except Exception as e:
                logger.error(f"Failed to hot-subscribe {symbol}: {e}")

    async def start(self):
        if self._running:
            return
        self._running = True
        self.task = asyncio.create_task(self._run_loop())
        logger.info(f"Kraken WebSocket streamer task launched for {self.symbols}")

    async def stop(self):
        self._running = False
        if self.task:
            self.task.cancel()
            try:
                await self.task
            except asyncio.CancelledError:
                pass
        self.connected = False
        logger.info("Kraken WebSocket streamer stopped")

    async def _run_loop(self):
        backoff = 1.0
        max_backoff = 15.0

        while self._running:
            try:
                logger.info(f"Connecting to Kraken WebSocket v2 at {self.ws_url}...")
                async with websockets.connect(
                    self.ws_url,
                    ping_interval=20,
                    ping_timeout=10,
                    close_timeout=5,
                ) as ws:
                    self.ws = ws
                    self.connected = True
                    self.last_connected_at = time.time()
                    backoff = 1.0
                    logger.info("Connected to Kraken WS v2. Subscribing to ticker channel...")

                    sub_payload = {
                        "method": "subscribe",
                        "params": {
                            "channel": "ticker",
                            "event_trigger": "bbo",
                            "symbol": self.symbols,
                        },
                    }
                    await ws.send(json.dumps(sub_payload))

                    while self._running:
                        try:
                            msg_raw = await ws.recv()
                            self._handle_message(msg_raw)
                        except asyncio.TimeoutError:
                            continue
            except asyncio.CancelledError:
                break
            except Exception as e:
                self.connected = False
                logger.warning(f"Kraken WS connection error: {e}. Reconnecting in {backoff:.1f}s...")
                await asyncio.sleep(backoff)
                backoff = min(backoff * 1.5, max_backoff)

    def _handle_message(self, raw_text: str):
        try:
            msg = json.loads(raw_text)
            channel = msg.get("channel")
            if channel != "ticker":
                return

            data_list = msg.get("data", [])
            now = time.time()

            for item in data_list:
                sym = item.get("symbol")
                if not sym:
                    continue

                # Preserve previous fields on partial update
                prev = self.live_ticks.get(sym, {})
                bid = float(item["bid"]) if item.get("bid") is not None else prev.get("bid")
                ask = float(item["ask"]) if item.get("ask") is not None else prev.get("ask")
                last = float(item["last"]) if item.get("last") is not None else prev.get("last")

                price = last or (round((bid + ask) / 2.0, 2) if (bid and ask) else bid or ask)
                if not price:
                    continue

                high = float(item["high"]) if item.get("high") is not None else prev.get("high24h")
                low = float(item["low"]) if item.get("low") is not None else prev.get("low24h")
                change_pct = float(item["change_pct"]) if item.get("change_pct") is not None else prev.get("change24h")

                self.live_ticks[sym] = {
                    "symbol": sym,
                    "price": round(price, 2),
                    "bid": round(bid, 2) if bid else None,
                    "ask": round(ask, 2) if ask else None,
                    "high24h": round(high, 2) if high is not None else None,
                    "low24h": round(low, 2) if low is not None else None,
                    "change24h": round(change_pct, 2) if change_pct is not None else 0.0,
                    "received_at": now,
                    "timestamp": item.get("timestamp") or time.strftime("%H:%M:%S", time.gmtime(now)),
                }
        except Exception as e:
            logger.debug(f"Error handling Kraken WS message: {e}")

    def get_symbol_price(self, symbol: str, stale_threshold_sec: float = 5.0) -> Tuple[Optional[float], str, Optional[str], Optional[Dict[str, Any]]]:
        """
        Returns (price, status, disclaimer, tick_dict).
        Status is one of:
          - 'LIVE': Fresh price received within stale_threshold_sec
          - 'OUTDATED': Price available but older than stale_threshold_sec
          - 'NO_DATA': No price has been received yet
        """
        tick = self.live_ticks.get(symbol)
        if not tick or tick.get("price") is None:
            return None, "NO_DATA", "No Data Received", None

        age = time.time() - tick.get("received_at", 0.0)
        if age > stale_threshold_sec:
            disclaimer = f"Outdated ({int(age)}s ago)"
            return tick["price"], "OUTDATED", disclaimer, tick

        return tick["price"], "LIVE", None, tick

kraken_streamer = KrakenWsStreamer()
