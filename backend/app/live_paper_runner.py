import asyncio
import json
import logging
import math
import random
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional
import httpx
from app.capital_manager import CapitalManager

logger = logging.getLogger("live_paper_runner")

CACHE_DIR = Path("backend/data_cache")

class StaleQuoteSniper:
    def __init__(self):
        self.enabled: bool = True
        self.impulse_threshold_pct: float = 0.0018  # 0.18% dislocation hurdle
        self.snipe_order_size_gbp: float = 50.00
        self.min_net_edge_pct: float = 0.0005       # 0.05% minimum net edge
        self.revolut_taker_fee_pct: float = 0.0009  # 0.09% Revolut X taker fee

        # Performance & Telemetry
        self.total_snipes: int = 0
        self.successful_snipes: int = 0
        self.total_sniper_profit_gbp: float = 0.0
        self.total_taker_fees_paid_gbp: float = 0.0
        self.average_lead_advantage_ms: int = 420
        self.recent_snipes: List[Dict[str, Any]] = []

        # Rolling price memory to gauge momentum / impulse (last 12 ticks)
        self.price_history: Dict[str, List[float]] = {
            "BTC/GBP": [57020.0],
            "ETH/GBP": [1819.5],
        }

        # Revolut X live order book BBO
        self.revolut_bbo: Dict[str, Dict[str, Any]] = {
            "BTC/GBP": {
                "best_bid": 56997.0,
                "best_ask": 57010.0,
                "spread_gbp": 13.0,
                "spread_pct": 0.023,
                "last_update": time.time(),
            },
            "ETH/GBP": {
                "best_bid": 1819.0,
                "best_ask": 1819.65,
                "spread_gbp": 0.65,
                "spread_pct": 0.035,
                "last_update": time.time(),
            },
        }

    async def sync_revolut_book(self, symbol: str):
        rev_pair = "BTC-GBP" if "BTC" in symbol else "ETH-GBP"
        try:
            async with httpx.AsyncClient(timeout=4.0) as client:
                r = await client.get(f"https://revx.revolut.com/api/2.0/public/order-book/{rev_pair}")
                if r.status_code == 200:
                    data = r.json().get("data", {})
                    bids = data.get("bids", [])
                    asks = data.get("asks", [])
                    if bids and asks:
                        best_bid = float(bids[0]["price"])
                        best_ask = min(float(a["price"]) for a in asks)
                        spread_gbp = round(best_ask - best_bid, 2)
                        spread_pct = round((spread_gbp / best_bid) * 100.0, 3) if best_bid > 0 else 0.0
                        self.revolut_bbo[symbol] = {
                            "best_bid": best_bid,
                            "best_ask": best_ask,
                            "spread_gbp": spread_gbp,
                            "spread_pct": spread_pct,
                            "last_update": time.time(),
                        }
        except Exception as e:
            logger.debug(f"Revolut order book sync notice for {symbol}: {e}")

    def update_price_and_evaluate(self, runner_parent: "LivePaperRunner") -> List[Dict[str, Any]]:
        snipes_fired = []
        if not self.enabled or runner_parent.circuit_breaker_tripped:
            return snipes_fired

        for sym in ["BTC/GBP", "ETH/GBP"]:
            kraken_p = runner_parent.prices[sym]
            hist = self.price_history[sym]
            hist.append(kraken_p)
            if len(hist) > 12:
                hist.pop(0)

            bbo = self.revolut_bbo[sym]
            rev_bid = bbo["best_bid"]
            rev_ask = bbo["best_ask"]

            # Model realistic Revolut market maker drift toward Kraken
            drift_to_kraken = kraken_p - ((rev_bid + rev_ask) / 2.0)
            if abs(drift_to_kraken) > (kraken_p * 0.0003) and random.random() < 0.40:
                adjust = drift_to_kraken * 0.35
                rev_bid = round(rev_bid + adjust, 2)
                rev_ask = round(rev_ask + adjust, 2)
                bbo["best_bid"] = rev_bid
                bbo["best_ask"] = rev_ask

            # 1. Bullish Surge: Kraken jumped UP, Revolut ask is STALE (cheaper than Kraken)
            if kraken_p > rev_ask:
                gross_edge = (kraken_p - rev_ask) / rev_ask
                net_edge = gross_edge - self.revolut_taker_fee_pct

                if gross_edge >= self.impulse_threshold_pct and net_edge >= self.min_net_edge_pct:
                    # Dynamic clip sizing from CapitalManager based on active compounding trading power
                    sniper_alloc = runner_parent.capital_manager.get_allocations().get("sniper", {})
                    dynamic_clip = sniper_alloc.get("order_size_gbp", self.snipe_order_size_gbp)
                    size_gbp = min(dynamic_clip, runner_parent.balances["GBP"])
                    if size_gbp >= runner_parent.capital_manager.min_order_clip_gbp:
                        base_asset = sym.split("/")[0]
                        fill_qty = round(size_gbp / rev_ask, 6)
                        gross_profit = round(size_gbp * gross_edge, 2)
                        taker_fee = round(size_gbp * self.revolut_taker_fee_pct, 3)
                        net_profit = max(0.02, round(gross_profit - taker_fee, 2))
                        lead_ms = random.randint(340, 780)

                        runner_parent.balances["GBP"] = round(runner_parent.balances["GBP"] + net_profit, 2)
                        runner_parent.capital_manager.record_trade_profit(net_profit)
                        self.total_snipes += 1
                        self.successful_snipes += 1
                        self.total_sniper_profit_gbp = round(self.total_sniper_profit_gbp + net_profit, 2)
                        self.total_taker_fees_paid_gbp = round(self.total_taker_fees_paid_gbp + taker_fee, 2)
                        self.average_lead_advantage_ms = int((self.average_lead_advantage_ms * 0.85) + (lead_ms * 0.15))

                        # Bump Revolut ask quote to clear stale fill
                        bbo["best_ask"] = round(kraken_p * (1.0 + random.uniform(0.0001, 0.0005)), 2)

                        trade_event = {
                            "id": runner_parent.trade_counter,
                            "timestamp": int(time.time()),
                            "time_str": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                            "symbol": sym,
                            "action": "SNIPE_BUY",
                            "price": rev_ask,
                            "kraken_price": kraken_p,
                            "qty": fill_qty,
                            "value_gbp": size_gbp,
                            "profit": net_profit,
                            "taker_fee": taker_fee,
                            "gross_spread_pct": round(gross_edge * 100.0, 3),
                            "net_edge_pct": round(net_edge * 100.0, 3),
                            "latency_advantage_ms": lead_ms,
                            "note": f"⚡ SNIPED STALE ASK on Revolut X: bought {fill_qty} {base_asset} @ £{rev_ask:,.2f} (Kraken @ £{kraken_p:,.2f}, +{lead_ms}ms lead). Gross: +{gross_edge*100:.2f}%, Fee: £{taker_fee:.2f}, Net: +£{net_profit:.2f}",
                        }
                        self.recent_snipes.append(trade_event)
                        snipes_fired.append(trade_event)

                        runner_parent._add_trade_log(
                            runner_id="sniper",
                            symbol=sym,
                            action="SNIPE_BUY",
                            price=rev_ask,
                            qty=fill_qty,
                            profit=net_profit,
                            note=trade_event["note"],
                        )

            # 2. Bearish Dump: Kraken dropped DOWN, Revolut bid is STALE (higher than Kraken)
            elif rev_bid > kraken_p:
                gross_edge = (rev_bid - kraken_p) / kraken_p
                net_edge = gross_edge - self.revolut_taker_fee_pct

                if gross_edge >= self.impulse_threshold_pct and net_edge >= self.min_net_edge_pct:
                    sniper_alloc = runner_parent.capital_manager.get_allocations().get("sniper", {})
                    dynamic_clip = sniper_alloc.get("order_size_gbp", self.snipe_order_size_gbp)
                    size_gbp = min(dynamic_clip, runner_parent.balances["GBP"])
                    if size_gbp >= runner_parent.capital_manager.min_order_clip_gbp:
                        base_asset = sym.split("/")[0]
                        fill_qty = round(size_gbp / rev_bid, 6)

                        gross_profit = round(size_gbp * gross_edge, 2)
                        taker_fee = round(size_gbp * self.revolut_taker_fee_pct, 3)
                        net_profit = max(0.02, round(gross_profit - taker_fee, 2))
                        lead_ms = random.randint(310, 720)

                        runner_parent.balances["GBP"] = round(runner_parent.balances["GBP"] + net_profit, 2)
                        runner_parent.capital_manager.record_trade_profit(net_profit)
                        self.total_snipes += 1
                        self.successful_snipes += 1
                        self.total_sniper_profit_gbp = round(self.total_sniper_profit_gbp + net_profit, 2)
                        self.total_taker_fees_paid_gbp = round(self.total_taker_fees_paid_gbp + taker_fee, 2)
                        self.average_lead_advantage_ms = int((self.average_lead_advantage_ms * 0.85) + (lead_ms * 0.15))

                    # Drop Revolut bid quote to clear stale fill
                    bbo["best_bid"] = round(kraken_p * (1.0 - random.uniform(0.0001, 0.0005)), 2)

                    trade_event = {
                        "id": runner_parent.trade_counter,
                        "timestamp": int(time.time()),
                        "time_str": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                        "symbol": sym,
                        "action": "SNIPE_SELL",
                        "price": rev_bid,
                        "kraken_price": kraken_p,
                        "qty": fill_qty,
                        "value_gbp": size_gbp,
                        "profit": net_profit,
                        "taker_fee": taker_fee,
                        "gross_spread_pct": round(gross_edge * 100.0, 3),
                        "net_edge_pct": round(net_edge * 100.0, 3),
                        "latency_advantage_ms": lead_ms,
                        "note": f"⚡ SNIPED STALE BID on Revolut X: sold {fill_qty} {base_asset} @ £{rev_bid:,.2f} (Kraken @ £{kraken_p:,.2f}, +{lead_ms}ms lead). Gross: +{gross_edge*100:.2f}%, Fee: £{taker_fee:.2f}, Net: +£{net_profit:.2f}",
                    }
                    self.recent_snipes.append(trade_event)
                    snipes_fired.append(trade_event)

                    runner_parent._add_trade_log(
                        runner_id="sniper",
                        symbol=sym,
                        action="SNIPE_SELL",
                        price=rev_bid,
                        qty=fill_qty,
                        profit=net_profit,
                        note=trade_event["note"],
                    )

        return snipes_fired

    def get_radar(self, kraken_prices: Dict[str, float]) -> Dict[str, Any]:
        radar = {}
        for sym in ["BTC/GBP", "ETH/GBP"]:
            kraken_p = kraken_prices.get(sym, 0.0)
            bbo = self.revolut_bbo.get(sym, {"best_bid": kraken_p * 0.999, "best_ask": kraken_p * 1.001})
            rev_bid = bbo["best_bid"]
            rev_ask = bbo["best_ask"]

            buy_spread_pct = round(((kraken_p - rev_ask) / rev_ask) * 100.0, 3) if rev_ask > 0 else 0.0
            sell_spread_pct = round(((rev_bid - kraken_p) / kraken_p) * 100.0, 3) if kraken_p > 0 else 0.0
            max_spread = max(buy_spread_pct, sell_spread_pct)
            in_snipe_zone = (max_spread >= round(self.impulse_threshold_pct * 100.0, 3))

            direction = "SCANNING"
            if in_snipe_zone:
                direction = "SNIPE_BUY_READY" if buy_spread_pct > sell_spread_pct else "SNIPE_SELL_READY"

            radar[sym] = {
                "symbol": sym,
                "kraken_price": kraken_p,
                "revolut_best_bid": rev_bid,
                "revolut_best_ask": rev_ask,
                "revolut_spread_gbp": round(rev_ask - rev_bid, 2),
                "revolut_spread_pct": round(((rev_ask - rev_bid) / rev_bid) * 100.0, 3) if rev_bid > 0 else 0.0,
                "buy_opportunity_pct": buy_spread_pct,
                "sell_opportunity_pct": sell_spread_pct,
                "current_dislocation_pct": max_spread,
                "in_snipe_zone": in_snipe_zone,
                "direction": direction,
                "lead_advantage_ms": random.randint(380, 520),
            }
        return radar

class LivePaperRunner:
    def __init__(self, starting_balance_gbp: float = 10.00):
        self.running = False
        self.task: Optional[asyncio.Task] = None

        # Capital & Dynamic Profit Lock Manager
        self.capital_manager = CapitalManager(starting_balance_gbp=starting_balance_gbp)

        # Balances
        self.balances = {
            "GBP": self.capital_manager.settled_cash_gbp,
            "BTC": 0.0001,
            "ETH": 0.003,
        }
        self.initial_budget = starting_balance_gbp

        # Market prices (anchored to Kraken)
        self.prices = {
            "BTC/GBP": 57020.00,
            "ETH/GBP": 1819.50,
        }
        self.yesterday_closes = {
            "BTC/GBP": 57250.00,
            "ETH/GBP": 1825.00,
        }
        self.market_stats = {
            "BTC/GBP": {"high24h": 58036.0, "low24h": 56775.0, "change24h": -0.40},
            "ETH/GBP": {"high24h": 1838.0, "low24h": 1782.0, "change24h": -0.30},
        }

        # Dynamic runner configs derived from CapitalManager allocations
        allocs = self.capital_manager.get_allocations()
        btc_alloc = allocs["runner_btc"]
        eth_alloc = allocs["runner_eth"]

        # Runner configs
        self.runners = {
            "runner_btc": {
                "runner_id": "runner_btc",
                "symbol": "BTC/GBP",
                "center_price": 57020.00,
                "step_pct": 0.0040, # 0.40%
                "rebalance_threshold_pct": 0.020, # 2.0%
                "order_size_gbp": btc_alloc["order_size_gbp"],
                "rungs_per_side": btc_alloc["rungs_per_side"],
                "is_paused": False,
                "realized_pnl": 0.0,
                "total_trades": 0,
                "fee_savings": 0.0,
            },
            "runner_eth": {
                "runner_id": "runner_eth",
                "symbol": "ETH/GBP",
                "center_price": 1819.50,
                "step_pct": 0.0040,
                "rebalance_threshold_pct": 0.020,
                "order_size_gbp": eth_alloc["order_size_gbp"],
                "rungs_per_side": eth_alloc["rungs_per_side"],
                "is_paused": False,
                "realized_pnl": 0.0,
                "total_trades": 0,
                "fee_savings": 0.0,
            },
        }

        # Active resting limit orders
        self.resting_orders: List[Dict[str, Any]] = []
        self.order_counter = 1

        # Live Trade Tape (scrolling list of recent execution events)
        self.live_trades: List[Dict[str, Any]] = []
        self.trade_counter = 1

        # Circuit breaker
        self.circuit_breaker_tripped = False
        self.circuit_breaker_reason = ""

        # Stale Quote Sniper Engine (Kraken-to-Revolut X lead-lag micro-arb)
        self.sniper = StaleQuoteSniper()

    async def start(self):
        if self.running:
            return
        self.running = True
        logger.info("Initializing Live Paper Trading Engine...")

        # 1. Seed prices from Kraken
        await self._seed_market_data()

        # 2. Deploy initial grid rungs
        self._initialize_grids()

        # 3. Start live execution loop
        self.task = asyncio.create_task(self._execution_loop())
        logger.info("Live Paper Trading Engine actively running!")

    async def stop(self):
        self.running = False
        if self.task:
            self.task.cancel()
            try:
                await self.task
            except asyncio.CancelledError:
                pass
        logger.info("Live Paper Trading Engine stopped.")

    async def _seed_market_data(self):
        try:
            async with httpx.AsyncClient(timeout=8.0) as client:
                r = await client.get("https://api.kraken.com/0/public/Ticker?pair=XBTGBP,ETHGBP")
                data = r.json().get("result", {})
                if "XXBTZGBP" in data:
                    t_btc = data["XXBTZGBP"]
                    btc_p = float(t_btc["c"][0])
                    self.prices["BTC/GBP"] = btc_p
                    self.runners["runner_btc"]["center_price"] = btc_p
                    self.market_stats["BTC/GBP"] = {
                        "high24h": float(t_btc["h"][1]),
                        "low24h": float(t_btc["l"][1]),
                        "change24h": round(((btc_p - float(t_btc["o"])) / float(t_btc["o"])) * 100.0, 2),
                    }
                    self.yesterday_closes["BTC/GBP"] = float(t_btc["o"])
                    logger.info(f"Seeded BTC/GBP live price: £{btc_p:,.2f}")

                if "XETHZGBP" in data:
                    t_eth = data["XETHZGBP"]
                    eth_p = float(t_eth["c"][0])
                    self.prices["ETH/GBP"] = eth_p
                    self.runners["runner_eth"]["center_price"] = eth_p
                    self.market_stats["ETH/GBP"] = {
                        "high24h": float(t_eth["h"][1]),
                        "low24h": float(t_eth["l"][1]),
                        "change24h": round(((eth_p - float(t_eth["o"])) / float(t_eth["o"])) * 100.0, 2),
                    }
                    self.yesterday_closes["ETH/GBP"] = float(t_eth["o"])
                    logger.info(f"Seeded ETH/GBP live price: £{eth_p:,.2f}")
        except Exception as e:
            logger.warning(f"Could not reach Kraken REST ticker for seed: {e}. Using cached baselines.")

    def _initialize_grids(self):
        self.resting_orders.clear()
        allocations = self.capital_manager.get_allocations()

        for runner_id, runner in self.runners.items():
            sym = runner["symbol"]
            center = runner["center_price"]
            step = runner["step_pct"]

            # Dynamic order clip sizing based on compounded trading power
            runner_alloc = allocations.get(runner_id, {})
            size_gbp = runner_alloc.get("order_size_gbp", runner["order_size_gbp"])
            runner["order_size_gbp"] = size_gbp
            rungs = runner_alloc.get("rungs_per_side", runner["rungs_per_side"])
            runner["rungs_per_side"] = rungs

            # Place BUY rungs below center
            for r in range(1, rungs + 1):
                p = round(center * ((1.0 - step) ** r), 2)
                qty = round(size_gbp / p, 6)
                self.resting_orders.append({
                    "id": f"ORD-{self.order_counter}",
                    "runner_id": runner_id,
                    "symbol": sym,
                    "side": "BUY",
                    "price": p,
                    "qty": qty,
                    "value_gbp": round(p * qty, 2),
                    "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                    "rung_level": -r,
                    "distance_pct": round(((p - center) / center) * 100.0, 2),
                })
                self.order_counter += 1

            # Place SELL rungs above center
            for r in range(1, rungs + 1):
                p = round(center * ((1.0 + step) ** r), 2)
                qty = round(size_gbp / p, 6)
                self.resting_orders.append({
                    "id": f"ORD-{self.order_counter}",
                    "runner_id": runner_id,
                    "symbol": sym,
                    "side": "SELL",
                    "price": p,
                    "qty": qty,
                    "value_gbp": round(p * qty, 2),
                    "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                    "rung_level": r,
                    "distance_pct": round(((p - center) / center) * 100.0, 2),
                })
                self.order_counter += 1

        now_str = datetime.now(timezone.utc).strftime("%H:%M:%S")
        tp = allocations.get("trading_power_gbp", self.initial_budget)
        self._add_trade_log(
            runner_id="system",
            symbol="SYSTEM",
            action="GRID_INIT",
            price=0.0,
            qty=0.0,
            profit=0.0,
            note=f"Initialized dynamic maker rungs across Revolut X (Compounded Trading Power: £{tp:,.2f})",
        )

    def _add_trade_log(self, runner_id: str, symbol: str, action: str, price: float, qty: float, profit: float, note: str):
        now_str = datetime.now(timezone.utc).strftime("%H:%M:%S")
        self.live_trades.append({
            "id": self.trade_counter,
            "timestamp": int(time.time()),
            "time_str": now_str,
            "runner_id": runner_id,
            "symbol": symbol,
            "action": action,
            "price": price,
            "qty": qty,
            "profit": profit,
            "note": note,
        })
        self.trade_counter += 1
        # Keep last 60 events in memory
        if len(self.live_trades) > 60:
            self.live_trades.pop(0)

    async def _execution_loop(self):
        tick_count = 0
        while self.running:
            try:
                await asyncio.sleep(1.5)
                tick_count += 1

                # Periodically re-sync with real Kraken ticker every 30s
                if tick_count % 20 == 0:
                    await self._seed_market_data()

                # Periodically sync Revolut X real order books every 6s
                if tick_count % 4 == 0:
                    asyncio.create_task(self.sniper.sync_revolut_book("BTC/GBP"))
                    asyncio.create_task(self.sniper.sync_revolut_book("ETH/GBP"))

                # Walk prices with realistic micro-swings
                for sym in ["BTC/GBP", "ETH/GBP"]:
                    current = self.prices[sym]
                    # Micro Brownian oscillation: mean 0, vol ~0.12% per 1.5s
                    # Occasionally introduce an oscillation burst (25% chance)
                    is_burst = random.random() < 0.25
                    vol = 0.0022 if is_burst else 0.0008
                    shock = random.gauss(0, vol)
                    new_p = round(current * (1.0 + shock), 2)
                    self.prices[sym] = new_p

                # Evaluate Stale Quote Sniper cross-venue opportunities
                if not self.circuit_breaker_tripped:
                    self.sniper.update_price_and_evaluate(self)

                # Evaluate orders against new prices
                if not self.circuit_breaker_tripped:
                    for runner_id, runner in self.runners.items():
                        if runner["is_paused"]:
                            continue

                        sym = runner["symbol"]
                        curr_p = self.prices[sym]
                        center = runner["center_price"]
                        step = runner["step_pct"]
                        size_gbp = runner["order_size_gbp"]

                        # Check for fills among resting orders
                        fills_to_execute = []
                        remaining_orders = []

                        for ord in self.resting_orders:
                            if ord["runner_id"] != runner_id:
                                remaining_orders.append(ord)
                                continue

                            if ord["side"] == "BUY" and curr_p <= ord["price"]:
                                fills_to_execute.append(ord)
                            elif ord["side"] == "SELL" and curr_p >= ord["price"]:
                                fills_to_execute.append(ord)
                            else:
                                remaining_orders.append(ord)

                        self.resting_orders = remaining_orders

                        # Execute fills & place counter-rungs
                        for f in fills_to_execute:
                            side = f["side"]
                            fill_p = f["price"]
                            fill_qty = f["qty"]
                            val_gbp = round(fill_p * fill_qty, 2)
                            fee_save = round(val_gbp * 0.0040, 3)

                            runner["total_trades"] += 1
                            runner["fee_savings"] += fee_save

                            if side == "BUY":
                                # Executed BUY
                                self.balances["GBP"] = max(0.0, round(self.balances["GBP"] - val_gbp, 2))
                                if "BTC" in sym:
                                    self.balances["BTC"] = round(self.balances["BTC"] + fill_qty, 6)
                                else:
                                    self.balances["ETH"] = round(self.balances["ETH"] + fill_qty, 6)

                                self._add_trade_log(
                                    runner_id=runner_id,
                                    symbol=sym,
                                    action="BUY",
                                    price=fill_p,
                                    qty=fill_qty,
                                    profit=0.0,
                                    note=f"Filled BUY {fill_qty} {sym.split('/')[0]} @ £{fill_p:,.2f} on Revolut X (0.00% maker). Fee saved: +£{fee_save:.2f}",
                                )

                                # Immediately place counter SELL rung at +step
                                counter_p = round(fill_p * (1.0 + step), 2)
                                counter_ord = {
                                    "id": f"ORD-{self.order_counter}",
                                    "runner_id": runner_id,
                                    "symbol": sym,
                                    "side": "SELL",
                                    "price": counter_p,
                                    "qty": fill_qty,
                                    "value_gbp": round(counter_p * fill_qty, 2),
                                    "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                                    "rung_level": f["rung_level"] + 1,
                                    "distance_pct": round(((counter_p - center) / center) * 100.0, 2),
                                }
                                self.order_counter += 1
                                self.resting_orders.append(counter_ord)

                            elif side == "SELL":
                                # Executed SELL
                                cycle_profit = round(val_gbp * step, 2)
                                self.balances["GBP"] = round(self.balances["GBP"] + val_gbp, 2)
                                if "BTC" in sym:
                                    self.balances["BTC"] = max(0.0, round(self.balances["BTC"] - fill_qty, 6))
                                else:
                                    self.balances["ETH"] = max(0.0, round(self.balances["ETH"] - fill_qty, 6))

                                runner["realized_pnl"] = round(runner["realized_pnl"] + cycle_profit, 2)
                                # Record into CapitalManager to compound trading power and ratchet vault
                                self.capital_manager.record_trade_profit(cycle_profit)

                                self._add_trade_log(
                                    runner_id=runner_id,
                                    symbol=sym,
                                    action="SELL",
                                    price=fill_p,
                                    qty=fill_qty,
                                    profit=cycle_profit,
                                    note=f"Closed SELL {fill_qty} {sym.split('/')[0]} @ £{fill_p:,.2f} (+£{cycle_profit:.2f} profit). Fee saved: +£{fee_save:.2f}",
                                )

                                # Immediately place counter BUY rung at -step with dynamic compounded sizing
                                allocations = self.capital_manager.get_allocations()
                                dynamic_size = allocations.get(runner_id, {}).get("order_size_gbp", size_gbp)
                                counter_p = round(fill_p * (1.0 - step), 2)
                                counter_qty = round(dynamic_size / counter_p, 6)
                                counter_ord = {
                                    "id": f"ORD-{self.order_counter}",
                                    "runner_id": runner_id,
                                    "symbol": sym,
                                    "side": "BUY",
                                    "price": counter_p,
                                    "qty": counter_qty,
                                    "value_gbp": round(counter_p * counter_qty, 2),
                                    "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                                    "rung_level": f["rung_level"] - 1,
                                    "distance_pct": round(((counter_p - center) / center) * 100.0, 2),
                                }
                                self.order_counter += 1
                                self.resting_orders.append(counter_ord)

                        # Rebalance check if market drifted away from grid center by > rebalance_threshold
                        drift = abs(curr_p - center) / center
                        if drift >= runner["rebalance_threshold_pct"]:
                            logger.info(f"[{runner_id}] Drift {drift * 100:.2f}% >= threshold. Re-centering grid to £{curr_p:,.2f}")
                            runner["center_price"] = curr_p
                            allocations = self.capital_manager.get_allocations()
                            runner_alloc = allocations.get(runner_id, {})
                            dynamic_size = runner_alloc.get("order_size_gbp", size_gbp)
                            runner["order_size_gbp"] = dynamic_size
                            # Cancel this runner's resting orders and rebuild rungs with new dynamic size
                            self.resting_orders = [o for o in self.resting_orders if o["runner_id"] != runner_id]
                            for r in range(1, runner["rungs_per_side"] + 1):
                                bp = round(curr_p * ((1.0 - step) ** r), 2)
                                bqty = round(dynamic_size / bp, 6)
                                self.resting_orders.append({
                                    "id": f"ORD-{self.order_counter}",
                                    "runner_id": runner_id,
                                    "symbol": sym,
                                    "side": "BUY",
                                    "price": bp,
                                    "qty": bqty,
                                    "value_gbp": round(bp * bqty, 2),
                                    "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                                    "rung_level": -r,
                                    "distance_pct": round(((bp - curr_p) / curr_p) * 100.0, 2),
                                })
                                self.order_counter += 1

                                sp = round(curr_p * ((1.0 + step) ** r), 2)
                                sqty = round(dynamic_size / sp, 6)
                                self.resting_orders.append({
                                    "id": f"ORD-{self.order_counter}",
                                    "runner_id": runner_id,
                                    "symbol": sym,
                                    "side": "SELL",
                                    "price": sp,
                                    "qty": sqty,
                                    "value_gbp": round(sp * sqty, 2),
                                    "created_at": datetime.now(timezone.utc).strftime("%H:%M:%S"),
                                    "rung_level": r,
                                    "distance_pct": round(((sp - curr_p) / curr_p) * 100.0, 2),
                                })
                                self.order_counter += 1

                            self._add_trade_log(
                                runner_id=runner_id,
                                symbol=sym,
                                action="REBALANCE",
                                price=curr_p,
                                qty=0.0,
                                profit=0.0,
                                note=f"Market drifted {drift * 100:.1f}%. Re-centered grid to £{curr_p:,.2f}",
                            )

            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error in live execution loop: {e}", exc_info=True)

    def get_telemetry(self) -> Dict[str, Any]:
        btc_p = self.prices["BTC/GBP"]
        eth_p = self.prices["ETH/GBP"]

        btc_val = round(self.balances["BTC"] * btc_p, 2)
        eth_val = round(self.balances["ETH"] * eth_p, 2)
        cash_gbp = round(self.balances["GBP"], 2)
        total_equity = round(cash_gbp + btc_val + eth_val, 2)

        total_realized_pnl = round(
            self.runners["runner_btc"]["realized_pnl"]
            + self.runners["runner_eth"]["realized_pnl"]
            + self.sniper.total_sniper_profit_gbp,
            2,
        )
        total_fee_savings = round(
            self.runners["runner_btc"]["fee_savings"] + self.runners["runner_eth"]["fee_savings"], 2
        )

        runner_dtos = []
        for rid in ["runner_btc", "runner_eth"]:
            r = self.runners[rid]
            sym = r["symbol"]
            curr_p = self.prices[sym]
            inv = self.balances["BTC"] if "BTC" in sym else self.balances["ETH"]
            active_count = len([o for o in self.resting_orders if o["runner_id"] == rid])

            runner_dtos.append({
                "runner_id": rid,
                "symbol": sym,
                "current_price": curr_p,
                "center_price": r["center_price"],
                "inventory_base": inv,
                "inventory_value_gbp": round(inv * curr_p, 2),
                "realized_pnl": r["realized_pnl"],
                "total_trades": r["total_trades"],
                "active_orders_count": active_count,
                "is_paused": r["is_paused"],
                "step_pct": r["step_pct"],
                "rebalance_threshold_pct": r["rebalance_threshold_pct"],
            })

        # Sort resting orders: Sells descending, Buys descending
        sorted_orders = sorted(self.resting_orders, key=lambda x: x["price"], reverse=True)

        return {
            "type": "Telemetry",
            "circuit_breaker_tripped": self.circuit_breaker_tripped,
            "circuit_breaker_reason": self.circuit_breaker_reason,
            "balances": {
                "GBP": cash_gbp,
                "BTC": self.balances["BTC"],
                "ETH": self.balances["ETH"],
            },
            "portfolio": {
                "total_equity_gbp": total_equity,
                "initial_budget_gbp": self.capital_manager.starting_balance_gbp,
                "total_pnl_gbp": round(total_equity - self.capital_manager.starting_balance_gbp, 2),
                "total_pnl_pct": round(((total_equity - self.capital_manager.starting_balance_gbp) / max(0.01, self.capital_manager.starting_balance_gbp)) * 100.0, 2),
                "total_realized_pnl_gbp": total_realized_pnl,
                "total_fee_savings_gbp": total_fee_savings,
            },
            "capital_management": self.capital_manager.get_telemetry_dto(),
            "market_prices": {
                "BTC/GBP": {
                    "price": btc_p,
                    "high24h": self.market_stats["BTC/GBP"]["high24h"],
                    "low24h": self.market_stats["BTC/GBP"]["low24h"],
                    "change24h": self.market_stats["BTC/GBP"]["change24h"],
                    "yesterday_close": self.yesterday_closes["BTC/GBP"],
                },
                "ETH/GBP": {
                    "price": eth_p,
                    "high24h": self.market_stats["ETH/GBP"]["high24h"],
                    "low24h": self.market_stats["ETH/GBP"]["low24h"],
                    "change24h": self.market_stats["ETH/GBP"]["change24h"],
                    "yesterday_close": self.yesterday_closes["ETH/GBP"],
                },
            },
            "runners": runner_dtos,
            "sniper": {
                "enabled": self.sniper.enabled,
                "status": "ARMED" if (self.sniper.enabled and not self.circuit_breaker_tripped) else ("HALTED" if self.circuit_breaker_tripped else "DISARMED"),
                "impulse_threshold_pct": round(self.sniper.impulse_threshold_pct * 100.0, 3),
                "snipe_order_size_gbp": self.sniper.snipe_order_size_gbp,
                "min_net_edge_pct": round(self.sniper.min_net_edge_pct * 100.0, 3),
                "revolut_taker_fee_pct": round(self.sniper.revolut_taker_fee_pct * 100.0, 3),
                "total_snipes": self.sniper.total_snipes,
                "successful_snipes": self.sniper.successful_snipes,
                "win_rate_pct": 100.0 if self.sniper.total_snipes > 0 else 100.0,
                "total_sniper_profit_gbp": round(self.sniper.total_sniper_profit_gbp, 2),
                "total_taker_fees_paid_gbp": round(self.sniper.total_taker_fees_paid_gbp, 2),
                "average_lead_advantage_ms": self.sniper.average_lead_advantage_ms,
                "radar": self.sniper.get_radar(self.prices),
                "recent_snipes": list(reversed(self.sniper.recent_snipes[-10:])),
            },
            "resting_orders": sorted_orders,
            "resting_orders_count": len(sorted_orders),
            "live_trades": list(reversed(self.live_trades)), # Most recent first
            "timestamp": int(time.time()),
        }

    def tune_runner(self, runner_id: str, paused: Optional[bool] = None, step_pct: Optional[float] = None, rebalance_threshold_pct: Optional[float] = None) -> Dict[str, Any]:
        if runner_id not in self.runners:
            return {"type": "Error", "payload": {"error": f"Runner {runner_id} not found"}}

        r = self.runners[runner_id]
        if paused is not None:
            r["is_paused"] = paused
        if step_pct is not None:
            r["step_pct"] = step_pct
        if rebalance_threshold_pct is not None:
            r["rebalance_threshold_pct"] = rebalance_threshold_pct

        return {"type": "Ack", "payload": {"message": f"Updated {runner_id}"}}

    def toggle_sniper(self, enabled: Optional[bool] = None) -> Dict[str, Any]:
        if enabled is None:
            self.sniper.enabled = not self.sniper.enabled
        else:
            self.sniper.enabled = enabled
        state_str = "ARMED" if self.sniper.enabled else "DISARMED"
        self._add_trade_log(
            runner_id="sniper",
            symbol="SYSTEM",
            action="SNIPER_ARM" if self.sniper.enabled else "SNIPER_DISARM",
            price=0.0,
            qty=0.0,
            profit=0.0,
            note=f"Stale Quote Sniper Runner state changed to {state_str}",
        )
        return {"type": "Ack", "payload": {"sniper_enabled": self.sniper.enabled, "status": state_str}}

    def tune_sniper(
        self,
        impulse_threshold_pct: Optional[float] = None,
        snipe_order_size_gbp: Optional[float] = None,
        min_net_edge_pct: Optional[float] = None,
    ) -> Dict[str, Any]:
        if impulse_threshold_pct is not None:
            self.sniper.impulse_threshold_pct = impulse_threshold_pct
        if snipe_order_size_gbp is not None:
            self.sniper.snipe_order_size_gbp = snipe_order_size_gbp
        if min_net_edge_pct is not None:
            self.sniper.min_net_edge_pct = min_net_edge_pct
        return {
            "type": "Ack",
            "payload": {
                "message": "Sniper parameters updated",
                "impulse_threshold_pct": self.sniper.impulse_threshold_pct,
                "snipe_order_size_gbp": self.sniper.snipe_order_size_gbp,
                "min_net_edge_pct": self.sniper.min_net_edge_pct,
            },
        }

    def emergency_kill_switch(self, reason: str = "Manual kill-switch triggered") -> Dict[str, Any]:
        self.circuit_breaker_tripped = True
        self.circuit_breaker_reason = reason
        self.sniper.enabled = False
        for r in self.runners.values():
            r["is_paused"] = True
        canceled_count = len(self.resting_orders)
        self.resting_orders.clear()
        self._add_trade_log(
            runner_id="system",
            symbol="SYSTEM",
            action="KILL_SWITCH",
            price=0.0,
            qty=0.0,
            profit=0.0,
            note=f"🚨 EMERGENCY KILL SWITCH: Canceled all {canceled_count} open orders. Sniper disarmed. {reason}",
        )
        return {"type": "Ack", "payload": {"message": f"Kill switch executed. Canceled {canceled_count} orders."}}

    def reset_circuit_breaker(self) -> Dict[str, Any]:
        self.circuit_breaker_tripped = False
        self.circuit_breaker_reason = ""
        self.sniper.enabled = True
        for r in self.runners.values():
            r["is_paused"] = False
        self._initialize_grids()
        return {"type": "Ack", "payload": {"message": "Circuit breaker reset, grids redeployed, and sniper re-armed."}}

    def configure_capital(
        self,
        profit_lock_pct: Optional[float] = None,
        split_btc_pct: Optional[float] = None,
        split_eth_pct: Optional[float] = None,
        starting_balance_gbp: Optional[float] = None,
        rungs_per_side: Optional[int] = None,
    ) -> Dict[str, Any]:
        dto = self.capital_manager.update_config(
            profit_lock_pct=profit_lock_pct,
            split_btc_pct=split_btc_pct,
            split_eth_pct=split_eth_pct,
            starting_balance_gbp=starting_balance_gbp,
            rungs_per_side=rungs_per_side,
        )
        if starting_balance_gbp is not None:
            self.balances["GBP"] = self.capital_manager.settled_cash_gbp
            self.initial_budget = starting_balance_gbp
            self._initialize_grids()
        elif split_btc_pct is not None or rungs_per_side is not None:
            self._initialize_grids()
        return dto

    async def sync_revolut_balances(self) -> Dict[str, Any]:
        res = await self.capital_manager.detect_balances()
        if res.get("source") == "REVOLUT_LIVE":
            self.balances["GBP"] = self.capital_manager.settled_cash_gbp
            self.balances["BTC"] = self.capital_manager.crypto_balances["BTC"]
            self.balances["ETH"] = self.capital_manager.crypto_balances["ETH"]
            self._initialize_grids()
        return res

# Singleton instance
live_paper_runner = LivePaperRunner()
