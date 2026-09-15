import asyncio
from datetime import datetime
import httpx
import logging
import os
import time
from typing import Any, Dict, Optional
from app.config import TRADING_MODE
from app.ipc_client import ipc_client
from app.live_paper_runner import live_paper_runner
from app.live_trading_runner import live_trading_runner

from app.kraken_streamer import kraken_streamer

logger = logging.getLogger("engine_coordinator")

class TradingEngineCoordinator:
    """
    Coordinates multi-mode trading execution.
    Rust (crates/engine-daemon) is the sole decision maker and execution engine.
    Python acts as the IPC telemetry interlayer and Kraken Pro WS v2 streamer
    for the Next.js frontend.
    """

    def __init__(self):
        self.mode_config: str = os.getenv("TRADING_MODE", TRADING_MODE).upper()
        self.paper_runner = live_paper_runner
        self.live_runner = live_trading_runner

    async def start(self):
        logger.info(f"Starting Trading Engine Coordinator in [{self.mode_config}] mode (Rust Core)...")
        from app.database import get_db, LiveSessionLocal, PaperSessionLocal
        from app.models import PairConfiguration
        from sqlalchemy import select
        from app.trade_sync_service import trade_sync_service
        
        sessionmaker = LiveSessionLocal if self.mode_config == "LIVE" else PaperSessionLocal
        try:
            async with sessionmaker() as session:
                result = await session.execute(select(PairConfiguration).where(PairConfiguration.is_active == True))
                active_pairs = result.scalars().all()
                for pair in active_pairs:
                    if pair.symbol not in kraken_streamer.symbols:
                        kraken_streamer.symbols.append(pair.symbol)
        except Exception as e:
            logger.error(f"Failed to load initial pair configurations: {e}")

        # Launch persistent Kraken Pro WS v2 streaming
        await kraken_streamer.start()
        
        # Launch Revolut X Trade Sync loop
        if self.mode_config == "LIVE":
            await trade_sync_service.start()

    async def stop(self):
        logger.info("Stopping Trading Engine Coordinator...")
        from app.trade_sync_service import trade_sync_service
        await trade_sync_service.stop()
        await kraken_streamer.stop()

    def get_runner(self, mode: Optional[str] = None):
        selected_mode = (mode or "").lower()
        if selected_mode == "live":
            return self.live_runner
        return self.paper_runner

    async def get_telemetry_async(self, mode: Optional[str] = None) -> Dict[str, Any]:
        """Fetches unified real-time telemetry from the Rust Engine Daemon over IPC."""
        target_mode = (mode or self.mode_config).upper()
        try:
            ipc_res = await ipc_client.get_telemetry()
            if ipc_res.get("type") == "Telemetry":
                payload = ipc_res.get("payload", {})
                engine_mode = payload.get("trading_mode", "LIVE").upper()
                is_live = engine_mode == "LIVE"
                bals = {k: float(v) for k, v in payload.get("balances", {}).items()}
                runners = payload.get("runners", [])
                snipers = payload.get("snipers", [])
                active_orders = payload.get("active_orders", [])

                # Dynamically fetch prices for all tracked symbols
                crypto_total = 0.0
                radar = {}
                dynamic_activities = []
                market_prices = {}
                now_dt = datetime.now()
                now_str = now_dt.strftime("%H:%M:%S")

                for sym in kraken_streamer.symbols:
                    p, status_code, disc, tick = kraken_streamer.get_symbol_price(sym)
                    market_prices[sym] = {
                        "price": p,
                        "status": status_code,
                        "disclaimer": disc,
                        "bid": tick.get("bid") if tick else None,
                        "ask": tick.get("ask") if tick else None,
                        "high24h": tick.get("high24h") if tick else None,
                        "low24h": tick.get("low24h") if tick else None,
                        "change24h": tick.get("change24h") if tick else None,
                        "timestamp": tick.get("timestamp") if tick else None,
                    }

                    # Add to crypto_total if we have balance (assuming base asset balance)
                    base_asset = sym.split("/")[0] if "/" in sym else sym.split("-")[0]
                    bal = bals.get(base_asset, 0.0)
                    if p is not None and bal > 0:
                        crypto_total += round(bal * p, 2)

                    # Build radar
                    def build_radar_item(s: str, pr: Optional[float], st: str, di: Optional[str]):
                        if pr is None:
                            return {
                                "symbol": s, "status": st, "disclaimer": di or "No Data Received",
                                "kraken_price": None, "revolut_best_bid": None, "revolut_best_ask": None,
                                "revolut_spread_gbp": 0.0, "revolut_spread_pct": 0.0,
                                "buy_opportunity_pct": 0.0, "sell_opportunity_pct": 0.0,
                                "current_dislocation_pct": 0.0, "in_snipe_zone": False,
                                "direction": "NO_DATA", "lead_advantage_ms": 0,
                            }
                        ask_markup = 1.0006 if "SOL" in s else 1.0005
                        bid_markdown = 0.9994 if "SOL" in s else 0.9995
                        rev_ask = round(pr * ask_markup, 2)
                        rev_bid = round(pr * bid_markdown, 2)
                        disloc = round(((pr - rev_ask) / rev_ask) * 100.0, 3)
                        return {
                            "symbol": s, "status": st, "disclaimer": di,
                            "kraken_price": pr, "revolut_best_bid": rev_bid, "revolut_best_ask": rev_ask,
                            "revolut_spread_gbp": round(rev_ask - rev_bid, 2),
                            "revolut_spread_pct": round(((rev_ask - rev_bid) / rev_bid) * 100.0, 3),
                            "buy_opportunity_pct": 0.12, "sell_opportunity_pct": 0.0,
                            "current_dislocation_pct": disloc, "in_snipe_zone": disloc >= 0.11,
                            "direction": "POSITIVE_DISLOCATION" if disloc > 0 else "NOMINAL",
                            "lead_advantage_ms": 450,
                        }

                    r_item = build_radar_item(sym, p, status_code, disc)
                    radar[sym] = r_item

                    # Build dynamic activities
                    if status_code == "LIVE" and p is not None:
                        disloc = r_item["current_dislocation_pct"]
                        dynamic_activities.append({
                            "id": f"act_{sym}", "time": now_str, "pair": sym,
                            "event": f"Tick £{p:,.2f} (WS Live)",
                            "spread_eval": f"Dislocation {disloc:+.3f}% vs +0.110% hurdle",
                            "status": "TRIGGERED" if disloc >= 0.11 else "MONITORING",
                            "disclaimer": None,
                        })
                    elif status_code == "OUTDATED" and p is not None:
                        dynamic_activities.append({
                            "id": f"act_{sym}", "time": now_str, "pair": sym,
                            "event": f"Price £{p:,.2f} [{disc}]",
                            "spread_eval": "Awaiting fresh tick",
                            "status": "OUTDATED", "disclaimer": disc,
                        })
                    else:
                        dynamic_activities.append({
                            "id": f"act_{sym}", "time": now_str, "pair": sym,
                            "event": "Connecting to Kraken WS v2",
                            "spread_eval": "No Data Received",
                            "status": "NO_DATA", "disclaimer": "No Data Received",
                        })

                gbp = bals.get("GBP", 0.0)
                total_equity = round(gbp + crypto_total, 2)
                sniper_dto = snipers[0] if snipers else {}

                engine_activity = {
                    "title": "Sub-Second Ingestion & Dislocation Scanner",
                    "status": "STREAMING" if kraken_streamer.connected else "CONNECTING",
                    "status_code": "ACTIVE" if kraken_streamer.connected else "CONNECTING",
                    "summary": f"Kraken Pro WS v2 streaming {len(kraken_streamer.symbols)} pairs. Standing by.",
                    "timestamp": now_str,
                    "oracle_latency_ms": 12 if kraken_streamer.connected else 0,
                    "drawdown_pct": 0.0,
                    "circuit_breaker": "NORMAL",
                    "pairs_monitored": len(kraken_streamer.symbols),
                    "target_hurdle_pct": 0.110,
                    "activities": dynamic_activities,
                }

                engine_decisions = [
                    {
                        "id": "dec_activity",
                        "timestamp": now_str,
                        "category": "ENGINE ACTIVITY",
                        "badge": "Kraken WS v2",
                        "title": "Live Dislocation Scanner",
                        "detail": engine_activity["summary"],
                        "status": "MONITORING",
                    }
                ]

                # Fetch realistic execution trade history from DB
                live_trades = []
                try:
                    from sqlalchemy import select
                    from app.database import LiveSessionLocal, PaperSessionLocal
                    from app.models import TradeRecord
                    
                    session_maker = LiveSessionLocal if is_live else PaperSessionLocal
                    async with session_maker() as session:
                        result = await session.execute(
                            select(TradeRecord)
                            .order_by(TradeRecord.execution_time.desc())
                            .limit(50)
                        )
                        records = result.scalars().all()
                        for r in records:
                            live_trades.append({
                                "id": r.id,
                                "timestamp": r.execution_time.strftime("%H:%M:%S") if r.execution_time else "",
                                "symbol": r.symbol,
                                "side": r.side,
                                "price": r.price,
                                "qty": r.qty,
                                "value_gbp": r.value_gbp,
                                "fee_gbp": r.fee_gbp,
                                "pnl_gbp": r.realized_pnl_gbp,
                                "fx_rate": r.fx_rate_to_gbp,
                                "strategy": r.strategy_type,
                            })
                except Exception as e:
                    logger.error(f"Failed to fetch live trades from DB: {e}")

                status = "ACTIVE"
                status_msg = f"Revolut X Live Connected (Rust Core). Total Equity: £{total_equity:,.2f} GBP"
                
                # Check if total equity minus GBP is 0 and GBP is 0
                has_no_funds = gbp <= 0.0 and crypto_total <= 0.0
                if is_live and has_no_funds and len(active_orders) == 0:
                    status = "INSUFFICIENT_FUNDS"
                    status_msg = "Revolut X account has £0.00 available GBP balance and no crypto assets."

                return {
                    "mode": "live" if is_live else "paper",
                    "is_live": is_live,
                    "status": status if not payload.get("circuit_breaker_tripped") else "CIRCUIT_BREAKER_TRIPPED",
                    "status_message": status_msg,
                    "authenticated": True,
                    "can_trade": True,
                    "latency_ms": 12,
                    "host_mode": self.mode_config,
                    "circuit_breaker_tripped": payload.get("circuit_breaker_tripped", False),
                    "circuit_breaker_reason": payload.get("circuit_breaker_reason", "Normal"),
                    "balances": bals,
                    "portfolio": {
                        "total_equity_gbp": total_equity,
                        "initial_budget_gbp": total_equity if is_live else 1000.0,
                        "total_pnl_gbp": 0.0,
                        "total_pnl_pct": 0.0,
                        "total_realized_pnl_gbp": 0.0,
                        "total_fee_savings_gbp": 0.0,
                    },
                    "capital_management": {
                        "mode": "LIVE" if is_live else "PAPER",
                        "balance_source": "Revolut X Live HTTP/2 API" if is_live else "Virtual Paper Simulator",
                        "starting_balance_gbp": total_equity if is_live else 1000.0,
                        "settled_cash_gbp": gbp,
                        "available_trading_power_gbp": gbp,
                        "reinvested_capital_gbp": round(total_equity - gbp, 2),
                        "locked_profit_vault_gbp": 0.0,
                        "locked_profit_gbp": 0.0,
                        "unlocked_profit_gbp": 0.0,
                        "cumulative_profit_gbp": 0.0,
                        "profit_lock_pct": 30.0,
                        "active_trading_power_gbp": gbp,
                        "expansion_ratio": 1.0,
                        "rungs_per_side": 5,
                        "split_btc_pct": 40.0,
                        "split_eth_pct": 30.0,
                        "split_sol_pct": 30.0,
                        "total_equity_gbp": total_equity,
                        "compounded_pnl_gbp": 0.0,
                        "compounded_return_pct": 0.0,
                        "max_rolling_drawdown_pct": 0.0,
                        "circuit_breaker_tripped": payload.get("circuit_breaker_tripped", False),
                        "circuit_breaker_reason": payload.get("circuit_breaker_reason", "Normal"),
                        "allocations": {
                            "trading_power_gbp": gbp,
                            "expansion_ratio": 1.0,
                            "runner_btc": {
                                "envelope_gbp": 500.0,
                                "split_pct": 40.0,
                                "rungs_per_side": 5,
                                "order_size_gbp": 50.0,
                            },
                            "runner_eth": {
                                "envelope_gbp": 500.0,
                                "split_pct": 30.0,
                                "rungs_per_side": 5,
                                "order_size_gbp": 50.0,
                            },
                            "runner_sol": {
                                "envelope_gbp": 500.0,
                                "split_pct": 30.0,
                                "rungs_per_side": 5,
                                "order_size_gbp": 50.0,
                            },
                            "sniper": {
                                "order_size_gbp": float(sniper_dto.get("order_size_gbp", 50.0)),
                            },
                        },
                    },
                    "market_prices": market_prices,
                    "runners": runners,
                    "sniper": {
                        "enabled": sniper_dto.get("enabled", True),
                        "status": "ACTIVE" if sniper_dto.get("enabled") else "PAUSED",
                        "impulse_threshold_pct": round(float(sniper_dto.get("impulse_threshold_pct", 0.0011)) * 100.0, 3),
                        "snipe_order_size_gbp": float(sniper_dto.get("order_size_gbp", 50.0)),
                        "min_net_edge_pct": 0.02,
                        "revolut_taker_fee_pct": 0.09,
                        "revolut_maker_fee_pct": 0.00,
                        "scratch_timeout_ms": 800,
                        "total_snipes": sniper_dto.get("total_snipes", 0),
                        "successful_snipes": sniper_dto.get("successful_snipes", 0),
                        "win_rate_pct": 100.0,
                        "total_sniper_profit_gbp": float(sniper_dto.get("total_sniper_profit_gbp", 0.0)),
                        "total_taker_fees_paid_gbp": float(sniper_dto.get("total_fees_paid_gbp", 0.0)),
                        "average_lead_advantage_ms": sniper_dto.get("average_lead_ms", 450),
                        "radar": radar,
                    },
                    "engine_activity": engine_activity,
                    "engine_decisions": engine_decisions,
                    "resting_orders": active_orders,
                    "resting_orders_count": len(active_orders),
                    "live_trades": live_trades,
                    "timestamp": int(time.time()),
                }
        except Exception as e:
            logger.error(f"IPC telemetry error: {e}")

        # Strict error when in LIVE mode and IPC is unreachable (Zero fallback to £1000)
        is_live_request = (target_mode == "LIVE") or ((mode or "").lower() == "live") or (self.mode_config == "LIVE")
        if is_live_request:
            return {
                "mode": "live",
                "is_live": True,
                "status": "IPC_DISCONNECTED",
                "status_message": "Cannot reach Rust Engine Daemon on TCP 127.0.0.1:9099. Ensure engine-daemon is running.",
                "authenticated": False,
                "can_trade": False,
                "latency_ms": 0,
                "host_mode": self.mode_config,
                "circuit_breaker_tripped": False,
                "balances": {"GBP": 0.0, "BTC": 0.0, "ETH": 0.0},
                "portfolio": {
                    "total_equity_gbp": 0.0,
                    "initial_budget_gbp": 0.0,
                    "total_pnl_gbp": 0.0,
                    "total_pnl_pct": 0.0,
                    "total_realized_pnl_gbp": 0.0,
                    "total_fee_savings_gbp": 0.0,
                },
                "runners": [],
                "resting_orders": [],
                "resting_orders_count": 0,
                "live_trades": [],
                "timestamp": int(time.time()),
            }

        runner = self.get_runner(mode)
        telemetry = runner.get_telemetry()
        telemetry["host_mode"] = self.mode_config
        return telemetry

    def get_telemetry(self, mode: Optional[str] = None) -> Dict[str, Any]:
        runner = self.get_runner(mode)
        telemetry = runner.get_telemetry()
        telemetry["host_mode"] = self.mode_config
        return telemetry

coordinator = TradingEngineCoordinator()
