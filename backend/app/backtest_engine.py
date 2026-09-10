import os
import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional
import httpx
from pydantic import BaseModel

logger = logging.getLogger("backtest_engine")

CACHE_DIR = Path(__file__).resolve().parent.parent / "data_cache"
CACHE_DIR.mkdir(exist_ok=True)

class BacktestRequest(BaseModel):
    symbol: str = "BTC/GBP"                     # "BTC/GBP" or "ETH/GBP"
    timeframe_days: int = 365                  # 7, 30, 90, 180, 365
    initial_budget_gbp: float = 1000.0         # Total starting capital
    cash_crypto_split: float = 50.0            # 50.0 = 50% Cash / 50% Crypto
    step_pct: float = 0.0040                   # 0.40% geometric step
    rungs_per_side: int = 5                    # 5 rungs on each side
    rebalance_threshold_pct: float = 0.012     # 1.20% rebalance drift
    basis_spread_pct: float = 0.0010           # 0.10% pricing basis discrepancy
    lag_filter_drop_threshold_pct: float = 0.0060 # 0.60% drop threshold for adverse selection
    # Configurable Profit Removal & Vaulting
    enable_profit_sweep: bool = True
    profit_sweep_mode: str = "threshold"       # "threshold" or "ratchet"
    profit_sweep_threshold_pct: float = 0.10   # e.g. 0.10 = 10% milestone sweep (£100 on £1000)
    profit_sweep_ratchet_pct: float = 0.50     # e.g. 50% of every trade swept to vault

class EquityPoint(BaseModel):
    timestamp: int
    day: int
    date: str
    price: float
    equity_protected: float
    equity_unprotected: float
    benchmark_hodl: float
    vault_cash_protected: float
    total_wealth_protected: float
    cash_protected: float
    crypto_protected: float

class SimulatedTrade(BaseModel):
    id: int
    day: int
    date: str
    action: str                                # "BUY", "SELL", "VAULT_SWEEP", "LAG_CANCEL"
    price: float
    qty: float
    profit: float
    note: str

class BacktestSummary(BaseModel):
    symbol: str
    timeframe_days: int
    initial_budget: float
    # Protected execution metrics
    final_equity_protected: float
    vaulted_profit_gbp: float
    total_realized_wealth_gbp: float
    return_protected_pct: float
    # Unprotected execution metrics
    final_equity_unprotected: float
    return_unprotected_pct: float
    # Benchmark
    benchmark_final_equity: float
    benchmark_return_pct: float
    grid_alpha_vs_hodl_pct: float
    # Risk & Treasury metrics
    max_drawdown_protected_pct: float
    max_drawdown_unprotected_pct: float
    total_trades_protected: int
    total_trades_unprotected: int
    realized_grid_profit_gbp: float
    toxic_fills_avoided: int
    capital_saved_by_filter_gbp: float
    fee_savings_gbp: float
    # House Money / Principal Recovery
    house_money_achieved: bool
    house_money_day: Optional[int]
    principal_payback_pct: float
    equity_curve: List[EquityPoint]
    trades: List[SimulatedTrade]

class HistoricalBacktestEngine:
    KRAKEN_OHLC_URL = "https://api.kraken.com/0/public/OHLC"

    @classmethod
    async def fetch_historical_candles(cls, symbol: str, timeframe_days: int) -> List[Dict[str, float]]:
        pair = "XBTGBP" if "BTC" in symbol.upper() else "ETHGBP"
        interval = 1440 if timeframe_days > 30 else 60

        cache_file = CACHE_DIR / f"{pair}_{interval}.json"
        candles_raw = None

        if cache_file.exists():
            try:
                with open(cache_file, "r") as f:
                    cache_data = json.load(f)
                    cache_age = datetime.now(timezone.utc).timestamp() - cache_data.get("cached_at", 0)
                    if cache_age < 3600 * 6:
                        candles_raw = cache_data.get("candles")
            except Exception as e:
                logger.warning(f"Cache read error: {e}")

        if not candles_raw:
            async with httpx.AsyncClient(timeout=15.0) as client:
                url = f"{cls.KRAKEN_OHLC_URL}?pair={pair}&interval={interval}"
                resp = await client.get(url)
                data = resp.json()
                result = data.get("result", {})
                for key in result:
                    if key != "last":
                        candles_raw = result[key]
                        break

            if candles_raw:
                try:
                    with open(cache_file, "w") as f:
                        json.dump({"cached_at": datetime.now(timezone.utc).timestamp(), "candles": candles_raw}, f)
                except Exception as e:
                    logger.warning(f"Failed to cache data: {e}")

        if not candles_raw:
            raise ValueError(f"Could not retrieve historical data for {symbol}")

        required_candles = timeframe_days if interval == 1440 else timeframe_days * 24
        candles_subset = candles_raw[-required_candles:] if len(candles_raw) >= required_candles else candles_raw

        parsed = []
        for c in candles_subset:
            parsed.append({
                "time": int(c[0]),
                "open": float(c[1]),
                "high": float(c[2]),
                "low": float(c[3]),
                "close": float(c[4]),
                "volume": float(c[6]),
            })

        return parsed

    @classmethod
    def run_simulation(cls, req: BacktestRequest, candles: List[Dict[str, float]]) -> BacktestSummary:
        if not candles:
            raise ValueError("No candle data available for backtest")

        p0 = candles[0]["open"]
        budget = req.initial_budget_gbp

        # Run Protected Simulation (Lag Filter Active + Configurable Profit Sweep)
        prot_res = cls._simulate_single_mode(req, candles, p0, budget, lag_filter_active=True, enable_sweep=req.enable_profit_sweep)

        # Run Unprotected Simulation (Lag Filter Inactive, Toxic Fills Incurred)
        unprot_res = cls._simulate_single_mode(req, candles, p0, budget, lag_filter_active=False, enable_sweep=False)

        # Build equity curve points
        combined_curve = []
        for i in range(len(candles)):
            p_pt = prot_res["curve"][i]
            u_pt = unprot_res["curve"][i]
            c = candles[i]
            dt = datetime.fromtimestamp(c["time"], tz=timezone.utc).strftime("%Y-%m-%d")
            combined_curve.append(EquityPoint(
                timestamp=c["time"],
                day=i + 1,
                date=dt,
                price=c["close"],
                equity_protected=round(p_pt["active_equity"], 2),
                equity_unprotected=round(u_pt["active_equity"], 2),
                benchmark_hodl=round(p_pt["benchmark"], 2),
                vault_cash_protected=round(p_pt["vault_cash"], 2),
                total_wealth_protected=round(p_pt["total_wealth"], 2),
                cash_protected=round(p_pt["cash"], 2),
                crypto_protected=round(p_pt["crypto"], 6),
            ))

        total_wealth_prot = prot_res["total_wealth"]
        ret_prot_pct = ((total_wealth_prot - budget) / budget) * 100.0
        ret_unprot_pct = ((unprot_res["total_wealth"] - budget) / budget) * 100.0
        bench_ret_pct = ((prot_res["benchmark_final"] - budget) / budget) * 100.0
        alpha_pct = ret_prot_pct - bench_ret_pct

        fee_savings = prot_res["total_turnover_gbp"] * 0.0040
        capital_saved = max(0.0, total_wealth_prot - unprot_res["total_wealth"])

        # House Money metrics
        vault_cash = prot_res["vault_cash"]
        payback_pct = min(100.0, (vault_cash / budget) * 100.0) if budget > 0 else 0.0
        house_money_achieved = vault_cash >= budget

        return BacktestSummary(
            symbol=req.symbol,
            timeframe_days=req.timeframe_days,
            initial_budget=budget,
            final_equity_protected=round(prot_res["active_equity"], 2),
            vaulted_profit_gbp=round(vault_cash, 2),
            total_realized_wealth_gbp=round(total_wealth_prot, 2),
            return_protected_pct=round(ret_prot_pct, 2),
            final_equity_unprotected=round(unprot_res["total_wealth"], 2),
            return_unprotected_pct=round(ret_unprot_pct, 2),
            benchmark_final_equity=round(prot_res["benchmark_final"], 2),
            benchmark_return_pct=round(bench_ret_pct, 2),
            grid_alpha_vs_hodl_pct=round(alpha_pct, 2),
            max_drawdown_protected_pct=round(prot_res["max_drawdown"] * 100.0, 2),
            max_drawdown_unprotected_pct=round(unprot_res["max_drawdown"] * 100.0, 2),
            total_trades_protected=prot_res["total_trades"],
            total_trades_unprotected=unprot_res["total_trades"],
            realized_grid_profit_gbp=round(prot_res["cumulative_realized_profit"], 2),
            toxic_fills_avoided=unprot_res["toxic_fills_count"],
            capital_saved_by_filter_gbp=round(capital_saved, 2),
            fee_savings_gbp=round(fee_savings, 2),
            house_money_achieved=house_money_achieved,
            house_money_day=prot_res["house_money_day"],
            principal_payback_pct=round(payback_pct, 1),
            equity_curve=combined_curve,
            trades=prot_res["trades"],
        )

    @classmethod
    def _simulate_single_mode(
        cls,
        req: BacktestRequest,
        candles: List[Dict[str, float]],
        p0: float,
        budget: float,
        lag_filter_active: bool,
        enable_sweep: bool,
    ) -> Dict[str, Any]:
        cash_ratio = req.cash_crypto_split / 100.0
        cash = budget * cash_ratio
        crypto = (budget * (1.0 - cash_ratio)) / p0

        center = p0
        retained_profit = 0.0
        cumulative_realized_profit = 0.0
        vault_cash = 0.0
        unharvested_milestone_profit = 0.0
        house_money_day = None

        total_trades = 0
        total_turnover = 0.0
        toxic_fills_count = 0
        toxic_damage_accumulated = 0.0

        order_size_gbp = budget / (req.rungs_per_side * 2.0)
        curve = []
        trades: List[SimulatedTrade] = []
        trade_id = 1
        peak_total_wealth = budget
        max_drawdown = 0.0

        is_daily = req.timeframe_days > 30
        sweep_threshold_gbp = budget * req.profit_sweep_threshold_pct

        for idx, c in enumerate(candles):
            high = c["high"]
            low = c["low"]
            close = c["close"]
            dt = datetime.fromtimestamp(c["time"], tz=timezone.utc).strftime("%Y-%m-%d")

            dump_velocity = (high - low) / high if high > 0 else 0.0
            is_dump = dump_velocity >= req.lag_filter_drop_threshold_pct

            candle_range_pct = (high - low) / low if low > 0 else 0.0
            steps_traversed = candle_range_pct / req.step_pct if req.step_pct > 0 else 1.0

            wave_multiplier = 1.5 if is_daily else 1.0
            raw_waves = int(steps_traversed * wave_multiplier)
            waves = max(1, raw_waves) if steps_traversed >= 1.0 else (1 if steps_traversed >= 0.5 else 0)

            if is_dump:
                if lag_filter_active:
                    # Log Lag Filter shield action
                    trades.append(SimulatedTrade(
                        id=trade_id,
                        day=idx + 1,
                        date=dt,
                        action="LAG_CANCEL",
                        price=round(close, 2),
                        qty=0.0,
                        profit=0.0,
                        note=f"Kraken Lag Filter triggered (dump {dump_velocity * 100:.2f}%): Resting buy bids canceled ahead of toxic flow",
                    ))
                    trade_id += 1
                else:
                    toxic_fills_count += 1
                    damage = order_size_gbp * (dump_velocity * 0.4)
                    toxic_damage_accumulated += damage
                    trades.append(SimulatedTrade(
                        id=trade_id,
                        day=idx + 1,
                        date=dt,
                        action="TOXIC_FILL",
                        price=round(close, 2),
                        qty=round(order_size_gbp / close, 6) if close > 0 else 0.0,
                        profit=-round(damage, 2),
                        note=f"Adverse selection! Resting buy order hit into a {dump_velocity * 100:.2f}% plunge (-£{damage:.2f} slippage)",
                    ))
                    trade_id += 1

            if waves > 0:
                completed_cycles = max(1, waves // 2)
                cycle_profit = completed_cycles * (order_size_gbp * req.step_pct)
                cumulative_realized_profit += cycle_profit
                total_trades += waves
                total_turnover += waves * order_size_gbp

                buy_p = round(close * (1.0 - req.step_pct), 2)
                sell_p = round(close * (1.0 + req.step_pct), 2)
                qty_val = round(order_size_gbp / buy_p, 6) if buy_p > 0 else 0.0

                trades.append(SimulatedTrade(
                    id=trade_id,
                    day=idx + 1,
                    date=dt,
                    action="BUY",
                    price=buy_p,
                    qty=qty_val,
                    profit=0.0,
                    note=f"Placed & filled {completed_cycles} buy rungs on Revolut X (0.00% maker)",
                ))
                trade_id += 1

                trades.append(SimulatedTrade(
                    id=trade_id,
                    day=idx + 1,
                    date=dt,
                    action="SELL",
                    price=sell_p,
                    qty=qty_val,
                    profit=round(cycle_profit, 2),
                    note=f"Closed {completed_cycles} sell counter-rungs (+£{cycle_profit:.2f} profit)",
                ))
                trade_id += 1

                # Profit removal / Vaulting logic
                if enable_sweep:
                    if req.profit_sweep_mode == "threshold":
                        unharvested_milestone_profit += cycle_profit
                        if unharvested_milestone_profit >= sweep_threshold_gbp:
                            swept = unharvested_milestone_profit
                            vault_cash += swept
                            unharvested_milestone_profit = 0.0
                            if vault_cash >= budget and house_money_day is None:
                                house_money_day = idx + 1
                            trades.append(SimulatedTrade(
                                id=trade_id,
                                day=idx + 1,
                                date=dt,
                                action="VAULT_SWEEP",
                                price=round(close, 2),
                                qty=0.0,
                                profit=round(swept, 2),
                                note=f"🏦 Milestone Reached (+{req.profit_sweep_threshold_pct * 100:.0f}%): Swept £{swept:.2f} profit into Safe Vault Reserve!",
                            ))
                            trade_id += 1
                    elif req.profit_sweep_mode == "ratchet":
                        swept = cycle_profit * req.profit_sweep_ratchet_pct
                        vault_cash += swept
                        retained_profit += cycle_profit * (1.0 - req.profit_sweep_ratchet_pct)
                        if vault_cash >= budget and house_money_day is None:
                            house_money_day = idx + 1
                else:
                    retained_profit += cycle_profit

            if abs(close - center) / center >= req.rebalance_threshold_pct:
                center = close

            # Active working equity in the bot
            active_equity = cash + (crypto * close) + retained_profit + unharvested_milestone_profit - toxic_damage_accumulated
            # Total wealth = Active bot equity + Safe vaulted cash in bank
            total_wealth = active_equity + vault_cash

            if total_wealth > peak_total_wealth:
                peak_total_wealth = total_wealth
            dd = (peak_total_wealth - total_wealth) / peak_total_wealth if peak_total_wealth > 0 else 0.0
            if dd > max_drawdown:
                max_drawdown = dd

            benchmark = budget * (close / p0)

            curve.append({
                "active_equity": active_equity,
                "vault_cash": vault_cash,
                "total_wealth": total_wealth,
                "benchmark": benchmark,
                "cash": cash,
                "crypto": crypto,
            })

        final_active = curve[-1]["active_equity"] if curve else budget
        final_wealth = curve[-1]["total_wealth"] if curve else budget
        benchmark_final = curve[-1]["benchmark"] if curve else budget

        return {
            "active_equity": final_active,
            "vault_cash": vault_cash,
            "total_wealth": final_wealth,
            "benchmark_final": benchmark_final,
            "cumulative_realized_profit": cumulative_realized_profit,
            "total_trades": total_trades,
            "total_turnover_gbp": total_turnover,
            "toxic_fills_count": toxic_fills_count,
            "max_drawdown": max_drawdown,
            "house_money_day": house_money_day,
            "curve": curve,
            "trades": trades,
        }
