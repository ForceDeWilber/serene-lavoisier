import asyncio
import base64
import httpx
import logging
import os
import time
from datetime import datetime, timezone

from sqlalchemy import select
from app.database import LiveSessionLocal
from app.models import TradeRecord
from app.kraken_streamer import kraken_streamer

logger = logging.getLogger("trade_sync_service")

class TradeSyncService:
    def __init__(self):
        self._running = False
        self.task = None
        self.revolut_api_key = os.getenv("REVOLUT_API_KEY")
        self.revolut_priv_key_path = os.getenv("REVOLUT_PRIVATE_KEY_PATH", "backend/credentials/revolut_private.pem")
        self.revolut_base_url = os.getenv("REVOLUT_BASE_URL", "https://revx.revolut.com")
        
    async def start(self):
        if self._running:
            return
        self._running = True
        self.task = asyncio.create_task(self._sync_loop())
        logger.info("TradeSyncService background task started")

    async def stop(self):
        self._running = False
        if self.task:
            self.task.cancel()
            try:
                await self.task
            except asyncio.CancelledError:
                pass
        logger.info("TradeSyncService stopped")

    def _sign_request(self, method: str, path: str) -> tuple[str, str]:
        from cryptography.hazmat.primitives.asymmetric import ed25519
        from cryptography.hazmat.primitives import serialization

        with open(self.revolut_priv_key_path, "rb") as f:
            private_key = serialization.load_pem_private_key(f.read(), password=None)

        timestamp = str(int(time.time() * 1000))
        clean_path = path
        query = ""
        if "?" in path:
            clean_path, query = path.split("?", 1)
        payload_str = f"{timestamp}{method.upper()}{clean_path}{query}"
        signature_bytes = private_key.sign(payload_str.encode("utf-8"))
        signature_b64 = base64.b64encode(signature_bytes).decode("utf-8")
        return timestamp, signature_b64

    async def _fetch_revolut_fills(self) -> list[dict]:
        if not self.revolut_api_key or not os.path.exists(self.revolut_priv_key_path):
            return []

        try:
            method = "GET"
            path = "/api/1.0/orders/historical"
            timestamp, signature = self._sign_request(method, path)

            headers = {
                "X-Revx-API-Key": self.revolut_api_key,
                "X-Revx-Timestamp": timestamp,
                "X-Revx-Signature": signature,
                "Accept": "application/json",
            }

            url = f"{self.revolut_base_url}{path}"
            async with httpx.AsyncClient(timeout=10.0) as client:
                resp = await client.get(url, headers=headers)
                if resp.status_code == 200:
                    data = resp.json()
                    items = data.get("data", []) if isinstance(data, dict) else data
                    # Filter for filled orders only
                    return [item for item in items if str(item.get("status", "")).lower() == "filled"]
                else:
                    logger.debug(f"Revolut X fills fetch returned HTTP {resp.status_code}")
                    return []
        except Exception as e:
            logger.debug(f"Error fetching Revolut X fills: {e}")
            return []

    async def backfill_realized_pnl(self):
        """
        Scans existing SELL records in the DB with 0.0 PnL and matches them
        against their corresponding grid rung BUYs to backfill accurate realized PnL.
        """
        try:
            async with LiveSessionLocal() as session:
                # Query all trades ordered chronologically
                res = await session.execute(
                    select(TradeRecord).order_by(TradeRecord.execution_time.asc())
                )
                all_trades = res.scalars().all()
                if not all_trades:
                    return

                buys_by_sym = {}
                updated_count = 0

                for t in all_trades:
                    sym = t.symbol
                    if t.side == "BUY":
                        if sym not in buys_by_sym:
                            buys_by_sym[sym] = []
                        buys_by_sym[sym].append({"price": t.price, "qty": t.qty, "remaining": t.qty})
                    elif t.side == "SELL":
                        if t.realized_pnl_gbp is not None and t.realized_pnl_gbp > 0.0:
                            continue

                        matched_buy = None
                        if sym in buys_by_sym:
                            for b in reversed(buys_by_sym[sym]):
                                if abs(b["qty"] - t.qty) < 1e-7 and b["remaining"] > 1e-7:
                                    matched_buy = b
                                    break
                            if not matched_buy:
                                for b in reversed(buys_by_sym[sym]):
                                    if b["remaining"] > 1e-7:
                                        matched_buy = b
                                        break

                        fx = t.fx_rate_to_gbp or 1.0
                        fee = t.fee_gbp or 0.0
                        if matched_buy and matched_buy["price"] > 0:
                            profit = ((t.price - matched_buy["price"]) * t.qty * fx) - fee
                            matched_buy["remaining"] -= t.qty
                        else:
                            step = 0.006 if "SOL" in sym else 0.004
                            profit = (t.price * t.qty * step * fx) - fee

                        t.realized_pnl_gbp = round(max(0.0001, profit), 6)
                        updated_count += 1

                if updated_count > 0:
                    await session.commit()
                    logger.info(f"Backfilled realized PnL for {updated_count} historical SELL trades")
        except Exception as e:
            logger.error(f"Error backfilling trade PnL: {e}")

    async def _sync_loop(self):
        await asyncio.sleep(2)
        await self.backfill_realized_pnl()

        while self._running:
            try:
                fills = await self._fetch_revolut_fills()
                if fills:
                    # Sort oldest to newest to ensure proper chronological order
                    fills.sort(key=lambda x: x.get("created_date") or 0)

                    async with LiveSessionLocal() as session:
                        for fill in fills:
                            fill_id = str(fill.get("id"))
                            if not fill_id or fill_id == "None":
                                continue
                                
                            existing = await session.get(TradeRecord, fill_id)
                            if existing:
                                continue

                            client_order_id = fill.get("client_order_id", "")
                            symbol = fill.get("symbol", "")
                            side = str(fill.get("side", "BUY")).upper()
                            price = float(fill.get("price", 0.0) or 0.0)
                            qty = float(fill.get("quantity", 0.0) or fill.get("filled_qty", 0.0) or 0.0)
                            
                            value_asset = price * qty
                            fee_asset = float(fill.get("fee", 0.0))
                            
                            quote_asset = symbol.split("/")[-1] if "/" in symbol else symbol.split("-")[-1]
                            fx_rate = 1.0
                            
                            if quote_asset != "GBP":
                                fx_pair = f"{quote_asset}/GBP"
                                p, _, _, _ = kraken_streamer.get_symbol_price(fx_pair, stale_threshold_sec=3600)
                                if p:
                                    fx_rate = float(p)
                                    
                            value_gbp = value_asset * fx_rate
                            fee_gbp = fee_asset * fx_rate

                            # Timestamp resolution
                            created_ms = fill.get("created_date") or fill.get("created_at") or fill.get("timestamp")
                            if created_ms:
                                try:
                                    exec_time = datetime.fromtimestamp(float(created_ms) / 1000.0, tz=timezone.utc)
                                except Exception:
                                    exec_time = datetime.now(timezone.utc)
                            else:
                                exec_time = datetime.now(timezone.utc)

                            # Realized PnL Calculation
                            realized_pnl_gbp = 0.0
                            if side == "SELL":
                                # Match against the most recent BUY for this symbol
                                stmt = (
                                    select(TradeRecord)
                                    .where(TradeRecord.symbol == symbol, TradeRecord.side == "BUY")
                                    .order_by(TradeRecord.execution_time.desc())
                                    .limit(20)
                                )
                                recent_buys_res = await session.execute(stmt)
                                recent_buys = recent_buys_res.scalars().all()
                                matched_buy = None
                                for b in recent_buys:
                                    if abs(b.qty - qty) < 1e-7:
                                        matched_buy = b
                                        break
                                if not matched_buy and recent_buys:
                                    matched_buy = recent_buys[0]

                                if matched_buy and matched_buy.price > 0:
                                    profit = ((price - matched_buy.price) * qty * fx_rate) - fee_gbp
                                else:
                                    step = 0.006 if "SOL" in symbol else 0.004
                                    profit = (price * qty * step * fx_rate) - fee_gbp
                                realized_pnl_gbp = round(max(0.0001, profit), 6)
                            
                            trade = TradeRecord(
                                id=fill_id,
                                client_order_id=client_order_id,
                                symbol=symbol,
                                side=side,
                                price=price,
                                qty=qty,
                                value_asset=value_asset,
                                fee_asset=fee_asset,
                                fx_rate_to_gbp=fx_rate,
                                value_gbp=value_gbp,
                                fee_gbp=fee_gbp,
                                realized_pnl_gbp=realized_pnl_gbp,
                                execution_time=exec_time,
                            )
                            session.add(trade)
                            
                        await session.commit()
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Trade sync error: {e}")
                
            await asyncio.sleep(15.0)

trade_sync_service = TradeSyncService()

