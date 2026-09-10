"use client";

import React, { useEffect, useState, useRef } from "react";
import Link from "next/link";
import {
  ShieldAlert,
  Activity,
  Play,
  Pause,
  RefreshCw,
  TrendingUp,
  AlertTriangle,
  Sliders,
  Wallet,
  Zap,
  BarChart3,
  CheckCircle2,
  ShieldCheck,
  Landmark,
  PiggyBank,
  Award,
  FastForward,
  RotateCcw,
  Radio,
  ArrowDownRight,
  ArrowUpRight,
  Clock,
  Terminal,
} from "lucide-react";

interface RunnerTelemetry {
  runner_id: string;
  symbol: string;
  current_price?: number;
  center_price: number | null;
  inventory_base: number;
  inventory_value_gbp?: number;
  realized_pnl: number;
  total_trades: number;
  active_orders_count: number;
  is_paused: boolean;
  step_pct?: number;
  rebalance_threshold_pct?: number;
}

interface RestingOrder {
  id: string;
  runner_id: string;
  symbol: string;
  side: "BUY" | "SELL";
  price: number;
  qty: number;
  value_gbp: number;
  created_at: string;
  rung_level: number;
  distance_pct: number;
}

interface LiveTradeEvent {
  id: number;
  timestamp: number;
  time_str: string;
  runner_id: string;
  symbol: string;
  action: string;
  price: number;
  qty: number;
  profit: number;
  note: string;
}

interface MarketPriceInfo {
  price: number;
  high24h: number;
  low24h: number;
  change24h: number;
  yesterday_close: number;
}

interface VenueRadarItem {
  symbol: string;
  kraken_price: number;
  revolut_best_bid: number;
  revolut_best_ask: number;
  revolut_spread_gbp: number;
  revolut_spread_pct: number;
  buy_opportunity_pct: number;
  sell_opportunity_pct: number;
  current_dislocation_pct: number;
  in_snipe_zone: boolean;
  direction: string;
  lead_advantage_ms: number;
}

interface SniperTelemetry {
  enabled: boolean;
  status: string;
  impulse_threshold_pct: number;
  snipe_order_size_gbp: number;
  min_net_edge_pct: number;
  revolut_taker_fee_pct: number;
  total_snipes: number;
  successful_snipes: number;
  win_rate_pct: number;
  total_sniper_profit_gbp: number;
  total_taker_fees_paid_gbp: number;
  average_lead_advantage_ms: number;
  radar: Record<string, VenueRadarItem>;
  recent_snipes: any[];
}

interface TelemetryPayload {
  circuit_breaker_tripped: boolean;
  circuit_breaker_reason?: string;
  balances: Record<string, number>;
  portfolio?: {
    total_equity_gbp: number;
    initial_budget_gbp: number;
    total_pnl_gbp: number;
    total_pnl_pct: number;
    total_realized_pnl_gbp: number;
    total_fee_savings_gbp: number;
  };
  market_prices?: Record<string, MarketPriceInfo>;
  runners: RunnerTelemetry[];
  sniper?: SniperTelemetry;
  resting_orders?: RestingOrder[];
  resting_orders_count: number;
  live_trades?: LiveTradeEvent[];
  timestamp?: number;
}

interface EquityPoint {
  timestamp: number;
  day: number;
  date: string;
  price: number;
  equity_protected: number;
  equity_unprotected: number;
  benchmark_hodl: number;
  vault_cash_protected: number;
  total_wealth_protected: number;
  cash_protected: number;
  crypto_protected: number;
}

interface SimulatedTrade {
  id: number;
  day: number;
  date: string;
  action: "BUY" | "SELL" | "VAULT_SWEEP" | "LAG_CANCEL" | "TOXIC_FILL";
  price: number;
  qty: number;
  profit: number;
  note: string;
}

interface BacktestSummary {
  symbol: string;
  timeframe_days: number;
  initial_budget: number;
  final_equity_protected: number;
  vaulted_profit_gbp: number;
  total_realized_wealth_gbp: number;
  return_protected_pct: number;
  final_equity_unprotected: number;
  return_unprotected_pct: number;
  benchmark_final_equity: number;
  benchmark_return_pct: number;
  grid_alpha_vs_hodl_pct: number;
  max_drawdown_protected_pct: number;
  max_drawdown_unprotected_pct: number;
  total_trades_protected: number;
  total_trades_unprotected: number;
  realized_grid_profit_gbp: number;
  toxic_fills_avoided: number;
  capital_saved_by_filter_gbp: number;
  fee_savings_gbp: number;
  house_money_achieved: boolean;
  house_money_day: number | null;
  principal_payback_pct: number;
  equity_curve: EquityPoint[];
  trades: SimulatedTrade[];
}

export default function Dashboard() {
  const [activeTab, setActiveTab] = useState<"live" | "backtest">("live");

  // Live Terminal State
  const [telemetry, setTelemetry] = useState<TelemetryPayload | null>(null);
  const [connected, setConnected] = useState<boolean>(false);
  const [killModalOpen, setKillModalOpen] = useState<boolean>(false);
  const [tuningParams, setTuningParams] = useState<Record<string, { step_pct: string; rebalance_pct: string }>>({
    runner_btc: { step_pct: "0.40", rebalance_pct: "1.20" },
    runner_eth: { step_pct: "0.40", rebalance_pct: "1.20" },
  });
  const [statusMessage, setStatusMessage] = useState<string>("");
  const [ordersFilter, setOrdersFilter] = useState<"ALL" | "BTC" | "ETH">("ALL");

  // Backtest Lab State
  const [btSymbol, setBtSymbol] = useState<string>("BTC/GBP");
  const [btTimeframe, setBtTimeframe] = useState<number>(365);
  const [btBudget, setBtBudget] = useState<number>(1000);
  const [btSplit, setBtSplit] = useState<number>(50);
  const [btStepPct, setBtStepPct] = useState<number>(0.40);
  const [btRungs, setBtRungs] = useState<number>(5);
  const [btLagFilterThreshold, setBtLagFilterThreshold] = useState<number>(0.60);
  const [btBasisSpread, setBtBasisSpread] = useState<number>(0.10);
  
  // Profit Removal State
  const [btEnableSweep, setBtEnableSweep] = useState<boolean>(true);
  const [btSweepMode, setBtSweepMode] = useState<"threshold" | "ratchet">("threshold");
  const [btSweepThreshold, setBtSweepThreshold] = useState<number>(10);
  const [btSweepRatchet, setBtSweepRatchet] = useState<number>(50);

  const [btLoading, setBtLoading] = useState<boolean>(false);
  const [btResult, setBtResult] = useState<BacktestSummary | null>(null);
  const [hoveredPoint, setHoveredPoint] = useState<EquityPoint | null>(null);

  // Live Playback Replay State
  const [playbackIndex, setPlaybackIndex] = useState<number>(0);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [playbackSpeedMs, setPlaybackSpeedMs] = useState<number>(250); // 250ms per day
  const playbackTimerRef = useRef<NodeJS.Timeout | null>(null);

  const wsRef = useRef<WebSocket | null>(null);

  // Stale Quote Sniper Parameters & Controls
  const [sniperParams, setSniperParams] = useState({
    impulse_threshold_pct: "0.18",
    snipe_order_size_gbp: "50",
    min_net_edge_pct: "0.05",
  });

  const fetchTelemetry = async () => {
    try {
      const res = await fetch("/api/proxy/telemetry");
      if (res.ok) {
        const data = await res.json();
        setTelemetry(data);
        setConnected(true);
      }
    } catch {
      setConnected(false);
    }
  };

  useEffect(() => {
    fetchTelemetry();

    const connectWs = () => {
      const directWs = process.env.NEXT_PUBLIC_ENGINE_WS_URL;
      const wsUrl = directWs || (
        typeof window !== "undefined" && window.location.hostname === "localhost"
          ? "ws://localhost:8000/api/ws/stream"
          : null
      );
      if (!wsUrl) return;
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => setConnected(true);
      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === "Telemetry" && msg.payload) {
            setTelemetry(msg.payload);
          } else if (msg.balances) {
            setTelemetry(msg);
          }
        } catch {}
      };
      ws.onerror = () => setConnected(false);
      ws.onclose = () => {
        setConnected(false);
        setTimeout(connectWs, 3000);
      };
    };

    connectWs();
    const pollInterval = setInterval(fetchTelemetry, 2500);

    return () => {
      clearInterval(pollInterval);
      if (wsRef.current) wsRef.current.close();
    };
  }, []);

  // Playback timer effect
  useEffect(() => {
    if (isPlaying && btResult?.equity_curve?.length) {
      playbackTimerRef.current = setInterval(() => {
        setPlaybackIndex((prev) => {
          if (prev >= btResult.equity_curve.length - 1) {
            setIsPlaying(false);
            return prev;
          }
          return prev + 1;
        });
      }, playbackSpeedMs);
    } else {
      if (playbackTimerRef.current) {
        clearInterval(playbackTimerRef.current);
      }
    }

    return () => {
      if (playbackTimerRef.current) {
        clearInterval(playbackTimerRef.current);
      }
    };
  }, [isPlaying, playbackSpeedMs, btResult]);

  const handleTogglePause = async (runnerId: string, currentlyPaused: boolean) => {
    const action = currentlyPaused ? "resume" : "pause";
    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/${action}`, {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage(`Successfully ${action}d ${runnerId}`);
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Failed to ${action} runner: ${err.message}`);
    }
  };

  const handleTune = async (runnerId: string) => {
    const params = tuningParams[runnerId];
    if (!params) return;

    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/tune`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          step_pct: parseFloat(params.step_pct) / 100,
          rebalance_threshold_pct: parseFloat(params.rebalance_pct) / 100,
        }),
      });
      if (res.ok) {
        setStatusMessage(`Updated tuning parameters for ${runnerId}`);
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Tuning error: ${err.message}`);
    }
  };

  const handleKillSwitch = async () => {
    try {
      const res = await fetch("/api/proxy/emergency/kill-switch", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: "Emergency Kill Switch from Next.js Dashboard" }),
      });
      if (res.ok) {
        setStatusMessage("🚨 Global Kill Switch Activated! All orders canceled.");
        setKillModalOpen(false);
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Kill switch failed: ${err.message}`);
    }
  };

  const handleResetCircuitBreaker = async () => {
    try {
      const res = await fetch("/api/proxy/circuit-breaker/reset", {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage("✅ Circuit breaker reset and grids redeployed successfully!");
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Reset failed: ${err.message}`);
    }
  };

  const handleToggleSniper = async () => {
    try {
      const res = await fetch("/api/proxy/sniper/toggle", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({}),
      });
      if (res.ok) {
        const data = await res.json();
        setStatusMessage(`⚡ Stale Quote Sniper is now ${data.payload?.status || "updated"}`);
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Failed to toggle sniper: ${err.message}`);
    }
  };

  const handleTuneSniper = async () => {
    try {
      const res = await fetch("/api/proxy/sniper/tune", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          impulse_threshold_pct: parseFloat(sniperParams.impulse_threshold_pct) / 100,
          snipe_order_size_gbp: parseFloat(sniperParams.snipe_order_size_gbp),
          min_net_edge_pct: parseFloat(sniperParams.min_net_edge_pct) / 100,
        }),
      });
      if (res.ok) {
        setStatusMessage("⚡ Stale Quote Sniper parameters tuned successfully!");
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Sniper tuning error: ${err.message}`);
    }
  };

  const runBacktestSimulation = async () => {
    setBtLoading(true);
    setIsPlaying(false);
    try {
      const res = await fetch("/api/proxy/backtest/run", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          symbol: btSymbol,
          timeframe_days: btTimeframe,
          initial_budget_gbp: btBudget,
          cash_crypto_split: btSplit,
          step_pct: btStepPct / 100,
          rungs_per_side: btRungs,
          rebalance_threshold_pct: 0.012,
          basis_spread_pct: btBasisSpread / 100,
          lag_filter_drop_threshold_pct: btLagFilterThreshold / 100,
          enable_profit_sweep: btEnableSweep,
          profit_sweep_mode: btSweepMode,
          profit_sweep_threshold_pct: btSweepThreshold / 100,
          profit_sweep_ratchet_pct: btSweepRatchet / 100,
        }),
      });
      if (res.ok) {
        const data: BacktestSummary = await res.json();
        setBtResult(data);
        setPlaybackIndex(data.equity_curve.length - 1); // default to end, but can replay
      } else {
        const err = await res.json();
        setStatusMessage(`Backtest error: ${err.detail || "Simulation failed"}`);
      }
    } catch (err: any) {
      setStatusMessage(`Backtest request failed: ${err.message}`);
    } finally {
      setBtLoading(false);
    }
  };

  const cbTripped = telemetry?.circuit_breaker_tripped ?? false;
  const balances = telemetry?.balances ?? { GBP: 1000, BTC: 0, ETH: 0 };
  const runners = telemetry?.runners ?? [];
  const restingCount = telemetry?.resting_orders_count ?? 0;

  // Active playback slice
  const currentPoint = btResult?.equity_curve?.[playbackIndex] ?? null;
  const activeTrades = btResult?.trades?.filter((t) => t.day <= (currentPoint?.day || 0)) ?? [];
  const recentTrades = activeTrades.slice(-8).reverse();

  // Render SVG chart with progressive playback
  const renderChart = (points: EquityPoint[], activeUpToIndex: number) => {
    if (!points || points.length === 0) return null;
    const width = 800;
    const height = 260;
    const padding = 35;

    const allEquities = points.flatMap((p) => [
      p.total_wealth_protected,
      p.equity_protected,
      p.vault_cash_protected,
      p.equity_unprotected,
      p.benchmark_hodl,
    ]);
    const minVal = Math.min(...allEquities) * 0.95;
    const maxVal = Math.max(...allEquities) * 1.05;
    const range = maxVal - minVal || 1;

    const getX = (i: number) => padding + (i / (points.length - 1)) * (width - 2 * padding);
    const getY = (val: number) => height - padding - ((val - minVal) / range) * (height - 2 * padding);

    // Visible slice up to active playback point
    const visiblePoints = points.slice(0, activeUpToIndex + 1);

    const makePath = (pts: EquityPoint[], accessor: (p: EquityPoint) => number) => {
      return pts.map((p, i) => `${i === 0 ? "M" : "L"} ${getX(i).toFixed(1)} ${getY(accessor(p)).toFixed(1)}`).join(" ");
    };

    const pathTotalWealth = makePath(visiblePoints, (p) => p.total_wealth_protected);
    const pathVault = makePath(visiblePoints, (p) => p.vault_cash_protected);
    const pathUnprotected = makePath(visiblePoints, (p) => p.equity_unprotected);
    const pathBenchmark = makePath(visiblePoints, (p) => p.benchmark_hodl);

    // Full subtle ghost background lines
    const ghostBenchmark = makePath(points, (p) => p.benchmark_hodl);

    const currentX = getX(activeUpToIndex);
    const currentY = getY(visiblePoints[visiblePoints.length - 1]?.total_wealth_protected || minVal);

    return (
      <div className="relative w-full overflow-hidden">
        <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-auto">
          {/* Grid lines */}
          <line x1={padding} y1={getY(minVal)} x2={width - padding} y2={getY(minVal)} stroke="#1e293b" strokeDasharray="3 3" />
          <line x1={padding} y1={getY((minVal + maxVal) / 2)} x2={width - padding} y2={getY((minVal + maxVal) / 2)} stroke="#1e293b" strokeDasharray="3 3" />
          <line x1={padding} y1={getY(maxVal)} x2={width - padding} y2={getY(maxVal)} stroke="#1e293b" strokeDasharray="3 3" />

          {/* Y Axis labels */}
          <text x={padding - 5} y={getY(maxVal) + 4} fill="#64748b" fontSize="10" textAnchor="end" fontFamily="monospace">
            £{maxVal.toFixed(0)}
          </text>
          <text x={padding - 5} y={getY((minVal + maxVal) / 2) + 4} fill="#64748b" fontSize="10" textAnchor="end" fontFamily="monospace">
            £{((minVal + maxVal) / 2).toFixed(0)}
          </text>
          <text x={padding - 5} y={getY(minVal) + 4} fill="#64748b" fontSize="10" textAnchor="end" fontFamily="monospace">
            £{minVal.toFixed(0)}
          </text>

          {/* Ghost Benchmark Path */}
          <path d={ghostBenchmark} fill="none" stroke="#334155" strokeWidth="1" strokeDasharray="2 2" opacity="0.4" />

          {/* Progressive Active Paths */}
          {/* HODL Benchmark */}
          <path d={pathBenchmark} fill="none" stroke="#38bdf8" strokeWidth="1.8" strokeDasharray="4 2" opacity="0.8" />
          {/* Unprotected Toxic Grid */}
          <path d={pathUnprotected} fill="none" stroke="#f43f5e" strokeWidth="1.8" opacity="0.85" />
          {/* Vault Cash (Gold) */}
          <path d={pathVault} fill="none" stroke="#f59e0b" strokeWidth="2.2" strokeDasharray="2 2" />
          {/* Total Realized Wealth (Protected Grid + Vault) */}
          <path d={pathTotalWealth} fill="none" stroke="#10b981" strokeWidth="2.8" />

          {/* Playhead vertical line */}
          <line x1={currentX} y1={padding} x2={currentX} y2={height - padding} stroke="#38bdf8" strokeWidth="1.5" strokeDasharray="2 2" opacity="0.7" />

          {/* Current animated playhead dot */}
          <circle cx={currentX} cy={currentY} r="5" fill="#10b981" className="animate-pulse" />
        </svg>

        {/* Legend */}
        <div className="flex flex-wrap items-center justify-center gap-6 text-xs font-mono pt-2 border-t border-slate-800/80">
          <div className="flex items-center gap-2">
            <span className="h-2.5 w-5 bg-emerald-500 rounded-sm" />
            <span className="text-emerald-300 font-semibold">Total Wealth (Grid + Vault)</span>
          </div>
          <div className="flex items-center gap-2">
            <span className="h-2.5 w-5 bg-amber-500 rounded-sm border-t border-dashed" />
            <span className="text-amber-300 font-semibold">Safe Banked Cash (Vault)</span>
          </div>
          <div className="flex items-center gap-2">
            <span className="h-2.5 w-5 bg-rose-500 rounded-sm" />
            <span className="text-rose-300 font-semibold">Unprotected (Toxic Fills)</span>
          </div>
          <div className="flex items-center gap-2">
            <span className="h-2.5 w-5 bg-cyan-400 rounded-sm border-t border-dashed" />
            <span className="text-cyan-300 font-semibold">HODL Benchmark</span>
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="max-w-7xl mx-auto px-4 py-6 space-y-6 font-mono">
      {/* Top Header */}
      <header className="flex flex-col md:flex-row md:items-center md:justify-between pb-4 border-b border-slate-800 gap-4">
        <div>
          <div className="flex items-center gap-3">
            <div className="h-3 w-3 rounded-full bg-cyan-400 animate-ping" />
            <h1 className="text-2xl font-bold tracking-tight text-white flex items-center gap-2">
              <Zap className="h-6 w-6 text-cyan-400" />
              Testing & Simulation Laboratory
            </h1>
            <span className="text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded bg-cyan-500/20 text-cyan-300 border border-cyan-500/40">
              Sandbox Frontend
            </span>
          </div>
          <p className="text-sm text-slate-400 mt-1">
            Asymmetric UK Spot Prototype: <span className="text-cyan-300">Revolut X (0.00% Maker)</span> +{" "}
            <span className="text-purple-300">Kraken Pro WS v2 Market Feeds</span>
          </p>
        </div>

        <div className="flex items-center gap-3 flex-wrap">
          {/* Navigation Tabs */}
          <div className="bg-slate-900 border border-slate-800 p-1 rounded-lg flex items-center gap-1 text-xs">
            <button
              onClick={() => setActiveTab("live")}
              className={`px-3 py-1.5 rounded transition flex items-center gap-1.5 ${
                activeTab === "live" ? "bg-cyan-500 text-black font-bold shadow" : "text-slate-400 hover:text-white"
              }`}
            >
              <Radio className="h-3.5 w-3.5" />
              Live Paper Execution
            </button>
            <button
              onClick={() => setActiveTab("backtest")}
              className={`px-3 py-1.5 rounded transition flex items-center gap-1.5 ${
                activeTab === "backtest" ? "bg-cyan-500 text-black font-bold shadow" : "text-slate-400 hover:text-white"
              }`}
            >
              <BarChart3 className="h-3.5 w-3.5" />
              Paper & Playback Lab
            </button>
          </div>

          {/* Circuit Breaker */}
          <div
            className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-semibold ${
              cbTripped
                ? "bg-rose-950 text-rose-300 border border-rose-600 animate-pulse"
                : "bg-emerald-950 text-emerald-300 border border-emerald-700"
            }`}
          >
            <ShieldAlert className="h-4 w-4" />
            <span>BREAKER: {cbTripped ? "TRIPPED" : "NORMAL"}</span>
          </div>

          {/* Production Terminal Link */}
          <Link
            href="/dashboard"
            className="bg-cyan-500 hover:bg-cyan-400 text-black font-extrabold px-3.5 py-1.5 rounded-lg text-xs flex items-center gap-1.5 shadow-lg shadow-cyan-950/50 transition active:scale-95"
          >
            <Terminal className="h-3.5 w-3.5" />
            Go to Production Dashboard ➔
          </Link>

          {/* Kill Switch */}
          <button
            onClick={() => setKillModalOpen(true)}
            className="bg-rose-600 hover:bg-rose-500 text-white font-bold px-3 py-1.5 rounded-lg text-xs flex items-center gap-1.5 shadow-lg shadow-rose-950/50 transition active:scale-95"
          >
            <AlertTriangle className="h-4 w-4" />
            KILL SWITCH
          </button>
        </div>
      </header>

      {statusMessage && (
        <div className="bg-slate-900 border border-cyan-500/30 text-cyan-300 px-4 py-2.5 rounded-lg text-sm flex items-center justify-between">
          <span>{statusMessage}</span>
          <button onClick={() => setStatusMessage("")} className="text-slate-400 hover:text-white text-xs">
            Dismiss
          </button>
        </div>
      )}

      {/* ========================================================================= */}
      {/* TAB 1: LIVE EXECUTION TERMINAL (TESTING & SIMULATION SANDBOX) */}
      {/* ========================================================================= */}
      {activeTab === "live" && (
        <div className="space-y-6">
          {/* Circuit Breaker Warning / Reset Banner if Tripped */}
          {cbTripped && (
            <div className="bg-rose-950/80 border border-rose-600/80 p-4 rounded-xl flex items-center justify-between">
              <div className="flex items-center gap-3">
                <AlertTriangle className="h-6 w-6 text-rose-400 animate-bounce" />
                <div>
                  <div className="text-sm font-bold text-white">Circuit Breaker Tripped / Orders Halted</div>
                  <div className="text-xs text-rose-300 mt-0.5">
                    {telemetry?.circuit_breaker_reason || "Adverse volatility threshold reached or manual kill-switch triggered."}
                  </div>
                </div>
              </div>
              <button
                onClick={handleResetCircuitBreaker}
                className="bg-emerald-600 hover:bg-emerald-500 text-white font-bold text-xs px-4 py-2 rounded-lg flex items-center gap-1.5 shadow"
              >
                <RefreshCw className="h-3.5 w-3.5" />
                Reset & Redeploy Grids
              </button>
            </div>
          )}

          {/* Live Market Bar (Kraken Real-Time Price Tickers) */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {["BTC/GBP", "ETH/GBP"].map((sym) => {
              const mkt = telemetry?.market_prices?.[sym];
              const runner = runners.find((r) => r.symbol === sym);
              const price = mkt?.price ?? (sym === "BTC/GBP" ? 56800 : 1820);
              const change = mkt?.change24h ?? -0.35;
              const high = mkt?.high24h ?? (price * 1.02);
              const low = mkt?.low24h ?? (price * 0.98);
              const center = runner?.center_price ?? price;
              const isPositive = change >= 0;

              return (
                <div key={sym} className="bg-[#111622] border border-slate-800 rounded-xl p-4 flex flex-col justify-between">
                  <div className="flex items-center justify-between border-b border-slate-800/80 pb-2.5">
                    <div className="flex items-center gap-2">
                      <span className="h-2.5 w-2.5 rounded-full bg-emerald-400 animate-ping" />
                      <span className="font-bold text-white text-base tracking-wide">{sym}</span>
                      <span className="text-[10px] px-2 py-0.5 rounded bg-slate-800 text-cyan-300 font-bold border border-slate-700">
                        Kraken Pro WS v2
                      </span>
                    </div>
                    <div className={`text-xs font-bold px-2 py-0.5 rounded flex items-center gap-1 ${isPositive ? "bg-emerald-950 text-emerald-400 border border-emerald-800" : "bg-rose-950 text-rose-400 border border-rose-800"}`}>
                      {isPositive ? <ArrowUpRight className="h-3 w-3" /> : <ArrowDownRight className="h-3 w-3" />}
                      {change >= 0 ? `+${change.toFixed(2)}%` : `${change.toFixed(2)}%`} (24h)
                    </div>
                  </div>

                  <div className="grid grid-cols-3 gap-3 pt-3">
                    <div>
                      <div className="text-[10px] text-slate-400">LIVE MARKET PRICE</div>
                      <div className="text-xl font-bold text-white mt-0.5">
                        £{price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                      </div>
                    </div>
                    <div>
                      <div className="text-[10px] text-slate-400">GRID CENTER</div>
                      <div className="text-sm font-semibold text-cyan-300 mt-1">
                        £{center.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                      </div>
                    </div>
                    <div>
                      <div className="text-[10px] text-slate-400">24H RANGE (H / L)</div>
                      <div className="text-xs text-slate-300 mt-1">
                        £{high.toFixed(0)} / £{low.toFixed(0)}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>

          {/* Portfolio Metric Cards */}
          <div className="grid grid-cols-2 md:grid-cols-6 gap-3">
            {/* Total Paper Equity */}
            <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
              <div className="text-[10px] text-slate-400 flex items-center justify-between">
                <span>TOTAL EQUITY</span>
                <Wallet className="h-3.5 w-3.5 text-cyan-400" />
              </div>
              <div className="text-lg font-bold text-white mt-1">
                £{telemetry?.portfolio?.total_equity_gbp ? telemetry.portfolio.total_equity_gbp.toFixed(2) : (balances.GBP !== undefined ? Number(balances.GBP).toFixed(2) : "1,000.00")}
              </div>
              <div className={`text-[11px] font-semibold mt-0.5 ${(telemetry?.portfolio?.total_pnl_gbp || 0) >= 0 ? "text-emerald-400" : "text-rose-400"}`}>
                {(telemetry?.portfolio?.total_pnl_gbp || 0) >= 0 ? "+" : ""}
                £{(telemetry?.portfolio?.total_pnl_gbp || 0).toFixed(2)} ({(telemetry?.portfolio?.total_pnl_pct || 0).toFixed(2)}%)
              </div>
            </div>

            {/* GBP Cash */}
            <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
              <div className="text-[10px] text-slate-400 flex items-center justify-between">
                <span>GBP CASH</span>
                <Landmark className="h-3.5 w-3.5 text-emerald-400" />
              </div>
              <div className="text-lg font-bold text-white mt-1">
                £{balances.GBP !== undefined ? Number(balances.GBP).toFixed(2) : "500.00"}
              </div>
              <div className="text-[10px] text-slate-500 mt-0.5">Available Margin</div>
            </div>

            {/* BTC Inventory */}
            <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
              <div className="text-[10px] text-slate-400 flex items-center justify-between">
                <span>BTC INVENTORY</span>
                <TrendingUp className="h-3.5 w-3.5 text-amber-400" />
              </div>
              <div className="text-lg font-bold text-amber-300 mt-1">
                {balances.BTC !== undefined ? Number(balances.BTC).toFixed(6) : "0.000000"}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                ≈ £{((balances.BTC || 0) * (telemetry?.market_prices?.["BTC/GBP"]?.price || 56800)).toFixed(2)}
              </div>
            </div>

            {/* ETH Inventory */}
            <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
              <div className="text-[10px] text-slate-400 flex items-center justify-between">
                <span>ETH INVENTORY</span>
                <TrendingUp className="h-3.5 w-3.5 text-indigo-400" />
              </div>
              <div className="text-lg font-bold text-indigo-300 mt-1">
                {balances.ETH !== undefined ? Number(balances.ETH).toFixed(5) : "0.00000"}
              </div>
              <div className="text-[10px] text-slate-400 mt-0.5">
                ≈ £{((balances.ETH || 0) * (telemetry?.market_prices?.["ETH/GBP"]?.price || 1820)).toFixed(2)}
              </div>
            </div>

            {/* Realized Grid Profit */}
            <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
              <div className="text-[10px] text-slate-400 flex items-center justify-between">
                <span>REALIZED PNL</span>
                <PiggyBank className="h-3.5 w-3.5 text-emerald-400" />
              </div>
              <div className="text-lg font-bold text-emerald-400 mt-1">
                +£{(telemetry?.portfolio?.total_realized_pnl_gbp || 0).toFixed(2)}
              </div>
              <div className="text-[10px] text-slate-500 mt-0.5">Closed Cycles</div>
            </div>

            {/* 0.00% Maker Fee Savings */}
            <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
              <div className="text-[10px] text-slate-400 flex items-center justify-between">
                <span>FEE SAVINGS</span>
                <ShieldCheck className="h-3.5 w-3.5 text-cyan-400" />
              </div>
              <div className="text-lg font-bold text-cyan-300 mt-1">
                £{(telemetry?.portfolio?.total_fee_savings_gbp || 0).toFixed(2)}
              </div>
              <div className="text-[10px] text-slate-500 mt-0.5">0.00% Revolut X</div>
            </div>
          </div>

          {/* Active Strategy Runners */}
          <div>
            <h2 className="text-base font-bold text-white mb-3 flex items-center gap-2">
              <Sliders className="h-5 w-5 text-cyan-400" />
              Active Geometric Grid Runners
            </h2>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
              {runners.map((runner) => {
                const isPaused = runner.is_paused;
                const params = tuningParams[runner.runner_id] || { step_pct: "0.40", rebalance_pct: "2.00" };

                return (
                  <div
                    key={runner.runner_id}
                    className="bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-4 hover:border-slate-700 transition"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="font-bold text-white text-base">{runner.symbol}</span>
                        <span className="text-xs px-2 py-0.5 rounded bg-slate-800 text-cyan-300 font-mono">
                          Revolut X (0.00% maker)
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        <span
                          className={`text-xs px-2.5 py-1 rounded font-bold ${
                            isPaused
                              ? "bg-amber-950 text-amber-400 border border-amber-800"
                              : "bg-emerald-950 text-emerald-400 border border-emerald-800"
                          }`}
                        >
                          {isPaused ? "PAUSED" : "ACTIVE"}
                        </span>
                        <button
                          onClick={() => handleTogglePause(runner.runner_id, isPaused)}
                          className={`p-1.5 rounded-lg border text-xs flex items-center gap-1 transition ${
                            isPaused
                              ? "bg-emerald-900/50 border-emerald-700 text-emerald-200 hover:bg-emerald-800/60"
                              : "bg-amber-900/50 border-amber-700 text-amber-200 hover:bg-amber-800/60"
                          }`}
                        >
                          {isPaused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
                        </button>
                      </div>
                    </div>

                    <div className="grid grid-cols-4 gap-2 bg-[#0a0e17] p-3 rounded-lg border border-slate-800/60 text-xs">
                      <div>
                        <div className="text-slate-500">RESTING ORDERS</div>
                        <div className="text-white font-bold mt-0.5">{runner.active_orders_count}</div>
                      </div>
                      <div>
                        <div className="text-slate-500">TOTAL TRADES</div>
                        <div className="text-cyan-400 font-bold mt-0.5">{runner.total_trades}</div>
                      </div>
                      <div>
                        <div className="text-slate-500">INVENTORY</div>
                        <div className="text-white font-bold mt-0.5">{Number(runner.inventory_base).toFixed(5)}</div>
                      </div>
                      <div>
                        <div className="text-slate-500">REALIZED PNL</div>
                        <div className={`font-bold mt-0.5 ${runner.realized_pnl >= 0 ? "text-emerald-400" : "text-rose-400"}`}>
                          +£{Number(runner.realized_pnl).toFixed(2)}
                        </div>
                      </div>
                    </div>

                    <div className="space-y-3 pt-2 border-t border-slate-800">
                      <div className="text-xs font-semibold text-slate-300">Live Parameter Injection:</div>
                      <div className="grid grid-cols-2 gap-3">
                        <div>
                          <label className="text-[11px] text-slate-400 block mb-1">Step Spacing (%)</label>
                          <input
                            type="number"
                            step="0.05"
                            value={params.step_pct}
                            onChange={(e) =>
                              setTuningParams({
                                ...tuningParams,
                                [runner.runner_id]: { ...params, step_pct: e.target.value },
                              })
                            }
                            className="w-full bg-slate-900 border border-slate-700 px-2.5 py-1.5 rounded text-xs text-white focus:border-cyan-500 outline-none"
                          />
                        </div>
                        <div>
                          <label className="text-[11px] text-slate-400 block mb-1">Rebalance Drift (%)</label>
                          <input
                            type="number"
                            step="0.10"
                            value={params.rebalance_pct}
                            onChange={(e) =>
                              setTuningParams({
                                ...tuningParams,
                                [runner.runner_id]: { ...params, rebalance_pct: e.target.value },
                              })
                            }
                            className="w-full bg-slate-900 border border-slate-700 px-2.5 py-1.5 rounded text-xs text-white focus:border-cyan-500 outline-none"
                          />
                        </div>
                      </div>

                      <button
                        onClick={() => handleTune(runner.runner_id)}
                        className="w-full bg-slate-800 hover:bg-slate-700 text-cyan-300 border border-cyan-500/30 text-xs py-1.5 rounded transition flex items-center justify-center gap-1.5"
                      >
                        <RefreshCw className="h-3.5 w-3.5" />
                        Apply Dynamic Tuning
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {/* ========================================================================= */}
          {/* STALE QUOTE SNIPER RADAR & CONTROL CENTER (KRAKEN -> REVOLUT X ARB) */}
          {/* ========================================================================= */}
          <div className="bg-[#111622] border border-amber-500/40 rounded-xl p-5 space-y-5 shadow-lg shadow-amber-950/20">
            {/* Header with Title and Arm/Disarm Toggle */}
            <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 border-b border-slate-800/80 pb-4">
              <div className="flex items-center gap-3">
                <div className="p-2.5 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-400">
                  <Zap className="h-5 w-5 animate-pulse" />
                </div>
                <div>
                  <div className="flex items-center gap-2.5 flex-wrap">
                    <h2 className="text-base font-bold text-white tracking-wide">
                      Stale Quote Sniper Runner
                    </h2>
                    <span
                      className={`text-[10px] font-extrabold px-2 py-0.5 rounded border ${
                        telemetry?.sniper?.status === "ARMED"
                          ? "bg-emerald-950 text-emerald-300 border-emerald-700 animate-pulse"
                          : telemetry?.sniper?.status === "HALTED"
                          ? "bg-rose-950 text-rose-300 border-rose-700"
                          : "bg-slate-900 text-slate-400 border-slate-700"
                      }`}
                    >
                      {telemetry?.sniper?.status || "DISARMED"}
                    </span>
                    <span className="text-[10px] px-2 py-0.5 rounded bg-amber-950/50 text-amber-300 border border-amber-800/60 font-mono">
                      Kraken ➔ Revolut X Lead-Lag
                    </span>
                  </div>
                  <p className="text-xs text-slate-400 mt-0.5">
                    Cross-venue latency arbitrage front-running stale limit quotes on Revolut X using Kraken Pro high-frequency ticks.
                  </p>
                </div>
              </div>

              {/* Arm / Disarm Toggle Button */}
              <div className="flex items-center gap-3">
                <button
                  onClick={handleToggleSniper}
                  className={`px-4 py-2 rounded-lg font-bold text-xs flex items-center gap-2 transition active:scale-95 shadow ${
                    telemetry?.sniper?.enabled
                      ? "bg-amber-500/20 text-amber-300 border border-amber-500/50 hover:bg-amber-500/30"
                      : "bg-emerald-600 text-white hover:bg-emerald-500"
                  }`}
                >
                  {telemetry?.sniper?.enabled ? (
                    <>
                      <Pause className="h-3.5 w-3.5" /> Disarm Sniper
                    </>
                  ) : (
                    <>
                      <Play className="h-3.5 w-3.5" /> Arm Sniper Engine
                    </>
                  )}
                </button>
              </div>
            </div>

            {/* Sniper 4-Metric Telemetry HUD */}
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              <div className="bg-[#0a0e17] border border-amber-500/30 p-3 rounded-lg flex flex-col justify-between">
                <span className="text-[10px] text-amber-400 font-bold uppercase tracking-wider flex items-center gap-1">
                  <Award className="h-3 w-3 text-amber-400" />
                  Realized Arb Profit
                </span>
                <div className="text-lg font-mono font-black text-amber-300 mt-1">
                  +£{(telemetry?.sniper?.total_sniper_profit_gbp ?? 0).toFixed(2)}
                </div>
                <span className="text-[10px] text-slate-500 mt-0.5">Pure risk-free micro-arb</span>
              </div>

              <div className="bg-[#0a0e17] border border-slate-800 p-3 rounded-lg flex flex-col justify-between">
                <span className="text-[10px] text-slate-400 font-bold uppercase tracking-wider flex items-center gap-1">
                  <Activity className="h-3 w-3 text-cyan-400" />
                  Snipes & Win Rate
                </span>
                <div className="text-lg font-mono font-black text-white mt-1">
                  {telemetry?.sniper?.total_snipes ?? 0}{" "}
                  <span className="text-xs font-normal text-emerald-400">
                    ({telemetry?.sniper?.win_rate_pct ?? 100}% win rate)
                  </span>
                </div>
                <span className="text-[10px] text-slate-500 mt-0.5">
                  {telemetry?.sniper?.successful_snipes ?? 0} profitable fills
                </span>
              </div>

              <div className="bg-[#0a0e17] border border-slate-800 p-3 rounded-lg flex flex-col justify-between">
                <span className="text-[10px] text-slate-400 font-bold uppercase tracking-wider flex items-center gap-1">
                  <FastForward className="h-3 w-3 text-emerald-400" />
                  Latency Advantage
                </span>
                <div className="text-lg font-mono font-black text-emerald-400 mt-1">
                  ~{telemetry?.sniper?.average_lead_advantage_ms ?? 420}ms
                </div>
                <span className="text-[10px] text-slate-500 mt-0.5">Kraken tick lead over Revolut</span>
              </div>

              <div className="bg-[#0a0e17] border border-slate-800 p-3 rounded-lg flex flex-col justify-between">
                <span className="text-[10px] text-slate-400 font-bold uppercase tracking-wider flex items-center gap-1">
                  <ShieldCheck className="h-3 w-3 text-rose-400" />
                  Taker Fees Absorbed
                </span>
                <div className="text-lg font-mono font-black text-slate-300 mt-1">
                  £{(telemetry?.sniper?.total_taker_fees_paid_gbp ?? 0).toFixed(2)}
                </div>
                <span className="text-[10px] text-slate-500 mt-0.5">0.09% Revolut taker fee factored</span>
              </div>
            </div>

            {/* Radar Comparison & Parameter Tuning Split Grid */}
            <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 pt-1">
              {/* Live Venue Radar (7 cols) */}
              <div className="lg:col-span-7 bg-[#0a0e17] border border-slate-800 rounded-lg p-4 space-y-4">
                <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                  <div className="flex items-center gap-2">
                    <Radio className="h-4 w-4 text-amber-400" />
                    <span className="text-xs font-bold text-white">Cross-Venue Lead-Lag Radar (Live Feeds)</span>
                  </div>
                  <span className="text-[10px] text-slate-400 font-mono">
                    Hurdle: ≥{telemetry?.sniper?.impulse_threshold_pct ?? 0.18}%
                  </span>
                </div>

                <div className="space-y-3 font-mono text-xs">
                  {["BTC/GBP", "ETH/GBP"].map((sym) => {
                    const radar = telemetry?.sniper?.radar?.[sym];
                    const krakenP = radar?.kraken_price ?? (sym === "BTC/GBP" ? 57020 : 1820);
                    const revBid = radar?.revolut_best_bid ?? (krakenP * 0.9995);
                    const revAsk = radar?.revolut_best_ask ?? (krakenP * 1.0005);
                    const dislocation = radar?.current_dislocation_pct ?? 0.02;
                    const inSnipeZone = radar?.in_snipe_zone ?? false;
                    const hurdle = telemetry?.sniper?.impulse_threshold_pct ?? 0.18;
                    const progressPct = Math.min(100, Math.max(0, (dislocation / hurdle) * 100));

                    return (
                      <div
                        key={sym}
                        className={`p-3 rounded-lg border transition ${
                          inSnipeZone
                            ? "bg-amber-950/40 border-amber-500/80 shadow-lg shadow-amber-950/40"
                            : "bg-[#111622] border-slate-800/80"
                        }`}
                      >
                        <div className="flex items-center justify-between pb-2 border-b border-slate-800/60">
                          <div className="flex items-center gap-2">
                            <span className="font-bold text-white">{sym}</span>
                            <span
                              className={`text-[10px] font-bold px-1.5 py-0.5 rounded ${
                                inSnipeZone
                                  ? "bg-amber-400 text-black animate-pulse"
                                  : "bg-slate-800 text-slate-400"
                              }`}
                            >
                              {inSnipeZone ? "⚡ SNIPE TRIGGER ZONE" : "SCANNING"}
                            </span>
                          </div>
                          <span className="text-[11px] text-slate-400 font-sans">
                            Advantage: ~{radar?.lead_advantage_ms ?? 420}ms
                          </span>
                        </div>

                        {/* Venue comparison row */}
                        <div className="grid grid-cols-3 gap-2 py-2 text-center">
                          <div className="bg-[#0a0e17] p-1.5 rounded border border-slate-800">
                            <div className="text-[9px] text-cyan-400 font-sans uppercase">Kraken (Lead)</div>
                            <div className="font-bold text-white text-[11px]">
                              £{krakenP.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                            </div>
                          </div>

                          <div className="bg-[#0a0e17] p-1.5 rounded border border-slate-800">
                            <div className="text-[9px] text-emerald-400 font-sans uppercase">Revolut Best Bid</div>
                            <div className="font-bold text-emerald-300 text-[11px]">
                              £{revBid.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                            </div>
                          </div>

                          <div className="bg-[#0a0e17] p-1.5 rounded border border-slate-800">
                            <div className="text-[9px] text-rose-400 font-sans uppercase">Revolut Best Ask</div>
                            <div className="font-bold text-rose-300 text-[11px]">
                              £{revAsk.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                            </div>
                          </div>
                        </div>

                        {/* Dislocation gauge bar */}
                        <div className="pt-1">
                          <div className="flex items-center justify-between text-[10px] text-slate-400 mb-1">
                            <span>Dislocation: {dislocation.toFixed(3)}%</span>
                            <span className={inSnipeZone ? "text-amber-300 font-bold" : "text-slate-500"}>
                              Taker Fee Hurdle: {hurdle.toFixed(2)}%
                            </span>
                          </div>
                          <div className="w-full bg-slate-900 rounded-full h-2 overflow-hidden border border-slate-800">
                            <div
                              className={`h-full transition-all duration-300 ${
                                inSnipeZone
                                  ? "bg-gradient-to-r from-amber-500 to-emerald-400 animate-pulse"
                                  : "bg-cyan-500/70"
                              }`}
                              style={{ width: `${progressPct}%` }}
                            />
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* Dynamic Tuning Controls (5 cols) */}
              <div className="lg:col-span-5 bg-[#0a0e17] border border-slate-800 rounded-lg p-4 space-y-4 flex flex-col justify-between">
                <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                  <div className="flex items-center gap-2">
                    <Sliders className="h-4 w-4 text-cyan-400" />
                    <span className="text-xs font-bold text-white">Sniper Parameters</span>
                  </div>
                  <span className="text-[10px] text-amber-400 font-mono">0.09% Taker Barrier</span>
                </div>

                <div className="space-y-3 text-xs">
                  <div>
                    <div className="flex items-center justify-between text-slate-300 mb-1">
                      <label className="text-[11px]">Impulse Hurdle Threshold (%)</label>
                      <span className="text-cyan-400 font-mono font-bold">{sniperParams.impulse_threshold_pct}%</span>
                    </div>
                    <input
                      type="range"
                      min="0.05"
                      max="0.50"
                      step="0.01"
                      value={sniperParams.impulse_threshold_pct}
                      onChange={(e) => setSniperParams({ ...sniperParams, impulse_threshold_pct: e.target.value })}
                      className="w-full accent-amber-400 cursor-pointer"
                    />
                    <span className="text-[10px] text-slate-500 block">
                      Must exceed 0.09% taker fee + net target to trigger.
                    </span>
                  </div>

                  <div>
                    <label className="text-[11px] text-slate-300 block mb-1">Snipe Order Size (£ GBP)</label>
                    <div className="grid grid-cols-4 gap-1.5">
                      {["25", "50", "100", "250"].map((sz) => (
                        <button
                          key={sz}
                          type="button"
                          onClick={() => setSniperParams({ ...sniperParams, snipe_order_size_gbp: sz })}
                          className={`py-1 rounded text-xs font-mono font-bold border transition ${
                            sniperParams.snipe_order_size_gbp === sz
                              ? "bg-amber-400 text-black border-amber-300 font-black"
                              : "bg-slate-900 text-slate-400 border-slate-700 hover:text-white"
                          }`}
                        >
                          £{sz}
                        </button>
                      ))}
                    </div>
                  </div>

                  <div>
                    <label className="text-[11px] text-slate-300 block mb-1">Minimum Net Edge Target (%)</label>
                    <input
                      type="number"
                      step="0.01"
                      value={sniperParams.min_net_edge_pct}
                      onChange={(e) => setSniperParams({ ...sniperParams, min_net_edge_pct: e.target.value })}
                      className="w-full bg-slate-900 border border-slate-700 px-2.5 py-1.5 rounded text-xs text-white focus:border-amber-400 outline-none font-mono"
                    />
                  </div>
                </div>

                <button
                  onClick={handleTuneSniper}
                  className="w-full bg-gradient-to-r from-amber-500 to-amber-600 hover:from-amber-400 hover:to-amber-500 text-black font-bold text-xs py-2 rounded-lg transition flex items-center justify-center gap-1.5 shadow-md shadow-amber-950/40 active:scale-95"
                >
                  <RefreshCw className="h-3.5 w-3.5" />
                  Apply Sniper Tuning
                </button>
              </div>
            </div>
          </div>

          {/* Real-Time Live Trade Tape & Resting Orders Ladder */}
          <div className="grid grid-cols-1 lg:grid-cols-12 gap-6">
            {/* Live Real-Time Trade Tape (5 cols) */}
            <div className="lg:col-span-5 bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-3 flex flex-col h-[420px]">
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <div className="flex items-center gap-2">
                  <Activity className="h-4 w-4 text-emerald-400" />
                  <span className="text-sm font-bold text-white">Live Execution Tape</span>
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-950 text-emerald-300 font-bold border border-emerald-800">
                    STREAMING
                  </span>
                </div>
                <span className="text-xs text-slate-500">{(telemetry?.live_trades?.length || 0)} events</span>
              </div>

              <div className="flex-1 overflow-y-auto space-y-2 pr-1 font-mono text-xs">
                {(!telemetry?.live_trades || telemetry.live_trades.length === 0) ? (
                  <div className="text-center py-16 text-slate-500 italic">
                    Connecting to live tick feed and evaluating resting orders...
                  </div>
                ) : (
                  telemetry.live_trades.map((t) => {
                    const isSnipeBuy = t.action === "SNIPE_BUY";
                    const isSnipeSell = t.action === "SNIPE_SELL";
                    const isSnipe = isSnipeBuy || isSnipeSell;
                    const isSnipeArm = t.action.startsWith("SNIPER_");
                    const isBuy = t.action === "BUY";
                    const isSell = t.action === "SELL";
                    const isKill = t.action === "KILL_SWITCH";

                    return (
                      <div
                        key={t.id}
                        className={`p-2.5 rounded-lg border transition ${
                          isSnipe
                            ? "bg-amber-950/40 border-amber-500/70 shadow-sm shadow-amber-950/50"
                            : isSnipeArm
                            ? "bg-amber-950/20 border-amber-800/40"
                            : isBuy
                            ? "bg-emerald-950/30 border-emerald-800/50"
                            : isSell
                            ? "bg-cyan-950/30 border-cyan-800/50"
                            : isKill
                            ? "bg-rose-950/40 border-rose-800/60"
                            : "bg-slate-900/60 border-slate-800"
                        }`}
                      >
                        <div className="flex items-center justify-between text-[11px]">
                          <div className="flex items-center gap-2">
                            <span className="text-slate-500 text-[10px]">{t.time_str}</span>
                            <span
                              className={`px-1.5 py-0.5 rounded font-bold text-[10px] flex items-center gap-1 ${
                                isSnipeBuy
                                  ? "bg-amber-400 text-black font-extrabold"
                                  : isSnipeSell
                                  ? "bg-orange-400 text-black font-extrabold"
                                  : isSnipeArm
                                  ? "bg-amber-900 text-amber-200 font-semibold"
                                  : isBuy
                                  ? "bg-emerald-500 text-black"
                                  : isSell
                                  ? "bg-cyan-400 text-black"
                                  : isKill
                                  ? "bg-rose-600 text-white"
                                  : "bg-amber-500 text-black"
                              }`}
                            >
                              {isSnipe && <Zap className="h-2.5 w-2.5 fill-black" />}
                              {t.action}
                            </span>
                            <span className="text-slate-300 font-semibold">{t.symbol}</span>
                          </div>
                          {t.profit > 0 && (
                            <span className={`font-bold ${isSnipe ? "text-amber-300 font-mono" : "text-emerald-400 font-mono"}`}>
                              +£{t.profit.toFixed(2)}
                            </span>
                          )}
                        </div>
                        <div className={`text-[11px] mt-1 leading-snug ${isSnipe ? "text-amber-200/90 font-mono" : "text-slate-400"}`}>
                          {t.note}
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </div>

            {/* Active Resting Orders Ladder Table (7 cols) */}
            <div className="lg:col-span-7 bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-3 flex flex-col h-[420px]">
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <div className="flex items-center gap-2">
                  <Clock className="h-4 w-4 text-cyan-400" />
                  <span className="text-sm font-bold text-white">Active Resting Orders (Revolut X post_only)</span>
                  <span className="text-xs px-2 py-0.5 rounded bg-slate-800 text-cyan-300 font-bold">
                    {telemetry?.resting_orders?.length || 0} Open
                  </span>
                </div>

                {/* Filter buttons */}
                <div className="flex gap-1 text-[11px]">
                  {(["ALL", "BTC", "ETH"] as const).map((f) => (
                    <button
                      key={f}
                      onClick={() => setOrdersFilter(f)}
                      className={`px-2 py-0.5 rounded border transition ${
                        ordersFilter === f
                          ? "bg-cyan-500 text-black font-bold border-cyan-400"
                          : "bg-slate-900 border-slate-700 text-slate-400 hover:text-white"
                      }`}
                    >
                      {f}
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex-1 overflow-y-auto pr-1">
                <table className="w-full text-left text-xs font-mono">
                  <thead className="text-[10px] text-slate-500 border-b border-slate-800 sticky top-0 bg-[#111622]">
                    <tr>
                      <th className="pb-2">SIDE</th>
                      <th className="pb-2">ASSET</th>
                      <th className="pb-2 text-right">PRICE</th>
                      <th className="pb-2 text-right">QTY</th>
                      <th className="pb-2 text-right">VALUE</th>
                      <th className="pb-2 text-right">DISTANCE</th>
                      <th className="pb-2 text-center">TYPE</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-800/60">
                    {(!telemetry?.resting_orders || telemetry.resting_orders.length === 0) ? (
                      <tr>
                        <td colSpan={7} className="py-12 text-center text-slate-500 italic">
                          No open resting limit orders found.
                        </td>
                      </tr>
                    ) : (
                      telemetry.resting_orders
                        .filter((o) => {
                          if (ordersFilter === "BTC") return o.symbol.includes("BTC");
                          if (ordersFilter === "ETH") return o.symbol.includes("ETH");
                          return true;
                        })
                        .map((ord) => {
                          const isBuy = ord.side === "BUY";
                          return (
                            <tr key={ord.id} className="hover:bg-slate-800/40 transition text-[11px]">
                              <td className="py-1.5">
                                <span
                                  className={`px-1.5 py-0.5 rounded font-bold text-[10px] ${
                                    isBuy ? "bg-emerald-950 text-emerald-400 border border-emerald-800" : "bg-rose-950 text-rose-400 border border-rose-800"
                                  }`}
                                >
                                  {ord.side}
                                </span>
                              </td>
                              <td className="py-1.5 text-slate-300 font-semibold">{ord.symbol}</td>
                              <td className="py-1.5 text-right font-bold text-white">
                                £{ord.price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                              </td>
                              <td className="py-1.5 text-right text-slate-400">{ord.qty}</td>
                              <td className="py-1.5 text-right text-slate-300">£{ord.value_gbp.toFixed(2)}</td>
                              <td className={`py-1.5 text-right font-semibold ${ord.distance_pct >= 0 ? "text-rose-400" : "text-emerald-400"}`}>
                                {ord.distance_pct >= 0 ? `+${ord.distance_pct.toFixed(2)}%` : `${ord.distance_pct.toFixed(2)}%`}
                              </td>
                              <td className="py-1.5 text-center">
                                <span className="text-[10px] text-cyan-400 bg-cyan-950/40 px-1.5 py-0.5 rounded border border-cyan-800/50">
                                  post_only
                                </span>
                              </td>
                            </tr>
                          );
                        })
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ========================================================================= */}
      {/* TAB 2: HISTORICAL BACKTEST & LIVE PLAYBACK LAB */}
      {/* ========================================================================= */}
      {activeTab === "backtest" && (
        <div className="space-y-6">
          {/* Lab Controls Bar */}
          <div className="bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <h2 className="text-base font-bold text-white flex items-center gap-2">
                <BarChart3 className="h-5 w-5 text-cyan-400" />
                Paper Simulation & Live Order Playback Lab (Real Kraken OHLC Replay)
              </h2>
              <span className="text-xs text-slate-400">Intraday Wave Engine</span>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-4 gap-4 text-xs">
              {/* Asset Pair */}
              <div>
                <label className="text-slate-400 block mb-1.5">Asset Pair</label>
                <div className="flex gap-2">
                  {["BTC/GBP", "ETH/GBP"].map((s) => (
                    <button
                      key={s}
                      onClick={() => setBtSymbol(s)}
                      className={`flex-1 py-1.5 rounded border font-bold ${
                        btSymbol === s
                          ? "bg-cyan-500/20 border-cyan-500 text-cyan-300"
                          : "bg-slate-900 border-slate-700 text-slate-400 hover:text-white"
                      }`}
                    >
                      {s}
                    </button>
                  ))}
                </div>
              </div>

              {/* Timeframe */}
              <div>
                <label className="text-slate-400 block mb-1.5">Historical Timeframe</label>
                <div className="flex gap-1.5">
                  {[
                    { label: "7D", days: 7 },
                    { label: "30D", days: 30 },
                    { label: "90D", days: 90 },
                    { label: "180D", days: 180 },
                    { label: "12M", days: 365 },
                  ].map((t) => (
                    <button
                      key={t.days}
                      onClick={() => setBtTimeframe(t.days)}
                      className={`flex-1 py-1.5 rounded border text-[11px] font-bold ${
                        btTimeframe === t.days
                          ? "bg-cyan-500 text-black border-cyan-400"
                          : "bg-slate-900 border-slate-700 text-slate-400 hover:text-white"
                      }`}
                    >
                      {t.label}
                    </button>
                  ))}
                </div>
              </div>

              {/* Budget */}
              <div>
                <label className="text-slate-400 block mb-1.5">Starting Capital (£)</label>
                <div className="flex gap-2">
                  <input
                    type="number"
                    value={btBudget}
                    onChange={(e) => setBtBudget(Math.max(100, Number(e.target.value)))}
                    className="w-full bg-slate-900 border border-slate-700 px-2.5 py-1.5 rounded text-white focus:border-cyan-500 outline-none"
                  />
                  {[500, 1000, 5000].map((b) => (
                    <button
                      key={b}
                      onClick={() => setBtBudget(b)}
                      className="px-2 py-1 bg-slate-800 hover:bg-slate-700 border border-slate-700 rounded text-[10px] text-slate-300"
                    >
                      £{b}
                    </button>
                  ))}
                </div>
              </div>

              {/* Run button */}
              <div className="flex items-end">
                <button
                  onClick={runBacktestSimulation}
                  disabled={btLoading}
                  className="w-full bg-emerald-600 hover:bg-emerald-500 text-white font-bold py-2 px-4 rounded-lg flex items-center justify-center gap-2 shadow-lg shadow-emerald-950/60 transition disabled:opacity-50"
                >
                  {btLoading ? <RefreshCw className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4 fill-current" />}
                  <span>{btLoading ? "Simulating..." : `Run ${btTimeframe}D Simulation`}</span>
                </button>
              </div>
            </div>

            {/* Grid & Risk Sliders */}
            <div className="grid grid-cols-1 md:grid-cols-4 gap-4 pt-3 border-t border-slate-800/80 text-xs">
              <div>
                <div className="flex justify-between text-slate-400 mb-1">
                  <span>Starting Split:</span>
                  <span className="text-cyan-300 font-bold">{btSplit}% Cash / {100 - btSplit}% Crypto</span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="100"
                  step="10"
                  value={btSplit}
                  onChange={(e) => setBtSplit(Number(e.target.value))}
                  className="w-full accent-cyan-400 cursor-pointer"
                />
              </div>

              <div>
                <div className="flex justify-between text-slate-400 mb-1">
                  <span>Grid Step (%):</span>
                  <span className="text-white font-bold">{btStepPct.toFixed(2)}%</span>
                </div>
                <input
                  type="range"
                  min="0.20"
                  max="2.00"
                  step="0.05"
                  value={btStepPct}
                  onChange={(e) => setBtStepPct(Number(e.target.value))}
                  className="w-full accent-cyan-400 cursor-pointer"
                />
              </div>

              <div>
                <div className="flex justify-between text-slate-400 mb-1">
                  <span>Lag Filter Dump Threshold:</span>
                  <span className="text-emerald-400 font-bold">{btLagFilterThreshold.toFixed(2)}%</span>
                </div>
                <input
                  type="range"
                  min="0.30"
                  max="1.50"
                  step="0.05"
                  value={btLagFilterThreshold}
                  onChange={(e) => setBtLagFilterThreshold(Number(e.target.value))}
                  className="w-full accent-emerald-400 cursor-pointer"
                />
              </div>

              <div>
                <div className="flex justify-between text-slate-400 mb-1">
                  <span>Basis Divergence:</span>
                  <span className="text-purple-400 font-bold">±{btBasisSpread.toFixed(2)}%</span>
                </div>
                <input
                  type="range"
                  min="0.02"
                  max="0.50"
                  step="0.02"
                  value={btBasisSpread}
                  onChange={(e) => setBtBasisSpread(Number(e.target.value))}
                  className="w-full accent-purple-400 cursor-pointer"
                />
              </div>
            </div>

            {/* Configurable Profit Removal Section */}
            <div className="pt-3 border-t border-slate-800/80 space-y-3 bg-[#0a0e17] p-3 rounded-lg border border-slate-800">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs font-bold text-amber-300">
                  <PiggyBank className="h-4 w-4 text-amber-400" />
                  <span>Configurable Profit Removal & Vaulting (De-risking Principal)</span>
                </div>
                <label className="flex items-center gap-2 cursor-pointer text-xs">
                  <input
                    type="checkbox"
                    checked={btEnableSweep}
                    onChange={(e) => setBtEnableSweep(e.target.checked)}
                    className="accent-amber-400 rounded cursor-pointer h-4 w-4"
                  />
                  <span className="text-slate-300 font-bold">Enable Profit Skimming</span>
                </label>
              </div>

              {btEnableSweep && (
                <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-xs pt-1">
                  <div>
                    <label className="text-slate-400 block mb-1">Skimming Mode</label>
                    <div className="flex gap-2">
                      <button
                        onClick={() => setBtSweepMode("threshold")}
                        className={`flex-1 py-1 rounded border text-[11px] font-bold ${
                          btSweepMode === "threshold"
                            ? "bg-amber-500/20 border-amber-500 text-amber-300"
                            : "bg-slate-900 border-slate-700 text-slate-400 hover:text-white"
                        }`}
                      >
                        Milestone Threshold (+X%)
                      </button>
                      <button
                        onClick={() => setBtSweepMode("ratchet")}
                        className={`flex-1 py-1 rounded border text-[11px] font-bold ${
                          btSweepMode === "ratchet"
                            ? "bg-amber-500/20 border-amber-500 text-amber-300"
                            : "bg-slate-900 border-slate-700 text-slate-400 hover:text-white"
                        }`}
                      >
                        Continuous Ratchet (X% / fill)
                      </button>
                    </div>
                  </div>

                  {btSweepMode === "threshold" ? (
                    <div>
                      <div className="flex justify-between text-slate-400 mb-1">
                        <span>Milestone Threshold:</span>
                        <span className="text-amber-300 font-bold">
                          +{btSweepThreshold}% (£{(btBudget * (btSweepThreshold / 100)).toFixed(0)})
                        </span>
                      </div>
                      <input
                        type="range"
                        min="5"
                        max="25"
                        step="5"
                        value={btSweepThreshold}
                        onChange={(e) => setBtSweepThreshold(Number(e.target.value))}
                        className="w-full accent-amber-400 cursor-pointer"
                      />
                      <div className="flex justify-between text-[10px] text-slate-500 mt-0.5">
                        <span>+5% (£50)</span>
                        <span>+10% (£100)</span>
                        <span>+20% (£200)</span>
                      </div>
                    </div>
                  ) : (
                    <div>
                      <div className="flex justify-between text-slate-400 mb-1">
                        <span>Ratchet Split:</span>
                        <span className="text-amber-300 font-bold">{btSweepRatchet}% to Vault / {100 - btSweepRatchet}% bot</span>
                      </div>
                      <input
                        type="range"
                        min="25"
                        max="75"
                        step="25"
                        value={btSweepRatchet}
                        onChange={(e) => setBtSweepRatchet(Number(e.target.value))}
                        className="w-full accent-amber-400 cursor-pointer"
                      />
                      <div className="flex justify-between text-[10px] text-slate-500 mt-0.5">
                        <span>25% Vault</span>
                        <span>50% Vault</span>
                        <span>75% Vault</span>
                      </div>
                    </div>
                  )}

                  <div className="flex items-center text-[11px] text-slate-400 leading-tight bg-slate-900/60 p-2 rounded border border-slate-800">
                    <div>
                      <strong className="text-amber-300">How it works:</strong> Realized profits above the threshold are automatically extracted into a safe GBP Vault, locking in gains and ensuring your principal is recovered.
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>

          {/* Simulation Output Area */}
          {btResult && (
            <div className="space-y-6">
              {/* LIVE PLAYBACK CONTROLLER BAR */}
              <div className="bg-[#111622] border border-cyan-500/40 rounded-xl p-4 shadow-xl space-y-3">
                <div className="flex flex-col md:flex-row md:items-center justify-between gap-3 border-b border-slate-800 pb-3">
                  <div className="flex items-center gap-3">
                    <div className="flex items-center gap-1.5 text-xs font-bold text-cyan-400 bg-cyan-500/10 border border-cyan-500/30 px-2.5 py-1 rounded">
                      <Radio className="h-3.5 w-3.5 animate-pulse" />
                      <span>LIVE PLAYBACK REPLAY</span>
                    </div>
                    <span className="text-sm font-bold text-white">
                      Day {currentPoint?.day || 1} / {btResult.equity_curve.length}
                    </span>
                    <span className="text-xs text-slate-400 bg-slate-900 px-2 py-0.5 rounded border border-slate-800">
                      {currentPoint?.date || ""}
                    </span>
                  </div>

                  {/* Playback Controls */}
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => {
                        setPlaybackIndex(0);
                        setIsPlaying(true);
                      }}
                      className="p-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded border border-slate-700"
                      title="Restart to Day 1"
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                    </button>
                    <button
                      onClick={() => setIsPlaying(!isPlaying)}
                      className={`px-3 py-1.5 rounded font-bold text-xs flex items-center gap-1.5 transition ${
                        isPlaying
                          ? "bg-amber-500 hover:bg-amber-400 text-black"
                          : "bg-emerald-600 hover:bg-emerald-500 text-white"
                      }`}
                    >
                      {isPlaying ? <Pause className="h-3.5 w-3.5 fill-current" /> : <Play className="h-3.5 w-3.5 fill-current" />}
                      <span>{isPlaying ? "Pause Playback" : "Play Replay"}</span>
                    </button>

                    {/* Speed Selector */}
                    <div className="flex items-center gap-1 bg-slate-900 border border-slate-800 p-0.5 rounded text-[11px]">
                      {[
                        { label: "1x", ms: 400 },
                        { label: "3x", ms: 200 },
                        { label: "10x", ms: 80 },
                        { label: "Fast", ms: 30 },
                      ].map((sp) => (
                        <button
                          key={sp.label}
                          onClick={() => setPlaybackSpeedMs(sp.ms)}
                          className={`px-2 py-0.5 rounded ${
                            playbackSpeedMs === sp.ms ? "bg-cyan-500 text-black font-bold" : "text-slate-400 hover:text-white"
                          }`}
                        >
                          {sp.label}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>

                {/* Scrubber Slider */}
                <div className="space-y-1">
                  <div className="flex justify-between text-[11px] text-slate-400">
                    <span>Replay Progress:</span>
                    <span>
                      {(((playbackIndex + 1) / btResult.equity_curve.length) * 100).toFixed(0)}% (Day {playbackIndex + 1} of {btResult.equity_curve.length})
                    </span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max={btResult.equity_curve.length - 1}
                    value={playbackIndex}
                    onChange={(e) => {
                      setIsPlaying(false);
                      setPlaybackIndex(Number(e.target.value));
                    }}
                    className="w-full accent-cyan-400 cursor-pointer h-2 bg-slate-900 rounded-lg"
                  />
                </div>

                {/* Current Playback Tickers HUD */}
                {currentPoint && (
                  <div className="grid grid-cols-2 md:grid-cols-4 gap-3 pt-2 text-xs">
                    <div className="bg-[#0a0e17] p-2.5 rounded border border-slate-800">
                      <div className="text-slate-400 text-[10px]">CURRENT ASSET PRICE</div>
                      <div className="text-base font-bold text-white mt-0.5">
                        £{currentPoint.price.toFixed(2)}
                      </div>
                    </div>
                    <div className="bg-[#0a0e17] p-2.5 rounded border border-emerald-900/40">
                      <div className="text-emerald-400 text-[10px]">TOTAL WEALTH</div>
                      <div className="text-base font-bold text-emerald-300 mt-0.5">
                        £{currentPoint.total_wealth_protected.toFixed(2)}
                      </div>
                    </div>
                    <div className="bg-[#0a0e17] p-2.5 rounded border border-amber-900/40">
                      <div className="text-amber-400 text-[10px]">SAFE BANKED VAULT</div>
                      <div className="text-base font-bold text-amber-300 mt-0.5">
                        £{currentPoint.vault_cash_protected.toFixed(2)}
                      </div>
                    </div>
                    <div className="bg-[#0a0e17] p-2.5 rounded border border-cyan-900/40">
                      <div className="text-cyan-400 text-[10px]">HODL VALUE</div>
                      <div className="text-base font-bold text-cyan-300 mt-0.5">
                        £{currentPoint.benchmark_hodl.toFixed(2)}
                      </div>
                    </div>
                  </div>
                )}
              </div>

              {/* Metric KPI Cards */}
              <div className="grid grid-cols-1 md:grid-cols-6 gap-3">
                <div className="bg-[#111622] border border-emerald-500/40 p-3.5 rounded-xl">
                  <div className="text-[11px] text-emerald-400 flex items-center gap-1">
                    <ShieldCheck className="h-3.5 w-3.5" />
                    TOTAL WEALTH
                  </div>
                  <div className="text-xl font-bold text-white mt-1">
                    £{currentPoint ? currentPoint.total_wealth_protected.toFixed(2) : btResult.total_realized_wealth_gbp.toFixed(2)}
                  </div>
                  <div className="text-xs text-emerald-400 font-bold mt-0.5">
                    {currentPoint
                      ? (((currentPoint.total_wealth_protected - btResult.initial_budget) / btResult.initial_budget) * 100 >= 0 ? "+" : "") +
                        (((currentPoint.total_wealth_protected - btResult.initial_budget) / btResult.initial_budget) * 100).toFixed(2)
                      : btResult.return_protected_pct.toFixed(2)}% Total Return
                  </div>
                </div>

                <div className="bg-[#111622] border border-amber-500/40 p-3.5 rounded-xl">
                  <div className="text-[11px] text-amber-400 flex items-center gap-1">
                    <Landmark className="h-3.5 w-3.5" />
                    BANKED VAULT CASH
                  </div>
                  <div className="text-xl font-bold text-amber-300 mt-1">
                    £{currentPoint ? currentPoint.vault_cash_protected.toFixed(2) : btResult.vaulted_profit_gbp.toFixed(2)}
                  </div>
                  <div className="text-xs text-slate-400 mt-0.5">
                    Safe in fiat reserve
                  </div>
                </div>

                <div className={`p-3.5 rounded-xl border ${btResult.house_money_achieved ? "bg-amber-950/30 border-amber-500" : "bg-[#111622] border-slate-800"}`}>
                  <div className="text-[11px] text-slate-300 flex items-center gap-1">
                    <Award className="h-3.5 w-3.5 text-amber-400" />
                    PRINCIPAL PAYBACK
                  </div>
                  <div className="text-xl font-bold text-white mt-1">
                    {currentPoint
                      ? Math.min(100, (currentPoint.vault_cash_protected / btResult.initial_budget) * 100).toFixed(1)
                      : btResult.principal_payback_pct.toFixed(1)}%
                  </div>
                  <div className="text-xs text-amber-400 mt-0.5 font-bold">
                    {btResult.house_money_achieved
                      ? `House Money (Day ${btResult.house_money_day})`
                      : `£${Math.max(0, btResult.initial_budget - (currentPoint ? currentPoint.vault_cash_protected : btResult.vaulted_profit_gbp)).toFixed(0)} to Breakeven`}
                  </div>
                </div>

                <div className="bg-[#111622] border border-cyan-500/30 p-3.5 rounded-xl">
                  <div className="text-[11px] text-cyan-400 flex items-center gap-1">
                    <TrendingUp className="h-3.5 w-3.5" />
                    HODL BENCHMARK
                  </div>
                  <div className="text-xl font-bold text-white mt-1">
                    £{currentPoint ? currentPoint.benchmark_hodl.toFixed(2) : btResult.benchmark_final_equity.toFixed(2)}
                  </div>
                  <div className="text-xs text-slate-400 mt-0.5">
                    Buy & Hold value
                  </div>
                </div>

                <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
                  <div className="text-[11px] text-slate-400">TOTAL COMPLETED TRADES</div>
                  <div className="text-xl font-bold text-cyan-400 mt-1">
                    {btResult ? Math.round(btResult.total_trades_protected * ((playbackIndex + 1) / Math.max(1, btResult.equity_curve.length))) : 0}
                  </div>
                  <div className="text-xs text-slate-500 mt-0.5">
                    Executed up to Day {playbackIndex + 1}
                  </div>
                </div>

                <div className="bg-[#111622] border border-slate-800 p-3.5 rounded-xl">
                  <div className="text-[11px] text-slate-400">0.00% FEE SAVINGS</div>
                  <div className="text-xl font-bold text-cyan-300 mt-1">
                    £{btResult ? (btResult.fee_savings_gbp * ((playbackIndex + 1) / Math.max(1, btResult.equity_curve.length))).toFixed(2) : "0.00"}
                  </div>
                  <div className="text-xs text-slate-500 mt-0.5">vs 0.40% exchange fees</div>
                </div>
              </div>

              {/* Multi-Line Performance Chart with Progressive Playback */}
              <div className="bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-4">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-bold text-white flex items-center gap-2">
                    <TrendingUp className="h-4 w-4 text-emerald-400" />
                    Live Trajectory Animation (Playback up to Day {playbackIndex + 1})
                  </h3>
                  {currentPoint && (
                    <div className="text-xs text-slate-300 bg-slate-900 border border-slate-700 px-3 py-1 rounded">
                      <span>{currentPoint.date}</span> | Price: £{currentPoint.price.toFixed(2)} |{" "}
                      <span className="text-emerald-400 font-bold">Wealth: £{currentPoint.total_wealth_protected.toFixed(2)}</span> |{" "}
                      <span className="text-amber-400">Vault: £{currentPoint.vault_cash_protected.toFixed(2)}</span>
                    </div>
                  )}
                </div>

                {renderChart(btResult.equity_curve, playbackIndex)}
              </div>

              {/* LIVE SIMULATED ORDER TAPE */}
              <div className="bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-3">
                <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                  <h4 className="text-xs font-bold text-white flex items-center gap-2">
                    <Activity className="h-4 w-4 text-cyan-400" />
                    <span>Live Simulated Order Execution Tape (Day {playbackIndex + 1})</span>
                  </h4>
                  <span className="text-[11px] text-slate-500">Showing recent fills up to playhead</span>
                </div>

                <div className="space-y-2 max-h-64 overflow-y-auto pr-1">
                  {recentTrades.length === 0 ? (
                    <div className="text-center py-6 text-xs text-slate-500">
                      Press <strong className="text-cyan-400">Play Replay</strong> to watch orders execute in real-time.
                    </div>
                  ) : (
                    recentTrades.map((trade) => {
                      let badgeColor = "bg-slate-800 text-slate-300";
                      let icon = <Clock className="h-3.5 w-3.5" />;

                      if (trade.action === "BUY") {
                        badgeColor = "bg-emerald-950 text-emerald-300 border border-emerald-800";
                        icon = <ArrowDownRight className="h-3.5 w-3.5 text-emerald-400" />;
                      } else if (trade.action === "SELL") {
                        badgeColor = "bg-indigo-950 text-indigo-300 border border-indigo-800";
                        icon = <ArrowUpRight className="h-3.5 w-3.5 text-indigo-400" />;
                      } else if (trade.action === "VAULT_SWEEP") {
                        badgeColor = "bg-amber-950 text-amber-300 border border-amber-600 animate-bounce";
                        icon = <Landmark className="h-3.5 w-3.5 text-amber-400" />;
                      } else if (trade.action === "LAG_CANCEL") {
                        badgeColor = "bg-purple-950 text-purple-300 border border-purple-700";
                        icon = <ShieldCheck className="h-3.5 w-3.5 text-purple-400" />;
                      }

                      return (
                        <div
                          key={trade.id}
                          className="bg-[#0a0e17] border border-slate-800/80 p-2.5 rounded-lg flex items-center justify-between text-xs transition hover:border-slate-700"
                        >
                          <div className="flex items-center gap-3">
                            <span className={`px-2 py-0.5 rounded font-bold text-[10px] flex items-center gap-1 ${badgeColor}`}>
                              {icon}
                              {trade.action}
                            </span>
                            <span className="text-slate-400 text-[11px]">{trade.date} (Day {trade.day})</span>
                            <span className="text-slate-200">{trade.note}</span>
                          </div>

                          <div className="text-right">
                            <div className="font-bold text-white">£{trade.price.toFixed(2)}</div>
                            {trade.profit > 0 && (
                              <div className="text-[10px] text-emerald-400 font-bold">+£{trade.profit.toFixed(2)} profit</div>
                            )}
                          </div>
                        </div>
                      );
                    })
                  )}
                </div>
              </div>

              {/* Strategy Insights */}
              <div className="bg-[#111622] border border-slate-800 rounded-xl p-5 space-y-3 text-xs">
                <h4 className="font-bold text-slate-200 flex items-center gap-2">
                  <CheckCircle2 className="h-4 w-4 text-cyan-400" />
                  Profit Removal & Treasury Strategy Summary
                </h4>
                <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-slate-400">
                  <div className="bg-[#0a0e17] p-3 rounded border border-slate-800/80">
                    <span className="text-amber-300 font-bold block mb-1">Permanent Fiat Protection:</span>
                    By sweeping profits into the Vault, you banked{" "}
                    <strong className="text-amber-300">£{btResult.vaulted_profit_gbp.toFixed(2)} in cold cash</strong>.
                    Even if the crypto market collapses 60% tomorrow, that £{btResult.vaulted_profit_gbp.toFixed(2)} is 100% safe.
                  </div>
                  <div className="bg-[#0a0e17] p-3 rounded border border-slate-800/80">
                    <span className="text-emerald-300 font-bold block mb-1">House Money Acceleration:</span>
                    {btResult.house_money_achieved ? (
                      <span>
                        Congratulations! The strategy achieved <strong className="text-emerald-300">100% principal payback on Day {btResult.house_money_day}</strong>. From that point on, you were trading entirely on free-roll profits.
                      </span>
                    ) : (
                      <span>
                        The bot recovered <strong className="text-emerald-300">{btResult.principal_payback_pct.toFixed(1)}% of your starting capital</strong>, drastically lowering your downside risk exposure.
                      </span>
                    )}
                  </div>
                  <div className="bg-[#0a0e17] p-3 rounded border border-slate-800/80">
                    <span className="text-cyan-300 font-bold block mb-1">The Revolut X 0.00% Edge:</span>
                    Over {btResult.total_trades_protected} trades, paying standard 0.40% exchange fees would have drained{" "}
                    <strong className="text-rose-400">£{btResult.fee_savings_gbp.toFixed(2)}</strong> from your account. With Revolut X <code className="text-white">post_only</code>, you kept every single penny.
                  </div>
                </div>
              </div>
            </div>
          )}

          {!btResult && !btLoading && (
            <div className="bg-[#111622] border border-slate-800/80 rounded-xl p-12 text-center space-y-3">
              <div className="h-12 w-12 rounded-full bg-cyan-500/10 text-cyan-400 flex items-center justify-center mx-auto">
                <BarChart3 className="h-6 w-6" />
              </div>
              <h3 className="text-base font-bold text-white">Historical Replay Ready</h3>
              <p className="text-xs text-slate-400 max-w-md mx-auto">
                Configure your asset, starting budget, and profit skimming threshold above, then click <strong>Run Simulation</strong> to launch the live playback replay.
              </p>
            </div>
          )}
        </div>
      )}

      {/* Emergency Kill Modal */}
      {killModalOpen && (
        <div className="fixed inset-0 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4 z-50">
          <div className="bg-slate-900 border border-rose-600 rounded-xl max-w-md w-full p-6 space-y-4 shadow-2xl">
            <div className="flex items-center gap-3 text-rose-400">
              <AlertTriangle className="h-6 w-6" />
              <h3 className="text-lg font-bold">Confirm Emergency Kill Switch?</h3>
            </div>
            <p className="text-xs text-slate-300 leading-relaxed">
              This action will immediately:
              <br />• Trip the Global Circuit Breaker.
              <br />• Cancel all resting limit orders across Revolut X and Kraken.
              <br />• Pause all active strategy runners.
            </p>
            <div className="flex items-center justify-end gap-3 pt-2">
              <button
                onClick={() => setKillModalOpen(false)}
                className="px-4 py-2 rounded text-xs bg-slate-800 text-slate-300 hover:bg-slate-700"
              >
                Cancel
              </button>
              <button
                onClick={handleKillSwitch}
                className="px-4 py-2 rounded text-xs font-bold bg-rose-600 text-white hover:bg-rose-500 shadow-lg shadow-rose-900/50"
              >
                Yes, Trigger Kill Switch
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
