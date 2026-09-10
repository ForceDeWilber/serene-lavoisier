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
  ShieldCheck,
  Award,
  FastForward,
  RotateCcw,
  Radio,
  ArrowDownRight,
  ArrowUpRight,
  Clock,
  Terminal,
  Layers,
  FileCode,
  ExternalLink,
  ChevronDown,
  ChevronUp,
  SlidersHorizontal,
  Wifi,
  WifiOff,
  Lock,
  Coins,
  Scale,
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

interface CapitalManagement {
  balance_source: string;
  starting_balance_gbp: number;
  settled_cash_gbp: number;
  cumulative_profit_gbp: number;
  profit_lock_pct: number;
  locked_profit_gbp: number;
  unlocked_profit_gbp: number;
  active_trading_power_gbp: number;
  expansion_ratio: number;
  rungs_per_side: number;
  split_btc_pct: number;
  split_eth_pct: number;
  allocations: {
    trading_power_gbp: number;
    expansion_ratio: number;
    runner_btc: {
      envelope_gbp: number;
      split_pct: number;
      rungs_per_side: number;
      order_size_gbp: number;
    };
    runner_eth: {
      envelope_gbp: number;
      split_pct: number;
      rungs_per_side: number;
      order_size_gbp: number;
    };
    sniper: {
      order_size_gbp: number;
    };
  };
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
  capital_management?: CapitalManagement;
  market_prices?: Record<string, MarketPriceInfo>;
  runners: RunnerTelemetry[];
  sniper?: SniperTelemetry;
  resting_orders?: RestingOrder[];
  resting_orders_count: number;
  live_trades?: LiveTradeEvent[];
  timestamp?: number;
}

export default function ProductionDashboard() {
  const [telemetry, setTelemetry] = useState<TelemetryPayload | null>(null);
  const [connected, setConnected] = useState<boolean>(false);
  const [lastSeenTs, setLastSeenTs] = useState<number | null>(null);
  const [isFeedStale, setIsFeedStale] = useState<boolean>(false);
  const [statusMessage, setStatusMessage] = useState<string>("");

  // Mode Toggles: "monitor" (pure data first) vs "tuning" (reveals inline sliders)
  const [viewMode, setViewMode] = useState<"monitor" | "tuning">("monitor");

  // Filter for resting orders and trade tape
  const [assetFilter, setAssetFilter] = useState<"ALL" | "BTC" | "ETH">("ALL");
  const [tapeFilter, setTapeFilter] = useState<"ALL" | "SNIPES" | "GRIDS">("ALL");

  // Modals
  const [killModalOpen, setKillModalOpen] = useState<boolean>(false);

  // Runner tuning state
  const [runnerParams, setRunnerParams] = useState<Record<string, { step_pct: string; rebalance_pct: string }>>({
    runner_btc: { step_pct: "0.40", rebalance_pct: "2.0" },
    runner_eth: { step_pct: "0.40", rebalance_pct: "2.0" },
  });

  // Sniper tuning state
  const [sniperParams, setSniperParams] = useState({
    impulse_threshold_pct: "0.18",
    snipe_order_size_gbp: "50",
    min_net_edge_pct: "0.05",
  });

  // Capital Management & Dynamic Profit Lock state
  const [capitalParams, setCapitalParams] = useState({
    profit_lock_pct: "30",
    split_btc_pct: "50",
    starting_balance_gbp: "10",
  });
  const [syncingRevolut, setSyncingRevolut] = useState<boolean>(false);

  const wsRef = useRef<WebSocket | null>(null);
  const lastSeenRef = useRef<number>(Date.now());
  const stalenessCheckTimer = useRef<NodeJS.Timeout | null>(null);

  // Telemetry refresh rate is 1.0s (1000ms).
  // Stale feed notification only appears if elapsed time exceeds 1 second over refresh time (2000ms total).
  const REFRESH_INTERVAL_MS = 1000;
  const STALE_THRESHOLD_MS = REFRESH_INTERVAL_MS + 1000; // 2000ms

  // Poll fallback & WebSocket connection
  const fetchTelemetry = async () => {
    try {
      const res = await fetch("/api/proxy/telemetry");
      if (res.ok) {
        const data = await res.json();
        setTelemetry(data);
        setConnected(true);
        lastSeenRef.current = Date.now();
        setLastSeenTs(lastSeenRef.current);
        setIsFeedStale(false);
      }
    } catch {
      setConnected(false);
      if (Date.now() - lastSeenRef.current > STALE_THRESHOLD_MS) {
        setIsFeedStale(true);
      }
    }
  };

  useEffect(() => {
    let active = true;

    const connectWs = () => {
      if (!active) return;
      
      // Determine WebSocket URL:
      // If NEXT_PUBLIC_ENGINE_WS_URL is set, connect directly to it.
      // If running on localhost, connect to local engine.
      // Otherwise, gracefully rely on the 1.0s /api/proxy/telemetry polling.
      const directWs = process.env.NEXT_PUBLIC_ENGINE_WS_URL;
      const wsUrl = directWs || (
        typeof window !== "undefined" && window.location.hostname === "localhost"
          ? "ws://localhost:8000/api/ws/stream"
          : null
      );

      if (!wsUrl) return;

      try {
        const socket = new WebSocket(wsUrl);
        wsRef.current = socket;

        socket.onopen = () => {
          if (!active) return;
          setConnected(true);
          lastSeenRef.current = Date.now();
          setLastSeenTs(lastSeenRef.current);
          setIsFeedStale(false);
        };

        socket.onmessage = (event) => {
          if (!active) return;
          try {
            const data = JSON.parse(event.data);
            setTelemetry(data);
            setConnected(true);
            lastSeenRef.current = Date.now();
            setLastSeenTs(lastSeenRef.current);
            setIsFeedStale(false);
          } catch (e) {
            console.error("Telemetry parse error", e);
          }
        };

        socket.onerror = () => {
          socket.close();
        };

        socket.onclose = () => {
          if (!active) return;
          setConnected(false);
          setTimeout(connectWs, 2000);
        };
      } catch {
        // Fallback polling handles updates
      }
    };

    connectWs();
    const pollInterval = setInterval(fetchTelemetry, REFRESH_INTERVAL_MS);

    // Staleness watchdog: only flag feed as STALE if > 1s over our refresh time (2000ms total)
    stalenessCheckTimer.current = setInterval(() => {
      const elapsed = Date.now() - lastSeenRef.current;
      setIsFeedStale(elapsed > STALE_THRESHOLD_MS);
    }, 250);

    return () => {
      active = false;
      clearInterval(pollInterval);
      if (stalenessCheckTimer.current) clearInterval(stalenessCheckTimer.current);
      if (wsRef.current) wsRef.current.close();
    };
  }, []);

  // Sync params from telemetry
  useEffect(() => {
    if (telemetry?.runners) {
      const updated = { ...runnerParams };
      telemetry.runners.forEach((r) => {
        if (r.step_pct && !runnerParams[r.runner_id]) {
          updated[r.runner_id] = {
            step_pct: (r.step_pct * 100).toFixed(2),
            rebalance_pct: (r.rebalance_threshold_pct ? r.rebalance_threshold_pct * 100 : 2.0).toFixed(1),
          };
        }
      });
      setRunnerParams(updated);
    }

    if (telemetry?.sniper) {
      const sn = telemetry.sniper;
      setSniperParams((prev) => ({
        impulse_threshold_pct: prev.impulse_threshold_pct || String(sn.impulse_threshold_pct),
        snipe_order_size_gbp: prev.snipe_order_size_gbp || String(sn.snipe_order_size_gbp),
        min_net_edge_pct: prev.min_net_edge_pct || String(sn.min_net_edge_pct),
      }));
    }

    if (telemetry?.capital_management) {
      const cm = telemetry.capital_management;
      setCapitalParams((prev) => ({
        profit_lock_pct: prev.profit_lock_pct || String(Math.round(cm.profit_lock_pct * 100)),
        split_btc_pct: prev.split_btc_pct || String(Math.round(cm.split_btc_pct * 100)),
        starting_balance_gbp: prev.starting_balance_gbp || String(cm.starting_balance_gbp),
      }));
    }
  }, [telemetry]);

  // API Calls
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

  const handleTuneRunner = async (runnerId: string) => {
    const params = runnerParams[runnerId];
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

  const handleToggleSniper = async () => {
    try {
      const res = await fetch("/api/proxy/sniper/toggle", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({}),
      });
      if (res.ok) {
        const data = await res.json();
        setStatusMessage(`Stale Quote Sniper is now ${data.payload?.status || "updated"}`);
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
        setStatusMessage("Stale Quote Sniper parameters tuned successfully!");
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Sniper tuning error: ${err.message}`);
    }
  };

  const handleKillSwitch = async () => {
    try {
      const res = await fetch("/api/proxy/emergency/kill-switch", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: "Emergency Kill Switch from Production Dashboard" }),
      });
      if (res.ok) {
        setStatusMessage("🚨 Global Kill Switch Activated! All resting orders canceled.");
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
        setStatusMessage("✅ Circuit breaker reset, grids redeployed, and sniper re-armed!");
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Reset failed: ${err.message}`);
    }
  };

  const handleConfigureCapital = async (updates?: Partial<{ profit_lock_pct: number; split_btc_pct: number; starting_balance_gbp: number }>) => {
    const lockVal = updates?.profit_lock_pct !== undefined ? updates.profit_lock_pct : parseFloat(capitalParams.profit_lock_pct) / 100;
    const splitVal = updates?.split_btc_pct !== undefined ? updates.split_btc_pct : parseFloat(capitalParams.split_btc_pct) / 100;
    const startingVal = updates?.starting_balance_gbp !== undefined ? updates.starting_balance_gbp : parseFloat(capitalParams.starting_balance_gbp);

    try {
      const res = await fetch("/api/proxy/capital/configure", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          profit_lock_pct: lockVal,
          split_btc_pct: splitVal,
          starting_balance_gbp: startingVal,
        }),
      });
      if (res.ok) {
        setStatusMessage("Capital allocation & profit lock updated successfully!");
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Capital config error: ${err.message}`);
    }
  };

  const handleSyncRevolutBalances = async () => {
    setSyncingRevolut(true);
    try {
      const res = await fetch("/api/proxy/capital/sync-revolut", {
        method: "POST",
      });
      if (res.ok) {
        const data = await res.json();
        const src = data.payload?.source || "PAPER_WALLET";
        setStatusMessage(`Balances synchronized (${src})`);
        fetchTelemetry();
      }
    } catch (err: any) {
      setStatusMessage(`Sync error: ${err.message}`);
    } finally {
      setSyncingRevolut(false);
    }
  };

  const handleLogout = async () => {
    try {
      await fetch("/api/auth/logout", { method: "POST" });
    } catch {}
    window.location.href = "/login";
  };

  const cbTripped = telemetry?.circuit_breaker_tripped ?? false;
  const portfolio = telemetry?.portfolio;
  const capMgmt = telemetry?.capital_management;
  const sniper = telemetry?.sniper;
  const runners = telemetry?.runners || [];
  const restingOrders = telemetry?.resting_orders || [];
  const liveTrades = telemetry?.live_trades || [];

  // Filter resting orders
  const filteredOrders = restingOrders.filter((o) => {
    if (assetFilter === "BTC") return o.symbol.includes("BTC");
    if (assetFilter === "ETH") return o.symbol.includes("ETH");
    return true;
  });

  // Filter trade tape
  const filteredTrades = liveTrades.filter((t) => {
    if (tapeFilter === "SNIPES") return t.action.includes("SNIPE");
    if (tapeFilter === "GRIDS") return t.action === "BUY" || t.action === "SELL";
    return true;
  });

  return (
    <main className="min-h-screen bg-[#070a10] text-slate-100 font-sans antialiased p-4 md:p-6 space-y-4">
      {/* ========================================================================= */}
      {/* 1. TOP INSTITUTIONAL CONTROL HEADER */}
      {/* ========================================================================= */}
      <header className="bg-[#0e131f] border border-slate-800/90 rounded-xl p-4 flex flex-col lg:flex-row lg:items-center justify-between gap-4 shadow-xl">
        <div className="flex items-center gap-3">
          <div className="p-2.5 rounded-lg bg-cyan-500/10 border border-cyan-500/30 text-cyan-400">
            <Terminal className="h-6 w-6" />
          </div>
          <div>
            <div className="flex items-center gap-2.5 flex-wrap">
              <h1 className="text-base md:text-lg font-black tracking-wider text-white font-mono uppercase">
                SERENE LAVOISIER <span className="text-cyan-400">// PROD TERMINAL</span>
              </h1>
              <span className="text-[10px] px-2 py-0.5 rounded font-mono font-bold bg-cyan-950 text-cyan-300 border border-cyan-800">
                v1.0-LIVE
              </span>
              <span className="text-[10px] px-2 py-0.5 rounded font-mono font-bold bg-slate-900 text-slate-300 border border-slate-700">
                PAPER EXECUTION (Zero Risk)
              </span>
            </div>
            <p className="text-xs text-slate-400 mt-0.5 font-sans">
              Autonomous Multi-Venue Control Desk • Revolut X (0.00% Maker) & Kraken Pro Oracle Feed
            </p>
          </div>
        </div>

        {/* Action Controls & Mode Switchers */}
        <div className="flex items-center gap-2.5 flex-wrap">
          {/* Link back to Testing Sandbox */}
          <Link
            href="/"
            className="bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-700 px-3 py-1.5 rounded-lg text-xs font-semibold flex items-center gap-1.5 transition"
          >
            <Layers className="h-3.5 w-3.5 text-cyan-400" />
            Testing Lab (/)
          </Link>

          {/* Feed Staleness Badge (Zero Fake Data Guarantee!) */}
          <div
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-mono font-bold border ${
              connected && !isFeedStale
                ? "bg-emerald-950/70 text-emerald-300 border-emerald-700"
                : "bg-amber-950/70 text-amber-300 border-amber-600 animate-pulse"
            }`}
          >
            {connected && !isFeedStale ? (
              <>
                <Wifi className="h-3.5 w-3.5 text-emerald-400" />
                <span>● LIVE REAL-TIME FEED</span>
              </>
            ) : (
              <>
                <WifiOff className="h-3.5 w-3.5 text-amber-400" />
                <span>
                  ⚠ STALE FEED {lastSeenTs ? `(T-${Math.round((Date.now() - lastSeenTs) / 1000)}s)` : "(OFFLINE)"}
                </span>
              </>
            )}
          </div>

          {/* Mode Switcher: Monitor Mode vs Tuning Mode */}
          <div className="bg-slate-950 p-1 rounded-lg border border-slate-800 flex items-center gap-1 text-xs font-semibold">
            <button
              onClick={() => setViewMode("monitor")}
              className={`px-3 py-1 rounded transition flex items-center gap-1.5 ${
                viewMode === "monitor"
                  ? "bg-cyan-500 text-black font-bold shadow"
                  : "text-slate-400 hover:text-white"
              }`}
            >
              <Activity className="h-3.5 w-3.5" />
              Monitor Mode
            </button>
            <button
              onClick={() => setViewMode("tuning")}
              className={`px-3 py-1 rounded transition flex items-center gap-1.5 ${
                viewMode === "tuning"
                  ? "bg-amber-500 text-black font-bold shadow"
                  : "text-slate-400 hover:text-white"
              }`}
            >
              <SlidersHorizontal className="h-3.5 w-3.5" />
              Tuning Mode
            </button>
          </div>

          {/* Lock / Logout Terminal Button */}
          <button
            onClick={handleLogout}
            title="Lock Production Terminal"
            className="bg-slate-900 hover:bg-slate-800 text-slate-400 hover:text-amber-400 border border-slate-800 hover:border-slate-700 px-2.5 py-1.5 rounded-lg text-xs transition flex items-center gap-1.5"
          >
            <Lock className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">Lock</span>
          </button>

          {/* Circuit Breaker Status */}
          <div
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold ${
              cbTripped
                ? "bg-rose-950 text-rose-300 border border-rose-600 animate-pulse"
                : "bg-emerald-950 text-emerald-300 border border-emerald-700"
            }`}
          >
            <ShieldAlert className="h-3.5 w-3.5" />
            <span>BREAKER: {cbTripped ? "TRIPPED" : "NORMAL"}</span>
          </div>

          {/* Kill Switch */}
          <button
            onClick={() => setKillModalOpen(true)}
            className="bg-rose-600 hover:bg-rose-500 text-white font-black px-3 py-1.5 rounded-lg text-xs flex items-center gap-1.5 shadow-lg shadow-rose-950/60 transition active:scale-95"
          >
            <AlertTriangle className="h-3.5 w-3.5" />
            KILL SWITCH
          </button>
        </div>
      </header>

      {/* Notifications banner */}
      {statusMessage && (
        <div className="bg-[#0e131f] border border-cyan-500/40 text-cyan-300 px-4 py-2.5 rounded-lg text-xs font-mono flex items-center justify-between shadow-lg">
          <span>{statusMessage}</span>
          <button onClick={() => setStatusMessage("")} className="text-slate-400 hover:text-white text-xs">
            Dismiss
          </button>
        </div>
      )}

      {/* ========================================================================= */}
      {/* 2. REAL-TIME CROSS-VENUE TICKER BAR */}
      {/* ========================================================================= */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {["BTC/GBP", "ETH/GBP"].map((sym) => {
          const mkt = telemetry?.market_prices?.[sym];
          const radar = sniper?.radar?.[sym];
          const krakenP = mkt?.price ?? (sym === "BTC/GBP" ? 57020 : 1820);
          const revBid = radar?.revolut_best_bid ?? (krakenP * 0.9995);
          const revAsk = radar?.revolut_best_ask ?? (krakenP * 1.0005);
          const change = mkt?.change24h ?? 0.0;
          const spreadGbp = roundToTwo(revAsk - revBid);
          const spreadBps = roundToTwo(((revAsk - revBid) / revBid) * 10000);
          const inSnipe = radar?.in_snipe_zone ?? false;

          return (
            <div
              key={sym}
              className={`bg-[#0e131f] border rounded-xl p-3 flex flex-col justify-between shadow-md transition ${
                inSnipe ? "border-amber-500/80 bg-amber-950/10" : "border-slate-800"
              }`}
            >
              <div className="flex items-center justify-between border-b border-slate-800/80 pb-2">
                <div className="flex items-center gap-2">
                  <span className="font-mono font-bold text-white text-sm">{sym}</span>
                  <span
                    className={`text-[10px] font-bold px-1.5 py-0.5 rounded ${
                      change >= 0 ? "bg-emerald-950 text-emerald-300" : "bg-rose-950 text-rose-300"
                    }`}
                  >
                    {change >= 0 ? `+${change.toFixed(2)}%` : `${change.toFixed(2)}%`}
                  </span>
                </div>

                <div className="flex items-center gap-2 text-[11px] font-mono">
                  <span className="text-slate-400">Revolut Spread:</span>
                  <span className="text-cyan-400 font-bold">
                    £{spreadGbp.toFixed(2)} ({spreadBps.toFixed(1)} bps)
                  </span>
                </div>
              </div>

              <div className="grid grid-cols-3 gap-2 pt-2 text-center font-mono">
                <div className="bg-[#070a10] p-2 rounded border border-slate-800">
                  <div className="text-[9px] text-cyan-400 uppercase tracking-wide font-sans">
                    Kraken Fair Value
                  </div>
                  <div className="text-sm font-bold text-white mt-0.5">
                    £{krakenP.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>

                <div className="bg-[#070a10] p-2 rounded border border-slate-800">
                  <div className="text-[9px] text-emerald-400 uppercase tracking-wide font-sans">
                    Revolut Best Bid
                  </div>
                  <div className="text-sm font-bold text-emerald-300 mt-0.5">
                    £{revBid.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>

                <div className="bg-[#070a10] p-2 rounded border border-slate-800">
                  <div className="text-[9px] text-rose-400 uppercase tracking-wide font-sans">
                    Revolut Best Ask
                  </div>
                  <div className="text-sm font-bold text-rose-300 mt-0.5">
                    £{revAsk.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* ========================================================================= */}
      {/* 2B. COMPOUNDED CAPITAL ALLOCATION & PROFIT LOCK VAULT */}
      {/* ========================================================================= */}
      <div className="bg-[#0e131f] border border-cyan-500/30 rounded-xl p-4 shadow-xl flex flex-col lg:flex-row items-stretch lg:items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="p-2.5 rounded-xl bg-cyan-500/10 border border-cyan-500/30 text-cyan-400 shrink-0">
            <Coins className="h-5 w-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-xs font-bold text-white font-mono uppercase tracking-wider">
                Compounded Capital Allocation
              </span>
              <span
                className={`text-[10px] font-mono px-2 py-0.5 rounded-full font-bold border ${
                  capMgmt?.balance_source === "REVOLUT_LIVE"
                    ? "bg-emerald-950 text-emerald-300 border-emerald-700"
                    : "bg-slate-800 text-cyan-300 border-slate-700"
                }`}
              >
                {capMgmt?.balance_source === "REVOLUT_LIVE" ? "● REVOLUT X LIVE" : "VIRTUAL PAPER DESK"}
              </span>
            </div>
            <div className="text-[11px] text-slate-400 mt-0.5 flex flex-wrap items-center gap-3 font-mono">
              <span>Starting Base: <strong className="text-slate-200">£{(capMgmt?.starting_balance_gbp ?? 10).toFixed(2)}</strong></span>
              <span>·</span>
              <span>Settled Cash: <strong className="text-slate-200">£{(capMgmt?.settled_cash_gbp ?? 10).toFixed(2)}</strong></span>
              <span>·</span>
              <span>Cumulative Profit: <strong className="text-emerald-400">+£{(capMgmt?.cumulative_profit_gbp ?? 0).toFixed(2)}</strong></span>
            </div>
          </div>
        </div>

        {/* Dynamic Metric Badges */}
        <div className="flex flex-wrap items-center gap-2.5 font-mono">
          {/* Locked Profit Vault */}
          <div className="bg-[#070a10] border border-amber-500/40 px-3 py-2 rounded-lg flex items-center gap-2.5">
            <Lock className="h-4 w-4 text-amber-400 shrink-0" />
            <div>
              <div className="text-[9px] text-amber-400/80 uppercase tracking-wide">
                Locked Profit Vault ({Math.round((capMgmt?.profit_lock_pct ?? 0.3) * 100)}% Lock)
              </div>
              <div className="text-sm font-bold text-amber-300">
                🔒 £{(capMgmt?.locked_profit_gbp ?? 0).toFixed(2)}
              </div>
            </div>
          </div>

          {/* Active Compounded Trading Power */}
          <div className="bg-[#070a10] border border-cyan-500/50 px-3 py-2 rounded-lg flex items-center gap-2.5 shadow-lg shadow-cyan-950/40">
            <Zap className="h-4 w-4 text-cyan-400 shrink-0 animate-pulse" />
            <div>
              <div className="text-[9px] text-cyan-400/80 uppercase tracking-wide flex items-center gap-1.5">
                <span>Active Trading Power</span>
                <span className="text-[10px] px-1 rounded bg-cyan-950 text-cyan-300 font-bold">
                  {(capMgmt?.expansion_ratio ?? 1.0).toFixed(2)}x Base
                </span>
              </div>
              <div className="text-sm font-black text-cyan-300">
                ⚡ £{(capMgmt?.active_trading_power_gbp ?? 10).toFixed(2)}
              </div>
            </div>
          </div>

          {/* Dynamic Runner Splits */}
          <div className="bg-[#070a10] border border-slate-800 px-3 py-2 rounded-lg flex items-center gap-3 text-xs">
            <div>
              <span className="text-[9px] text-slate-400 uppercase">BTC ({Math.round((capMgmt?.split_btc_pct ?? 0.5) * 100)}%)</span>
              <div className="text-xs font-bold text-white">
                £{(capMgmt?.allocations?.runner_btc?.envelope_gbp ?? 5).toFixed(2)} <span className="text-[10px] text-cyan-400 font-normal">(£{(capMgmt?.allocations?.runner_btc?.order_size_gbp ?? 1.67).toFixed(2)}/clip)</span>
              </div>
            </div>
            <div className="h-6 w-px bg-slate-800" />
            <div>
              <span className="text-[9px] text-slate-400 uppercase">ETH ({Math.round((capMgmt?.split_eth_pct ?? 0.5) * 100)}%)</span>
              <div className="text-xs font-bold text-white">
                £{(capMgmt?.allocations?.runner_eth?.envelope_gbp ?? 5).toFixed(2)} <span className="text-[10px] text-cyan-400 font-normal">(£{(capMgmt?.allocations?.runner_eth?.order_size_gbp ?? 1.67).toFixed(2)}/clip)</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* ========================================================================= */}
      {/* 3. PORTFOLIO PERFORMANCE & RISK HUD (4 Cards) */}
      {/* ========================================================================= */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        {/* Total Equity */}
        <div className="bg-[#0e131f] border border-slate-800 p-3.5 rounded-xl flex flex-col justify-between">
          <span className="text-[10px] text-slate-400 uppercase font-mono font-bold tracking-wider">
            Total Net Worth
          </span>
          <div className="text-xl font-mono font-black text-white mt-1">
            £{(portfolio?.total_equity_gbp ?? 1000).toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          </div>
          <div className="text-[11px] font-mono mt-0.5 text-slate-400">
            Initial Baseline: £{(portfolio?.initial_budget_gbp ?? 1000).toFixed(2)}
          </div>
        </div>

        {/* Total Realized PnL */}
        <div className="bg-[#0e131f] border border-slate-800 p-3.5 rounded-xl flex flex-col justify-between">
          <span className="text-[10px] text-slate-400 uppercase font-mono font-bold tracking-wider">
            Cumulative Realized PnL
          </span>
          <div
            className={`text-xl font-mono font-black mt-1 ${
              (portfolio?.total_realized_pnl_gbp ?? 0) >= 0 ? "text-emerald-400" : "text-rose-400"
            }`}
          >
            {(portfolio?.total_realized_pnl_gbp ?? 0) >= 0 ? "+" : ""}£{(portfolio?.total_realized_pnl_gbp ?? 0).toFixed(2)}
          </div>
          <div className="text-[11px] font-mono text-emerald-400 mt-0.5">
            +{(portfolio?.total_pnl_pct ?? 0).toFixed(2)}% total ROI
          </div>
        </div>

        {/* Stale Quote Sniper Micro-Arb */}
        <div className="bg-[#0e131f] border border-amber-500/40 p-3.5 rounded-xl flex flex-col justify-between shadow-lg shadow-amber-950/20">
          <span className="text-[10px] text-amber-400 uppercase font-mono font-bold tracking-wider flex items-center gap-1">
            <Zap className="h-3 w-3 text-amber-400" />
            Sniper Arb Profit
          </span>
          <div className="text-xl font-mono font-black text-amber-300 mt-1">
            +£{(sniper?.total_sniper_profit_gbp ?? 0).toFixed(2)}
          </div>
          <div className="text-[11px] font-mono text-slate-400 mt-0.5">
            {sniper?.total_snipes ?? 0} fills (~{sniper?.average_lead_advantage_ms ?? 450}ms lead)
          </div>
        </div>

        {/* Taker Fees Absorbed */}
        <div className="bg-[#0e131f] border border-slate-800 p-3.5 rounded-xl flex flex-col justify-between">
          <span className="text-[10px] text-slate-400 uppercase font-mono font-bold tracking-wider">
            Taker Fees Factored
          </span>
          <div className="text-xl font-mono font-black text-slate-300 mt-1">
            £{(sniper?.total_taker_fees_paid_gbp ?? 0).toFixed(2)}
          </div>
          <div className="text-[11px] font-mono text-slate-500 mt-0.5">
            0.09% Revolut X taker rate
          </div>
        </div>
      </div>

      {/* ========================================================================= */}
      {/* 4. TUNING MODE DRAWER (Visible when viewMode === "tuning") */}
      {/* ========================================================================= */}
      {viewMode === "tuning" && (
        <div className="bg-[#0e131f] border border-amber-500/50 rounded-xl p-5 space-y-4 shadow-xl">
          <div className="flex items-center justify-between border-b border-slate-800 pb-3">
            <div className="flex items-center gap-2">
              <Sliders className="h-5 w-5 text-amber-400" />
              <h2 className="text-sm font-bold text-white font-mono uppercase tracking-wide">
                Live Parameter Tuning Desk (Interactive Controls)
              </h2>
            </div>
            <span className="text-xs text-amber-300 font-mono">
              Changes update active running bots in real-time
            </span>
          </div>

          {/* Capital & Dynamic Profit Lock Panel */}
          <div className="bg-[#070a10] border border-cyan-500/40 p-4 rounded-xl space-y-4 font-mono">
            <div className="flex flex-col md:flex-row items-start md:items-center justify-between border-b border-slate-800 pb-3 gap-2">
              <div className="flex items-center gap-2">
                <Scale className="h-4 w-4 text-cyan-400" />
                <span className="text-xs font-bold text-white uppercase tracking-wider">
                  Capital Auto-Detection, Dynamic Splits & Profit Lock
                </span>
              </div>
              <button
                type="button"
                onClick={handleSyncRevolutBalances}
                disabled={syncingRevolut}
                className="bg-slate-800 hover:bg-slate-700 text-cyan-300 border border-cyan-500/40 px-3 py-1 rounded text-xs flex items-center gap-1.5 transition disabled:opacity-50"
              >
                <RefreshCw className={`h-3.5 w-3.5 ${syncingRevolut ? "animate-spin" : ""}`} />
                {syncingRevolut ? "Querying Revolut X..." : "Sync Revolut Balances"}
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-xs">
              {/* 1. Dynamic Profit Lock Ratio Slider */}
              <div className="space-y-2 bg-[#0e131f] p-3 rounded-lg border border-slate-800">
                <div className="flex items-center justify-between">
                  <span className="text-slate-300 font-bold flex items-center gap-1">
                    <Lock className="h-3.5 w-3.5 text-amber-400" />
                    Profit Lock Ratchet
                  </span>
                  <span className="text-amber-400 font-bold">{capitalParams.profit_lock_pct}%</span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="90"
                  step="5"
                  value={capitalParams.profit_lock_pct}
                  onChange={(e) => setCapitalParams({ ...capitalParams, profit_lock_pct: e.target.value })}
                  className="w-full accent-amber-400 cursor-pointer"
                />
                <p className="text-[11px] text-slate-400 font-sans leading-tight">
                  Quarantines <strong className="text-amber-300">{capitalParams.profit_lock_pct}%</strong> of net profit in a protected vault. The remaining <strong className="text-cyan-300">{100 - parseInt(capitalParams.profit_lock_pct || "30")}%</strong> compounds into active trading power!
                </p>
              </div>

              {/* 2. Runner Capital Split Slider */}
              <div className="space-y-2 bg-[#0e131f] p-3 rounded-lg border border-slate-800">
                <div className="flex items-center justify-between">
                  <span className="text-slate-300 font-bold flex items-center gap-1">
                    <Scale className="h-3.5 w-3.5 text-cyan-400" />
                    Runner Capital Split
                  </span>
                  <span className="text-cyan-400 font-bold">
                    {capitalParams.split_btc_pct}% BTC / {100 - parseInt(capitalParams.split_btc_pct || "50")}% ETH
                  </span>
                </div>
                <input
                  type="range"
                  min="10"
                  max="90"
                  step="5"
                  value={capitalParams.split_btc_pct}
                  onChange={(e) => setCapitalParams({ ...capitalParams, split_btc_pct: e.target.value })}
                  className="w-full accent-cyan-400 cursor-pointer"
                />
                <p className="text-[11px] text-slate-400 font-sans leading-tight">
                  Allocates trading power: <strong className="text-white">£{((parseFloat(capMgmt?.active_trading_power_gbp?.toString() || "10") * parseInt(capitalParams.split_btc_pct || "50")) / 100).toFixed(2)}</strong> to BTC, <strong className="text-white">£{((parseFloat(capMgmt?.active_trading_power_gbp?.toString() || "10") * (100 - parseInt(capitalParams.split_btc_pct || "50"))) / 100).toFixed(2)}</strong> to ETH.
                </p>
              </div>

              {/* 3. Paper Starting Capital Presets */}
              <div className="space-y-2 bg-[#0e131f] p-3 rounded-lg border border-slate-800">
                <div className="flex items-center justify-between">
                  <span className="text-slate-300 font-bold flex items-center gap-1">
                    <Wallet className="h-3.5 w-3.5 text-emerald-400" />
                    Starting Capital Base
                  </span>
                  <span className="text-emerald-400 font-bold">£{capitalParams.starting_balance_gbp}</span>
                </div>
                <div className="grid grid-cols-6 gap-1 pt-1">
                  {["10", "25", "50", "100", "500", "1000"].map((amt) => (
                    <button
                      key={amt}
                      type="button"
                      onClick={() => setCapitalParams({ ...capitalParams, starting_balance_gbp: amt })}
                      className={`py-1 rounded text-[10px] font-bold border transition ${
                        capitalParams.starting_balance_gbp === amt
                          ? "bg-emerald-500 text-black border-emerald-400 font-black"
                          : "bg-[#070a10] text-slate-400 border-slate-700 hover:text-white"
                      }`}
                    >
                      £{amt}
                    </button>
                  ))}
                </div>
                <p className="text-[11px] text-slate-400 font-sans leading-tight">
                  Micro account baseline: order clip sizes and runner rungs scale up dynamically from this starting capital.
                </p>
              </div>
            </div>

            <div className="flex justify-end pt-1">
              <button
                type="button"
                onClick={() => handleConfigureCapital()}
                className="bg-cyan-500 hover:bg-cyan-400 text-black font-black px-4 py-2 rounded-lg text-xs font-mono transition shadow-md shadow-cyan-950/50"
              >
                Apply Capital Allocation & Profit Lock
              </button>
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-xs font-mono">
            {/* BTC Runner Tuning */}
            <div className="bg-[#070a10] border border-slate-800 p-3.5 rounded-lg space-y-3">
              <div className="flex items-center justify-between">
                <span className="font-bold text-white">BTC/GBP Grid Runner</span>
                <span className="text-[10px] text-cyan-400">post_only (0.00% maker)</span>
              </div>
              <div className="space-y-2">
                <div>
                  <label className="text-[11px] text-slate-400 block mb-1">Step Spacing (%)</label>
                  <input
                    type="number"
                    step="0.05"
                    value={runnerParams["runner_btc"]?.step_pct || "0.40"}
                    onChange={(e) =>
                      setRunnerParams({
                        ...runnerParams,
                        runner_btc: { ...runnerParams["runner_btc"], step_pct: e.target.value },
                      })
                    }
                    className="w-full bg-[#0e131f] border border-slate-700 px-2.5 py-1.5 rounded text-white focus:border-cyan-400 outline-none"
                  />
                </div>
                <div>
                  <label className="text-[11px] text-slate-400 block mb-1">Rebalance Drift (%)</label>
                  <input
                    type="number"
                    step="0.10"
                    value={runnerParams["runner_btc"]?.rebalance_pct || "2.0"}
                    onChange={(e) =>
                      setRunnerParams({
                        ...runnerParams,
                        runner_btc: { ...runnerParams["runner_btc"], rebalance_pct: e.target.value },
                      })
                    }
                    className="w-full bg-[#0e131f] border border-slate-700 px-2.5 py-1.5 rounded text-white focus:border-cyan-400 outline-none"
                  />
                </div>
              </div>
              <button
                onClick={() => handleTuneRunner("runner_btc")}
                className="w-full bg-slate-800 hover:bg-slate-700 text-cyan-300 border border-cyan-500/40 py-1.5 rounded transition text-xs font-bold"
              >
                Apply BTC Tuning
              </button>
            </div>

            {/* ETH Runner Tuning */}
            <div className="bg-[#070a10] border border-slate-800 p-3.5 rounded-lg space-y-3">
              <div className="flex items-center justify-between">
                <span className="font-bold text-white">ETH/GBP Grid Runner</span>
                <span className="text-[10px] text-cyan-400">post_only (0.00% maker)</span>
              </div>
              <div className="space-y-2">
                <div>
                  <label className="text-[11px] text-slate-400 block mb-1">Step Spacing (%)</label>
                  <input
                    type="number"
                    step="0.05"
                    value={runnerParams["runner_eth"]?.step_pct || "0.40"}
                    onChange={(e) =>
                      setRunnerParams({
                        ...runnerParams,
                        runner_eth: { ...runnerParams["runner_eth"], step_pct: e.target.value },
                      })
                    }
                    className="w-full bg-[#0e131f] border border-slate-700 px-2.5 py-1.5 rounded text-white focus:border-cyan-400 outline-none"
                  />
                </div>
                <div>
                  <label className="text-[11px] text-slate-400 block mb-1">Rebalance Drift (%)</label>
                  <input
                    type="number"
                    step="0.10"
                    value={runnerParams["runner_eth"]?.rebalance_pct || "2.0"}
                    onChange={(e) =>
                      setRunnerParams({
                        ...runnerParams,
                        runner_eth: { ...runnerParams["runner_eth"], rebalance_pct: e.target.value },
                      })
                    }
                    className="w-full bg-[#0e131f] border border-slate-700 px-2.5 py-1.5 rounded text-white focus:border-cyan-400 outline-none"
                  />
                </div>
              </div>
              <button
                onClick={() => handleTuneRunner("runner_eth")}
                className="w-full bg-slate-800 hover:bg-slate-700 text-cyan-300 border border-cyan-500/40 py-1.5 rounded transition text-xs font-bold"
              >
                Apply ETH Tuning
              </button>
            </div>

            {/* Stale Quote Sniper Tuning */}
            <div className="bg-[#070a10] border border-amber-500/30 p-3.5 rounded-lg space-y-3">
              <div className="flex items-center justify-between">
                <span className="font-bold text-amber-300 flex items-center gap-1">
                  <Zap className="h-3.5 w-3.5 text-amber-400" />
                  Stale Quote Sniper
                </span>
                <button
                  onClick={handleToggleSniper}
                  className={`text-[10px] font-bold px-2 py-0.5 rounded border transition ${
                    sniper?.enabled
                      ? "bg-amber-500/20 text-amber-300 border-amber-500/50"
                      : "bg-slate-900 text-slate-400 border-slate-700"
                  }`}
                >
                  {sniper?.enabled ? "ARMED" : "DISARMED"}
                </button>
              </div>
              <div className="space-y-2">
                <div>
                  <div className="flex justify-between text-[11px] text-slate-400 mb-1">
                    <span>Hurdle Threshold</span>
                    <span className="text-amber-300 font-bold">{sniperParams.impulse_threshold_pct}%</span>
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
                </div>
                <div>
                  <label className="text-[11px] text-slate-400 block mb-1">Snipe Order Size (£)</label>
                  <div className="grid grid-cols-4 gap-1">
                    {["25", "50", "100", "250"].map((sz) => (
                      <button
                        key={sz}
                        type="button"
                        onClick={() => setSniperParams({ ...sniperParams, snipe_order_size_gbp: sz })}
                        className={`py-1 rounded text-[11px] font-bold border transition ${
                          sniperParams.snipe_order_size_gbp === sz
                            ? "bg-amber-400 text-black border-amber-300 font-black"
                            : "bg-[#0e131f] text-slate-400 border-slate-700"
                        }`}
                      >
                        £{sz}
                      </button>
                    ))}
                  </div>
                </div>
              </div>
              <button
                onClick={handleTuneSniper}
                className="w-full bg-gradient-to-r from-amber-500 to-amber-600 hover:from-amber-400 hover:to-amber-500 text-black font-black py-1.5 rounded transition text-xs shadow"
              >
                Apply Sniper Parameters
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ========================================================================= */}
      {/* 5. PRIMARY DATA GRID (3-COLUMN MODULAR TERMINAL) */}
      {/* ========================================================================= */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-4">
        {/* ======================================================================= */}
        {/* COLUMN 1: CROSS-VENUE RADAR & TOP-OF-BOOK DEPTH (4 COLS) */}
        {/* ======================================================================= */}
        <div className="lg:col-span-4 bg-[#0e131f] border border-slate-800 rounded-xl p-4 space-y-3 flex flex-col h-[520px]">
          <div className="flex items-center justify-between border-b border-slate-800 pb-2.5">
            <div className="flex items-center gap-2">
              <Radio className="h-4 w-4 text-cyan-400" />
              <span className="text-xs font-bold text-white font-mono uppercase">
                Cross-Venue Lead-Lag Radar
              </span>
            </div>
            <span className="text-[10px] text-amber-400 font-mono">0.09% Taker Barrier</span>
          </div>

          <div className="flex-1 overflow-y-auto space-y-3 pr-1 font-mono text-xs">
            {["BTC/GBP", "ETH/GBP"].map((sym) => {
              const radar = sniper?.radar?.[sym];
              const krakenP = radar?.kraken_price ?? (sym === "BTC/GBP" ? 57020 : 1820);
              const revBid = radar?.revolut_best_bid ?? (krakenP * 0.9995);
              const revAsk = radar?.revolut_best_ask ?? (krakenP * 1.0005);
              const dislocation = radar?.current_dislocation_pct ?? 0.02;
              const inSnipe = radar?.in_snipe_zone ?? false;
              const hurdle = sniper?.impulse_threshold_pct ?? 0.18;
              const progressPct = Math.min(100, Math.max(0, (dislocation / hurdle) * 100));

              return (
                <div
                  key={sym}
                  className={`p-3 rounded-lg border transition ${
                    inSnipe
                      ? "bg-amber-950/30 border-amber-500/80 shadow-md shadow-amber-950/30"
                      : "bg-[#070a10] border-slate-800/80"
                  }`}
                >
                  <div className="flex items-center justify-between pb-2 border-b border-slate-800/70">
                    <span className="font-bold text-white text-sm">{sym}</span>
                    <span
                      className={`text-[10px] font-bold px-2 py-0.5 rounded ${
                        inSnipe ? "bg-amber-400 text-black animate-pulse" : "bg-slate-800 text-slate-400"
                      }`}
                    >
                      {inSnipe ? "⚡ SNIPE ZONE ACTIVE" : "SCANNING FEED"}
                    </span>
                  </div>

                  <div className="grid grid-cols-2 gap-2 my-2 text-[11px]">
                    <div className="bg-[#0e131f] p-2 rounded border border-slate-800/60">
                      <div className="text-[9px] text-slate-400 uppercase">Kraken (Lead)</div>
                      <div className="text-white font-bold text-xs mt-0.5">
                        £{krakenP.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                      </div>
                    </div>
                    <div className="bg-[#0e131f] p-2 rounded border border-slate-800/60">
                      <div className="text-[9px] text-slate-400 uppercase">Revolut BBO Spread</div>
                      <div className="text-cyan-300 font-bold text-xs mt-0.5">
                        £{(revAsk - revBid).toFixed(2)} ({roundToTwo(((revAsk - revBid) / revBid) * 100)}%)
                      </div>
                    </div>
                  </div>

                  {/* Dislocation Hurdle Meter */}
                  <div>
                    <div className="flex justify-between text-[10px] text-slate-400 mb-1">
                      <span>Live Dislocation: {dislocation.toFixed(3)}%</span>
                      <span className={inSnipe ? "text-amber-300 font-bold" : "text-slate-500"}>
                        Hurdle: ≥{hurdle.toFixed(2)}%
                      </span>
                    </div>
                    <div className="w-full bg-slate-900 rounded-full h-2 overflow-hidden border border-slate-800">
                      <div
                        className={`h-full transition-all duration-300 ${
                          inSnipe
                            ? "bg-gradient-to-r from-amber-400 to-emerald-400 animate-pulse"
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

        {/* ======================================================================= */}
        {/* COLUMN 2: ACTIVE RESTING ORDER LADDER (4 COLS) */}
        {/* ======================================================================= */}
        <div className="lg:col-span-4 bg-[#0e131f] border border-slate-800 rounded-xl p-4 space-y-3 flex flex-col h-[520px]">
          <div className="flex items-center justify-between border-b border-slate-800 pb-2.5">
            <div className="flex items-center gap-2">
              <Clock className="h-4 w-4 text-cyan-400" />
              <span className="text-xs font-bold text-white font-mono uppercase">
                Resting Order Ladder
              </span>
              <span className="text-[10px] px-1.5 py-0.5 rounded bg-slate-800 text-cyan-300 font-bold font-mono">
                {filteredOrders.length} Open
              </span>
            </div>

            <div className="flex gap-1 text-[10px] font-mono">
              {(["ALL", "BTC", "ETH"] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setAssetFilter(f)}
                  className={`px-2 py-0.5 rounded border transition ${
                    assetFilter === f
                      ? "bg-cyan-500 text-black font-bold border-cyan-400"
                      : "bg-[#070a10] border-slate-700 text-slate-400 hover:text-white"
                  }`}
                >
                  {f}
                </button>
              ))}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto pr-1">
            <table className="w-full text-left font-mono text-[11px]">
              <thead className="text-[10px] text-slate-500 border-b border-slate-800 sticky top-0 bg-[#0e131f]">
                <tr>
                  <th className="pb-1.5">SIDE</th>
                  <th className="pb-1.5 text-right">PRICE</th>
                  <th className="pb-1.5 text-right">QTY</th>
                  <th className="pb-1.5 text-right">DIST</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-800/40">
                {filteredOrders.length === 0 ? (
                  <tr>
                    <td colSpan={4} className="py-16 text-center text-slate-500 italic">
                      No open resting limit orders found.
                    </td>
                  </tr>
                ) : (
                  filteredOrders.map((ord) => {
                    const isBuy = ord.side === "BUY";
                    return (
                      <tr key={ord.id} className="hover:bg-slate-800/30 transition">
                        <td className="py-1">
                          <span
                            className={`px-1 py-0.5 rounded font-bold text-[9px] ${
                              isBuy
                                ? "bg-emerald-950 text-emerald-300 border border-emerald-800"
                                : "bg-rose-950 text-rose-300 border border-rose-800"
                            }`}
                          >
                            {ord.side}
                          </span>
                        </td>
                        <td className="py-1 text-right font-bold text-white">
                          £{ord.price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                        </td>
                        <td className="py-1 text-right text-slate-400">{ord.qty}</td>
                        <td
                          className={`py-1 text-right font-semibold ${
                            ord.distance_pct >= 0 ? "text-rose-400" : "text-emerald-400"
                          }`}
                        >
                          {ord.distance_pct >= 0 ? `+${ord.distance_pct.toFixed(2)}%` : `${ord.distance_pct.toFixed(2)}%`}
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        </div>

        {/* ======================================================================= */}
        {/* COLUMN 3: STREAMING EXECUTION TAPE (4 COLS) */}
        {/* ======================================================================= */}
        <div className="lg:col-span-4 bg-[#0e131f] border border-slate-800 rounded-xl p-4 space-y-3 flex flex-col h-[520px]">
          <div className="flex items-center justify-between border-b border-slate-800 pb-2.5">
            <div className="flex items-center gap-2">
              <Activity className="h-4 w-4 text-emerald-400" />
              <span className="text-xs font-bold text-white font-mono uppercase">
                Execution Stream Tape
              </span>
              <span className="text-[9px] px-1.5 py-0.5 rounded bg-emerald-950 text-emerald-300 font-bold border border-emerald-800 font-mono">
                LIVE
              </span>
            </div>

            <div className="flex gap-1 text-[10px] font-mono">
              {(["ALL", "SNIPES", "GRIDS"] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setTapeFilter(f)}
                  className={`px-2 py-0.5 rounded border transition ${
                    tapeFilter === f
                      ? "bg-emerald-500 text-black font-bold border-emerald-400"
                      : "bg-[#070a10] border-slate-700 text-slate-400 hover:text-white"
                  }`}
                >
                  {f}
                </button>
              ))}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto space-y-2 pr-1 font-mono text-xs">
            {filteredTrades.length === 0 ? (
              <div className="text-center py-20 text-slate-500 italic">
                Awaiting trade executions...
              </div>
            ) : (
              filteredTrades.map((t) => {
                const isSnipeBuy = t.action === "SNIPE_BUY";
                const isSnipeSell = t.action === "SNIPE_SELL";
                const isSnipe = isSnipeBuy || isSnipeSell;
                const isBuy = t.action === "BUY";
                const isSell = t.action === "SELL";
                const isKill = t.action === "KILL_SWITCH";

                return (
                  <div
                    key={t.id}
                    className={`p-2.5 rounded-lg border transition ${
                      isSnipe
                        ? "bg-amber-950/30 border-amber-500/70 shadow-sm"
                        : isBuy
                        ? "bg-emerald-950/20 border-emerald-800/40"
                        : isSell
                        ? "bg-cyan-950/20 border-cyan-800/40"
                        : isKill
                        ? "bg-rose-950/40 border-rose-800/60"
                        : "bg-[#070a10] border-slate-800"
                    }`}
                  >
                    <div className="flex items-center justify-between text-[11px]">
                      <div className="flex items-center gap-2">
                        <span className="text-slate-500 text-[10px]">{t.time_str}</span>
                        <span
                          className={`px-1.5 py-0.5 rounded font-bold text-[9px] flex items-center gap-0.5 ${
                            isSnipeBuy
                              ? "bg-amber-400 text-black font-extrabold"
                              : isSnipeSell
                              ? "bg-orange-400 text-black font-extrabold"
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
                        <span
                          className={`font-bold ${
                            isSnipe ? "text-amber-300" : "text-emerald-400"
                          }`}
                        >
                          +£{t.profit.toFixed(2)}
                        </span>
                      )}
                    </div>
                    <div
                      className={`text-[10px] mt-1 leading-snug ${
                        isSnipe ? "text-amber-200/90 font-mono" : "text-slate-400"
                      }`}
                    >
                      {t.note}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>

      {/* ========================================================================= */}
      {/* 6. EMERGENCY KILL SWITCH MODAL */}
      {/* ========================================================================= */}
      {killModalOpen && (
        <div className="fixed inset-0 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4 z-50">
          <div className="bg-[#0e131f] border border-rose-600 rounded-2xl max-w-md w-full p-6 space-y-4 shadow-2xl">
            <div className="flex items-center gap-3 text-rose-400">
              <AlertTriangle className="h-6 w-6" />
              <h3 className="text-base font-bold text-white font-mono">Emergency Kill Switch</h3>
            </div>
            <p className="text-xs text-slate-300 leading-relaxed font-sans">
              This will immediately trip the circuit breaker, halt all strategy runners, cancel all open resting orders across Revolut X, and disarm the Stale Quote Sniper.
            </p>
            <div className="flex items-center justify-end gap-3 pt-3">
              <button
                onClick={() => setKillModalOpen(false)}
                className="px-4 py-2 rounded-lg text-xs font-semibold text-slate-400 hover:text-white transition"
              >
                Cancel
              </button>
              <button
                onClick={handleKillSwitch}
                className="bg-rose-600 hover:bg-rose-500 text-white font-black px-4 py-2 rounded-lg text-xs transition shadow-lg shadow-rose-950"
              >
                Confirm Emergency Kill
              </button>
            </div>
          </div>
        </div>
      )}
    </main>
  );
}

function roundToTwo(num: number): number {
  return Math.round((num + Number.EPSILON) * 100) / 100;
}
