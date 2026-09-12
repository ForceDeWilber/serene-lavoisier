"use client";

import React, { useEffect, useState, useRef } from "react";
import Link from "next/link";
import {
  Activity,
  Play,
  Pause,
  RefreshCw,
  TrendingUp,
  AlertTriangle,
  Sliders,
  Wallet,
  Zap,
  Clock,
  SlidersHorizontal,
  Wifi,
  WifiOff,
  Lock,
  Scale,
  Power,
  ChevronRight,
  Shield,
  ArrowUpRight,
  ArrowDownRight,
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
  mode?: "paper" | "live";
  is_live?: boolean;
  status?: string;
  status_message?: string;
  authenticated?: boolean;
  can_trade?: boolean;
  latency_ms?: number;
  host_mode?: string;
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
  // Completely separate state stores: Paper and Live are 2 strictly independent entities
  const [paperTelemetry, setPaperTelemetry] = useState<TelemetryPayload | null>(null);
  const [liveTelemetry, setLiveTelemetry] = useState<TelemetryPayload | null>(null);

  const [connected, setConnected] = useState<boolean>(false);
  const [lastSeenTs, setLastSeenTs] = useState<number | null>(null);
  const [isFeedStale, setIsFeedStale] = useState<boolean>(false);
  const [statusMessage, setStatusMessage] = useState<string>("");

  const [activeMode, setActiveMode] = useState<"paper" | "live">("paper");
  const [liveConfirmModalOpen, setLiveConfirmModalOpen] = useState<boolean>(false);
  const [verifyingLive, setVerifyingLive] = useState<boolean>(false);

  const [viewMode, setViewMode] = useState<"monitor" | "tuning">("monitor");
  const [assetFilter, setAssetFilter] = useState<"ALL" | "BTC" | "ETH">("ALL");
  const [tapeFilter, setTapeFilter] = useState<"ALL" | "SNIPES" | "GRIDS">("ALL");

  const [killModalOpen, setKillModalOpen] = useState<boolean>(false);

  const [runnerParams, setRunnerParams] = useState<Record<string, { step_pct: string; rebalance_pct: string }>>({
    runner_btc: { step_pct: "0.40", rebalance_pct: "2.0" },
    runner_eth: { step_pct: "0.40", rebalance_pct: "2.0" },
  });

  const [sniperParams, setSniperParams] = useState({
    impulse_threshold_pct: "0.18",
    snipe_order_size_gbp: "50",
    min_net_edge_pct: "0.05",
  });

  const [capitalParams, setCapitalParams] = useState({
    profit_lock_pct: "30",
    split_btc_pct: "50",
    starting_balance_gbp: "10",
  });
  const [syncingRevolut, setSyncingRevolut] = useState<boolean>(false);

  const lastSeenRef = useRef<number>(Date.now());
  const stalenessCheckTimer = useRef<NodeJS.Timeout | null>(null);

  const REFRESH_INTERVAL_MS = 1000;
  const STALE_THRESHOLD_MS = 2500;

  // Active telemetry is strictly partitioned by the current mode
  const telemetry = activeMode === "live" ? liveTelemetry : paperTelemetry;

  const fetchTelemetry = async (targetMode: "paper" | "live") => {
    try {
      const res = await fetch(`/api/proxy/telemetry?mode=${targetMode}`);
      if (res.ok) {
        const data: TelemetryPayload = await res.json();
        if (targetMode === "live") {
          setLiveTelemetry(data);
        } else {
          setPaperTelemetry(data);
        }
        setConnected(true);
        lastSeenRef.current = Date.now();
        setLastSeenTs(lastSeenRef.current);
        setIsFeedStale(false);
      }
    } catch {}
  };

  useEffect(() => {
    if (typeof window !== "undefined") {
      const params = new URLSearchParams(window.location.search);
      const m = params.get("mode");
      if (m === "live") {
        setActiveMode("live");
      } else {
        setActiveMode("paper");
      }
    }
  }, []);

  const handleSwitchMode = (targetMode: "paper" | "live") => {
    if (targetMode === activeMode) return;
    if (targetMode === "live") {
      setLiveConfirmModalOpen(true);
    } else {
      performSwitchMode("paper");
    }
  };

  const performSwitchMode = (targetMode: "paper" | "live") => {
    setActiveMode(targetMode);
    if (typeof window !== "undefined") {
      const url = new URL(window.location.href);
      url.searchParams.set("mode", targetMode);
      window.history.pushState({}, "", url.toString());
    }
    // Fetch immediately for the selected mode
    fetchTelemetry(targetMode);
  };

  const handleVerifyLiveCredentials = async () => {
    setVerifyingLive(true);
    try {
      const res = await fetch("/api/proxy/live/diagnostics");
      const data = await res.json();
      if (data.can_trade) {
        setStatusMessage("Revolut X credentials verified · Ready for live execution");
      } else {
        setStatusMessage(`Live check: ${data.message || data.status}`);
      }
      fetchTelemetry("live");
    } catch {
      setStatusMessage("Failed to connect to Live diagnostics");
    } finally {
      setVerifyingLive(false);
    }
  };

  useEffect(() => {
    let active = true;

    // Fetch immediately on mode change
    fetchTelemetry(activeMode);
    const pollInterval = setInterval(() => {
      if (active) {
        fetchTelemetry(activeMode);
      }
    }, REFRESH_INTERVAL_MS);

    stalenessCheckTimer.current = setInterval(() => {
      const elapsed = Date.now() - lastSeenRef.current;
      setIsFeedStale(elapsed > STALE_THRESHOLD_MS);
    }, 500);

    return () => {
      active = false;
      clearInterval(pollInterval);
      if (stalenessCheckTimer.current) clearInterval(stalenessCheckTimer.current);
    };
  }, [activeMode]);

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

  const handleTogglePause = async (runnerId: string, currentlyPaused: boolean) => {
    const action = currentlyPaused ? "resume" : "pause";
    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/${action}?mode=${activeMode}`, {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage(`Runner ${runnerId} ${action}d`);
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleTuneRunner = async (runnerId: string) => {
    const params = runnerParams[runnerId];
    if (!params) return;

    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/tune?mode=${activeMode}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          step_pct: parseFloat(params.step_pct) / 100,
          rebalance_threshold_pct: parseFloat(params.rebalance_pct) / 100,
        }),
      });
      if (res.ok) {
        setStatusMessage(`Parameters updated for ${runnerId}`);
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleTuneSniper = async () => {
    try {
      const res = await fetch(`/api/proxy/sniper/tune?mode=${activeMode}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          impulse_threshold_pct: parseFloat(sniperParams.impulse_threshold_pct),
          snipe_order_size_gbp: parseFloat(sniperParams.snipe_order_size_gbp),
          min_net_edge_pct: parseFloat(sniperParams.min_net_edge_pct),
        }),
      });
      if (res.ok) {
        setStatusMessage("Sniper parameters updated");
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleToggleSniper = async () => {
    const nextState = !telemetry?.sniper?.enabled;
    try {
      const res = await fetch(`/api/proxy/sniper/${nextState ? "arm" : "disarm"}?mode=${activeMode}`, {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage(`Sniper ${nextState ? "armed" : "disarmed"}`);
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleKillSwitch = async () => {
    try {
      const res = await fetch(`/api/proxy/circuit-breaker/kill?mode=${activeMode}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: "Manual halt triggered from control desk" }),
      });
      if (res.ok) {
        setStatusMessage("Circuit breaker tripped · All orders halted");
        setKillModalOpen(false);
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleResetCircuitBreaker = async () => {
    try {
      const res = await fetch(`/api/proxy/circuit-breaker/reset?mode=${activeMode}`, {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage("Circuit breaker reset");
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleConfigureCapital = async (updates?: Partial<{ profit_lock_pct: number; split_btc_pct: number; starting_balance_gbp: number }>) => {
    const lockVal = updates?.profit_lock_pct !== undefined ? updates.profit_lock_pct : parseFloat(capitalParams.profit_lock_pct) / 100;
    const splitVal = updates?.split_btc_pct !== undefined ? updates.split_btc_pct : parseFloat(capitalParams.split_btc_pct) / 100;
    const startingVal = updates?.starting_balance_gbp !== undefined ? updates.starting_balance_gbp : parseFloat(capitalParams.starting_balance_gbp);

    try {
      const res = await fetch(`/api/proxy/capital/configure?mode=${activeMode}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          profit_lock_pct: lockVal,
          split_btc_pct: splitVal,
          starting_balance_gbp: startingVal,
        }),
      });
      if (res.ok) {
        setStatusMessage("Capital allocation updated");
        fetchTelemetry(activeMode);
      }
    } catch {}
  };

  const handleSyncRevolutBalances = async () => {
    setSyncingRevolut(true);
    try {
      const res = await fetch("/api/proxy/capital/sync-revolut", { method: "POST" });
      if (res.ok) {
        setStatusMessage("Balances synchronized");
        fetchTelemetry(activeMode);
      }
    } catch {} finally {
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
  const restingOrders = telemetry?.resting_orders || [];
  const liveTrades = telemetry?.live_trades || [];

  const filteredOrders = restingOrders.filter((o) => {
    if (assetFilter === "BTC") return o.symbol.includes("BTC");
    if (assetFilter === "ETH") return o.symbol.includes("ETH");
    return true;
  });

  const filteredTrades = liveTrades.filter((t) => {
    if (tapeFilter === "SNIPES") return t.action.includes("SNIPE");
    if (tapeFilter === "GRIDS") return t.action === "BUY" || t.action === "SELL";
    return true;
  });

  const liveUnconfigured = activeMode === "live" && (!telemetry?.authenticated || telemetry?.status === "UNCONFIGURED" || telemetry?.status === "AUTH_ERROR");
  const liveUnfunded = activeMode === "live" && telemetry?.authenticated && (telemetry?.status === "INSUFFICIENT_FUNDS" || (telemetry?.balances?.GBP ?? 0) <= 0.0);

  return (
    <main className="min-h-screen bg-[#0d1117] text-[#e6edf3] p-4 md:p-6 space-y-4 max-w-7xl mx-auto">
      {/* 1. Header */}
      <header className="bg-[#161b22] border border-[#30363d] rounded-xl px-4 py-3 flex flex-col md:flex-row md:items-center justify-between gap-3 shadow-sm">
        <div className="flex items-center gap-3">
          <div className="w-7 h-7 rounded-lg bg-[#21262d] border border-[#30363d] flex items-center justify-center text-[#8b949e]">
            <Activity className="w-4 h-4 text-[#7d8590]" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold tracking-tight text-[#f0f6fc]">
                Serene Lavoisier
              </span>
              <span className="text-[11px] font-mono text-[#8b949e] px-1.5 py-0.2 rounded bg-[#21262d] border border-[#30363d]">
                {activeMode === "live" ? "Live Environment" : "Paper Sandbox"}
              </span>
            </div>
          </div>
        </div>

        {/* Controls */}
        <div className="flex items-center gap-2.5 flex-wrap">
          {/* Segmented Mode Switcher */}
          <div className="bg-[#0d1117] border border-[#30363d] p-0.5 rounded-lg flex items-center text-xs">
            <button
              onClick={() => handleSwitchMode("paper")}
              className={`px-2.5 py-1 rounded-md transition font-medium text-xs flex items-center gap-1.5 ${
                activeMode === "paper"
                  ? "bg-[#21262d] text-[#f0f6fc] shadow-sm"
                  : "text-[#8b949e] hover:text-[#c9d1d9]"
              }`}
            >
              <span className="w-1.5 h-1.5 rounded-full bg-[#3fb950]" />
              <span>Paper</span>
            </button>
            <button
              onClick={() => handleSwitchMode("live")}
              className={`px-2.5 py-1 rounded-md transition font-medium text-xs flex items-center gap-1.5 ${
                activeMode === "live"
                  ? "bg-[#21262d] text-[#f85149] shadow-sm font-semibold"
                  : "text-[#8b949e] hover:text-[#f85149]"
              }`}
            >
              <span className="w-1.5 h-1.5 rounded-full bg-[#f85149]" />
              <span>Live</span>
            </button>
          </div>

          {/* Feed Status Indicator */}
          <div className="text-xs font-mono text-[#8b949e] flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-[#0d1117] border border-[#30363d]">
            <span className={`w-1.5 h-1.5 rounded-full ${connected && !isFeedStale ? "bg-[#3fb950]" : "bg-[#d29922]"}`} />
            <span>{connected && !isFeedStale ? `Feed · ${telemetry?.latency_ms || 14}ms` : "Stale feed"}</span>
          </div>

          {/* View Mode Switcher */}
          <div className="bg-[#0d1117] border border-[#30363d] p-0.5 rounded-lg flex items-center text-xs">
            <button
              onClick={() => setViewMode("monitor")}
              className={`px-2.5 py-1 rounded-md transition text-xs ${
                viewMode === "monitor" ? "bg-[#21262d] text-[#f0f6fc]" : "text-[#8b949e] hover:text-[#c9d1d9]"
              }`}
            >
              Monitor
            </button>
            <button
              onClick={() => setViewMode("tuning")}
              className={`px-2.5 py-1 rounded-md transition text-xs ${
                viewMode === "tuning" ? "bg-[#21262d] text-[#f0f6fc]" : "text-[#8b949e] hover:text-[#c9d1d9]"
              }`}
            >
              Tuning
            </button>
          </div>

          {/* Circuit Breaker Pill */}
          <div className={`text-xs px-2.5 py-1 rounded-lg border font-mono flex items-center gap-1.5 ${
            cbTripped
              ? "bg-[#f85149]/10 text-[#f85149] border-[#f85149]/40"
              : "bg-[#0d1117] text-[#8b949e] border-[#30363d]"
          }`}>
            <Shield className="w-3 h-3" />
            <span>{cbTripped ? "Breaker Tripped" : "Normal"}</span>
            {cbTripped && (
              <button
                onClick={handleResetCircuitBreaker}
                className="underline ml-1 hover:text-white"
              >
                Reset
              </button>
            )}
          </div>

          {/* Emergency Halt Button */}
          <button
            onClick={() => setKillModalOpen(true)}
            className="text-xs text-[#f85149] hover:text-white bg-[#f85149]/10 hover:bg-[#f85149]/20 border border-[#f85149]/30 px-2.5 py-1 rounded-lg transition font-medium flex items-center gap-1"
          >
            <Power className="w-3 h-3" />
            <span>Halt</span>
          </button>

          {/* Lock Terminal */}
          <button
            onClick={handleLogout}
            title="Lock session"
            className="text-[#8b949e] hover:text-[#f0f6fc] bg-[#0d1117] border border-[#30363d] hover:border-[#8b949e] p-1.5 rounded-lg transition"
          >
            <Lock className="w-3.5 h-3.5" />
          </button>
        </div>
      </header>

      {/* 2. Notification / Diagnostics Banner */}
      {statusMessage && (
        <div className="bg-[#161b22] border border-[#30363d] text-[#e6edf3] px-3.5 py-2 rounded-lg text-xs font-mono flex items-center justify-between">
          <span>{statusMessage}</span>
          <button onClick={() => setStatusMessage("")} className="text-[#8b949e] hover:text-white">
            Dismiss
          </button>
        </div>
      )}

      {/* Understated Live Mode Diagnostics (Only when action needed) */}
      {liveUnconfigured && (
        <div className="bg-[#161b22] border border-[#f85149]/40 px-3.5 py-2 rounded-lg text-xs flex items-center justify-between gap-3 text-[#f0f6fc]">
          <div className="flex items-center gap-2 font-mono">
            <span className="w-1.5 h-1.5 rounded-full bg-[#f85149]" />
            <span className="text-[#f85149]">Live Unauthenticated</span>
            <span className="text-[#8b949e]">·</span>
            <span className="text-[#8b949e]">Revolut X API key or private key missing on host. Real orders blocked.</span>
          </div>
          <button
            onClick={handleVerifyLiveCredentials}
            disabled={verifyingLive}
            className="px-2.5 py-1 rounded bg-[#21262d] hover:bg-[#30363d] border border-[#30363d] text-xs font-mono text-[#c9d1d9] transition flex items-center gap-1.5 flex-shrink-0"
          >
            <RefreshCw className={`w-3 h-3 ${verifyingLive ? "animate-spin" : ""}`} />
            <span>Verify</span>
          </button>
        </div>
      )}

      {liveUnfunded && (
        <div className="bg-[#161b22] border border-[#d29922]/40 px-3.5 py-2 rounded-lg text-xs flex items-center justify-between gap-3 text-[#f0f6fc]">
          <div className="flex items-center gap-2 font-mono">
            <span className="w-1.5 h-1.5 rounded-full bg-[#d29922]" />
            <span className="text-[#d29922]">Account Unfunded</span>
            <span className="text-[#8b949e]">·</span>
            <span className="text-[#8b949e]">Available balance is £0.00 GBP. Deposit funds on Revolut X to resume trading.</span>
          </div>
          <button
            onClick={handleVerifyLiveCredentials}
            disabled={verifyingLive}
            className="px-2.5 py-1 rounded bg-[#21262d] hover:bg-[#30363d] border border-[#30363d] text-xs font-mono text-[#c9d1d9] transition flex items-center gap-1.5 flex-shrink-0"
          >
            <RefreshCw className={`w-3 h-3 ${verifyingLive ? "animate-spin" : ""}`} />
            <span>Check Balance</span>
          </button>
        </div>
      )}

      {/* 3. Ticker Bar */}
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

          return (
            <div key={sym} className="bg-[#161b22] border border-[#30363d] rounded-xl p-3.5 flex flex-col justify-between shadow-sm">
              <div className="flex items-center justify-between border-b border-[#30363d]/60 pb-2.5">
                <div className="flex items-center gap-2">
                  <span className="font-mono font-semibold text-sm text-[#f0f6fc]">{sym}</span>
                  <span className={`text-[11px] font-mono px-1.5 py-0.5 rounded ${
                    change >= 0 ? "text-[#3fb950] bg-[#3fb950]/10" : "text-[#f85149] bg-[#f85149]/10"
                  }`}>
                    {change >= 0 ? `+${change.toFixed(2)}%` : `${change.toFixed(2)}%`}
                  </span>
                </div>
                <div className="text-xs font-mono text-[#8b949e]">
                  Spread: <span className="text-[#c9d1d9]">£{spreadGbp.toFixed(2)} ({spreadBps.toFixed(1)} bps)</span>
                </div>
              </div>

              <div className="grid grid-cols-3 gap-2 pt-2.5 text-center font-mono text-xs">
                <div className="bg-[#0d1117] p-2 rounded-lg border border-[#30363d]/70">
                  <div className="text-[10px] text-[#8b949e] uppercase tracking-wider font-sans">Oracle Price</div>
                  <div className="text-sm font-semibold text-[#f0f6fc] mt-0.5">
                    £{krakenP.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>

                <div className="bg-[#0d1117] p-2 rounded-lg border border-[#30363d]/70">
                  <div className="text-[10px] text-[#8b949e] uppercase tracking-wider font-sans">Revolut Bid</div>
                  <div className="text-sm font-semibold text-[#3fb950] mt-0.5">
                    £{revBid.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>

                <div className="bg-[#0d1117] p-2 rounded-lg border border-[#30363d]/70">
                  <div className="text-[10px] text-[#8b949e] uppercase tracking-wider font-sans">Revolut Ask</div>
                  <div className="text-sm font-semibold text-[#f85149] mt-0.5">
                    £{revAsk.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* 4. Portfolio Overview Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div className="bg-[#161b22] border border-[#30363d] p-3.5 rounded-xl flex flex-col justify-between shadow-sm">
          <span className="text-[11px] text-[#8b949e] uppercase font-medium tracking-wider">
            {activeMode === "live" ? "Live Net Worth" : "Virtual Equity"}
          </span>
          <div className="text-lg font-mono font-semibold text-[#f0f6fc] mt-1">
            £{(portfolio?.total_equity_gbp ?? (activeMode === "live" ? 0 : 1000)).toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          </div>
          <div className="text-[11px] font-mono text-[#8b949e] mt-0.5">
            Cash: £{(capMgmt?.settled_cash_gbp ?? (activeMode === "live" ? (telemetry?.balances?.GBP ?? 0) : 1000)).toFixed(2)}
          </div>
        </div>

        <div className="bg-[#161b22] border border-[#30363d] p-3.5 rounded-xl flex flex-col justify-between shadow-sm">
          <span className="text-[11px] text-[#8b949e] uppercase font-medium tracking-wider">
            {activeMode === "live" ? "Live Realized PnL" : "Paper Realized PnL"}
          </span>
          <div className={`text-lg font-mono font-semibold mt-1 ${
            (portfolio?.total_realized_pnl_gbp ?? 0) >= 0 ? "text-[#3fb950]" : "text-[#f85149]"
          }`}>
            {(portfolio?.total_realized_pnl_gbp ?? 0) >= 0 ? "+" : ""}£{(portfolio?.total_realized_pnl_gbp ?? 0).toFixed(2)}
          </div>
          <div className="text-[11px] font-mono text-[#8b949e] mt-0.5">
            {(portfolio?.total_pnl_pct ?? 0).toFixed(2)}% return
          </div>
        </div>

        <div className="bg-[#161b22] border border-[#30363d] p-3.5 rounded-xl flex flex-col justify-between shadow-sm">
          <span className="text-[11px] text-[#8b949e] uppercase font-medium tracking-wider">
            Active Orders
          </span>
          <div className="text-lg font-mono font-semibold text-[#f0f6fc] mt-1">
            {restingOrders.length} {activeMode === "live" ? "live" : "virtual"}
          </div>
          <div className="text-[11px] font-mono text-[#8b949e] mt-0.5">
            {activeMode === "live" ? "Revolut X spot book" : "0.00% maker fee (post_only)"}
          </div>
        </div>

        <div className="bg-[#161b22] border border-[#30363d] p-3.5 rounded-xl flex flex-col justify-between shadow-sm">
          <span className="text-[11px] text-[#8b949e] uppercase font-medium tracking-wider">
            {activeMode === "live" ? "Risk State" : "Protected Vault"}
          </span>
          <div className="text-lg font-mono font-semibold text-[#c9d1d9] mt-1">
            {activeMode === "live" ? (cbTripped ? "HALTED" : "ACTIVE") : `£${(capMgmt?.locked_profit_gbp ?? 0).toFixed(2)}`}
          </div>
          <div className="text-[11px] font-mono text-[#8b949e] mt-0.5">
            {activeMode === "live" ? "Strict live safety" : `${Math.round((capMgmt?.profit_lock_pct ?? 0.3) * 100)}% profit retained`}
          </div>
        </div>
      </div>

      {/* 5. Parameter Tuning (When viewMode === "tuning") */}
      {viewMode === "tuning" && (
        <div className="bg-[#161b22] border border-[#30363d] rounded-xl p-4 space-y-4 shadow-sm">
          <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5">
            <div className="flex items-center gap-2">
              <Sliders className="w-4 h-4 text-[#8b949e]" />
              <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
                Parameter Configuration ({activeMode === "live" ? "LIVE REAL DESK" : "PAPER SANDBOX"})
              </span>
            </div>
            {activeMode === "live" && (
              <button
                onClick={handleSyncRevolutBalances}
                disabled={syncingRevolut}
                className="text-xs font-mono text-[#8b949e] hover:text-[#f0f6fc] flex items-center gap-1.5 transition"
              >
                <RefreshCw className={`w-3 h-3 ${syncingRevolut ? "animate-spin" : ""}`} />
                <span>Sync Balances</span>
              </button>
            )}
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-xs font-mono">
            {/* BTC Grid */}
            <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg space-y-2.5">
              <span className="font-medium text-[#f0f6fc]">BTC/GBP Grid</span>
              <div className="space-y-2 text-[#8b949e]">
                <div>
                  <label className="text-[10px] block mb-1 uppercase tracking-wider">Step Spacing (%)</label>
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
                    className="w-full bg-[#161b22] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-[#58a6ff] outline-none"
                  />
                </div>
                <div>
                  <label className="text-[10px] block mb-1 uppercase tracking-wider">Rebalance (%)</label>
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
                    className="w-full bg-[#161b22] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-[#58a6ff] outline-none"
                  />
                </div>
              </div>
              <button
                onClick={() => handleTuneRunner("runner_btc")}
                className="w-full bg-[#21262d] hover:bg-[#30363d] text-[#c9d1d9] border border-[#30363d] py-1.5 rounded transition text-xs font-medium"
              >
                Apply BTC ({activeMode})
              </button>
            </div>

            {/* ETH Grid */}
            <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg space-y-2.5">
              <span className="font-medium text-[#f0f6fc]">ETH/GBP Grid</span>
              <div className="space-y-2 text-[#8b949e]">
                <div>
                  <label className="text-[10px] block mb-1 uppercase tracking-wider">Step Spacing (%)</label>
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
                    className="w-full bg-[#161b22] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-[#58a6ff] outline-none"
                  />
                </div>
                <div>
                  <label className="text-[10px] block mb-1 uppercase tracking-wider">Rebalance (%)</label>
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
                    className="w-full bg-[#161b22] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-[#58a6ff] outline-none"
                  />
                </div>
              </div>
              <button
                onClick={() => handleTuneRunner("runner_eth")}
                className="w-full bg-[#21262d] hover:bg-[#30363d] text-[#c9d1d9] border border-[#30363d] py-1.5 rounded transition text-xs font-medium"
              >
                Apply ETH ({activeMode})
              </button>
            </div>

            {/* Capital Allocation & Profit Retain */}
            <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg space-y-2.5">
              <span className="font-medium text-[#f0f6fc]">Capital & Sizing</span>
              <div className="space-y-2 text-[#8b949e]">
                <div>
                  <div className="flex justify-between text-[10px] uppercase tracking-wider mb-1">
                    <span>Profit Lock</span>
                    <span className="text-[#c9d1d9]">{capitalParams.profit_lock_pct}%</span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max="90"
                    step="5"
                    value={capitalParams.profit_lock_pct}
                    onChange={(e) => setCapitalParams({ ...capitalParams, profit_lock_pct: e.target.value })}
                    className="w-full accent-[#58a6ff] cursor-pointer"
                  />
                </div>
                <div>
                  <div className="flex justify-between text-[10px] uppercase tracking-wider mb-1">
                    <span>BTC / ETH Split</span>
                    <span className="text-[#c9d1d9]">{capitalParams.split_btc_pct}% / {100 - parseInt(capitalParams.split_btc_pct || "50")}%</span>
                  </div>
                  <input
                    type="range"
                    min="10"
                    max="90"
                    step="5"
                    value={capitalParams.split_btc_pct}
                    onChange={(e) => setCapitalParams({ ...capitalParams, split_btc_pct: e.target.value })}
                    className="w-full accent-[#58a6ff] cursor-pointer"
                  />
                </div>
              </div>
              <button
                onClick={() => handleConfigureCapital()}
                className="w-full bg-[#21262d] hover:bg-[#30363d] text-[#c9d1d9] border border-[#30363d] py-1.5 rounded transition text-xs font-medium"
              >
                Save Allocation
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 6. Primary Workspace (2 Columns: Orders on left, Tape on right) */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-4">
        {/* Orders Ladder (7 Cols) */}
        <div className="lg:col-span-7 bg-[#161b22] border border-[#30363d] rounded-xl p-4 flex flex-col h-[480px] shadow-sm">
          <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5 mb-2">
            <div className="flex items-center gap-2">
              <Clock className="w-3.5 h-3.5 text-[#8b949e]" />
              <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
                {activeMode === "live" ? "Live Resting Orders (Revolut X)" : "Paper Resting Orders (Simulation)"}
              </span>
              <span className="text-[10px] px-1.5 py-0.2 rounded bg-[#21262d] text-[#8b949e] font-mono">
                {filteredOrders.length}
              </span>
            </div>

            <div className="flex gap-1 text-[10px] font-mono">
              {(['ALL', 'BTC', 'ETH'] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setAssetFilter(f)}
                  className={`px-2 py-0.5 rounded transition ${
                    assetFilter === f
                      ? "bg-[#21262d] text-[#f0f6fc] border border-[#30363d]"
                      : "text-[#8b949e] hover:text-[#c9d1d9]"
                  }`}
                >
                  {f}
                </button>
              ))}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto">
            <table className="w-full text-left font-mono text-[11px]">
              <thead className="text-[10px] text-[#8b949e] border-b border-[#30363d]/60 sticky top-0 bg-[#161b22]">
                <tr>
                  <th className="pb-1.5 font-normal">SIDE</th>
                  <th className="pb-1.5 font-normal">PAIR</th>
                  <th className="pb-1.5 font-normal text-right">PRICE</th>
                  <th className="pb-1.5 font-normal text-right">QTY</th>
                  <th className="pb-1.5 font-normal text-right">DIST</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[#30363d]/30">
                {filteredOrders.length === 0 ? (
                  <tr>
                    <td colSpan={5} className="py-16 text-center text-[#8b949e] text-xs font-sans">
                      {activeMode === "live"
                        ? "No live resting limit orders on Revolut X."
                        : "No open virtual resting orders."}
                    </td>
                  </tr>
                ) : (
                  filteredOrders.map((ord) => {
                    const isBuy = ord.side === "BUY";
                    return (
                      <tr key={ord.id} className="hover:bg-[#21262d]/40 transition">
                        <td className="py-1.5">
                          <span className={`text-[10px] font-medium ${isBuy ? "text-[#3fb950]" : "text-[#f85149]"}`}>
                            {ord.side}
                          </span>
                        </td>
                        <td className="py-1.5 text-[#8b949e]">{ord.symbol}</td>
                        <td className="py-1.5 text-right font-medium text-[#f0f6fc]">
                          £{ord.price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                        </td>
                        <td className="py-1.5 text-right text-[#8b949e]">{ord.qty}</td>
                        <td className={`py-1.5 text-right ${ord.distance_pct >= 0 ? "text-[#f85149]" : "text-[#3fb950]"}`}>
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

        {/* Execution Tape (5 Cols) */}
        <div className="lg:col-span-5 bg-[#161b22] border border-[#30363d] rounded-xl p-4 flex flex-col h-[480px] shadow-sm">
          <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5 mb-2">
            <div className="flex items-center gap-2">
              <Activity className="w-3.5 h-3.5 text-[#8b949e]" />
              <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
                {activeMode === "live" ? "Live Trade Tape (Revolut X)" : "Paper Trade Tape (Simulation)"}
              </span>
            </div>

            <div className="flex gap-1 text-[10px] font-mono">
              {(['ALL', 'SNIPES', 'GRIDS'] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setTapeFilter(f)}
                  className={`px-2 py-0.5 rounded transition ${
                    tapeFilter === f
                      ? "bg-[#21262d] text-[#f0f6fc] border border-[#30363d]"
                      : "text-[#8b949e] hover:text-[#c9d1d9]"
                  }`}
                >
                  {f}
                </button>
              ))}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto space-y-1.5 pr-1 font-mono text-xs">
            {filteredTrades.length === 0 ? (
              <div className="text-center py-16 text-[#8b949e] text-xs font-sans">
                {activeMode === "live"
                  ? "No live executions recorded on Revolut X."
                  : "Awaiting paper simulated executions..."}
              </div>
            ) : (
              filteredTrades.map((t) => {
                const isBuy = t.action.includes("BUY");
                const isSell = t.action.includes("SELL");

                return (
                  <div key={t.id} className="p-2 rounded-lg bg-[#0d1117] border border-[#30363d]/60 text-xs flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="text-[#8b949e] text-[10px]">{t.time_str}</span>
                      <span className={`text-[10px] font-medium ${isBuy ? "text-[#3fb950]" : isSell ? "text-[#f85149]" : "text-[#c9d1d9]"}`}>
                        {t.action}
                      </span>
                      <span className="text-[#f0f6fc]">{t.symbol}</span>
                    </div>
                    {t.profit > 0 && (
                      <span className="text-[#3fb950] font-medium">
                        +£{t.profit.toFixed(2)}
                      </span>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>

      {/* 7. Subtle Live Confirmation Modal */}
      {liveConfirmModalOpen && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="bg-[#161b22] border border-[#30363d] rounded-xl max-w-sm w-full p-5 space-y-4 shadow-xl">
            <div className="space-y-1">
              <h3 className="text-sm font-semibold text-[#f0f6fc]">Switch to Live Mode</h3>
              <p className="text-xs text-[#8b949e]">
                Orders will be dispatched directly to Revolut X using available GBP funds. Zero fallback is applied.
              </p>
            </div>

            <div className="flex items-center justify-end gap-2 pt-1 font-medium text-xs">
              <button
                onClick={() => setLiveConfirmModalOpen(false)}
                className="px-3 py-1.5 rounded-lg text-[#8b949e] hover:text-[#f0f6fc] transition"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  setLiveConfirmModalOpen(false);
                  performSwitchMode("live");
                }}
                className="px-3 py-1.5 rounded-lg bg-[#f85149] hover:bg-[#da3633] text-white transition"
              >
                Confirm Live
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 8. Subtle Emergency Halt Modal */}
      {killModalOpen && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="bg-[#161b22] border border-[#30363d] rounded-xl max-w-sm w-full p-5 space-y-4 shadow-xl">
            <div className="space-y-1">
              <h3 className="text-sm font-semibold text-[#f0f6fc]">Halt Strategy Execution</h3>
              <p className="text-xs text-[#8b949e]">
                This will trip the circuit breaker, halt runners, and cancel all resting orders across venues.
              </p>
            </div>

            <div className="flex items-center justify-end gap-2 pt-1 font-medium text-xs">
              <button
                onClick={() => setKillModalOpen(false)}
                className="px-3 py-1.5 rounded-lg text-[#8b949e] hover:text-[#f0f6fc] transition"
              >
                Cancel
              </button>
              <button
                onClick={handleKillSwitch}
                className="px-3 py-1.5 rounded-lg bg-[#f85149] hover:bg-[#da3633] text-white transition"
              >
                Halt Engine
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
