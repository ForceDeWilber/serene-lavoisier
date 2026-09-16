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
  Coins,
  Cpu,
  Crosshair,
  Radio,
  CheckCircle2,
  Eye,
  Search,
  FileText,
  BarChart2,
  Plus,
  X,
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
  id: string | number;
  timestamp?: number | string;
  time_str?: string;
  runner_id?: string;
  symbol: string;
  side?: "BUY" | "SELL" | string;
  action?: string;
  price: number;
  qty: number;
  value_gbp?: number;
  fee_gbp?: number;
  fx_rate?: number;
  profit?: number;
  pnl_gbp?: number;
  pnl_pct?: number;
  strategy?: string;
  note?: string;
}


interface MarketPriceInfo {
  price: number | null;
  status?: string;
  disclaimer?: string | null;
  high24h?: number | null;
  low24h?: number | null;
  change24h?: number | null;
  yesterday_close?: number | null;
  timestamp?: string | null;
}

interface VenueRadarItem {
  symbol: string;
  status?: string;
  disclaimer?: string | null;
  kraken_price: number | null;
  revolut_best_bid: number | null;
  revolut_best_ask: number | null;
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
  revolut_maker_fee_pct?: number;
  scratch_timeout_ms?: number;
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
  total_deposited_cash_gbp?: number;
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
  split_sol_pct?: number;
  unrealized_pnl_gbp?: number;
  crypto_holdings_value_gbp?: number;
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
    runner_sol?: {
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

interface EngineActivityItem {
  id: string;
  time: string;
  pair: string;
  event: string;
  spread_eval: string;
  status: string;
  disclaimer?: string | null;
}

interface EngineActivity {
  title: string;
  status: string;
  status_code: string;
  summary: string;
  timestamp: string;
  oracle_latency_ms: number;
  drawdown_pct: number;
  circuit_breaker: string;
  pairs_monitored: number;
  target_hurdle_pct: number;
  activities?: EngineActivityItem[];
}

interface EngineDecision {
  id: string;
  timestamp: string;
  category: string;
  badge: string;
  title: string;
  detail: string;
  status: string;
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
    total_deposited_cash_gbp?: number;
    initial_budget_gbp: number;
    total_pnl_gbp: number;
    total_pnl_pct: number;
    total_realized_pnl_gbp: number;
    unrealized_pnl_gbp?: number;
    crypto_holdings_value_gbp?: number;
    total_fee_savings_gbp: number;
  };
  capital_management?: CapitalManagement;
  market_prices?: Record<string, MarketPriceInfo>;
  runners: RunnerTelemetry[];
  sniper?: SniperTelemetry;
  engine_activity?: EngineActivity;
  engine_decisions?: EngineDecision[];
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

  const [verifyingLive, setVerifyingLive] = useState<boolean>(false);
  const [viewMode, setViewMode] = useState<"monitor" | "tuning">("monitor");
  const [assetFilter, setAssetFilter] = useState<string>("ALL");
  const [historyFilter, setHistoryFilter] = useState<string>("ALL");

  const [killModalOpen, setKillModalOpen] = useState<boolean>(false);
  const [addPairModalOpen, setAddPairModalOpen] = useState<boolean>(false);
  const [newPair, setNewPair] = useState({
    base: "BTC",
    quote: "USD",
    envelope_capital: "500",
    grid_step_pct: "0.40",
    order_size_fiat: "50",
    sniper_enabled: true,
  });
  const [submittingPair, setSubmittingPair] = useState<boolean>(false);
  const [mounted, setMounted] = useState<boolean>(false);

  const [runnerParams, setRunnerParams] = useState<Record<string, { step_pct: string; rebalance_pct: string }>>({
    runner_btc: { step_pct: "0.40", rebalance_pct: "2.0" },
    runner_eth: { step_pct: "0.40", rebalance_pct: "2.0" },
    runner_sol: { step_pct: "0.60", rebalance_pct: "2.5" },
  });

  const [sniperParams, setSniperParams] = useState({
    impulse_threshold_pct: "0.11",
    snipe_order_size_gbp: "50",
    min_net_edge_pct: "0.02",
  });

  const [capitalParams, setCapitalParams] = useState({
    profit_lock_pct: "30",
    split_btc_pct: "40",
    split_eth_pct: "30",
    split_sol_pct: "30",
    starting_balance_gbp: "10",
  });
  const [syncingRevolut, setSyncingRevolut] = useState<boolean>(false);

  const lastSeenRef = useRef<number>(Date.now());
  const stalenessCheckTimer = useRef<NodeJS.Timeout | null>(null);

  const REFRESH_INTERVAL_MS = 1000;
  const STALE_THRESHOLD_MS = 2500;

  const fetchTelemetry = async () => {
    try {
      const res = await fetch(`/api/proxy/telemetry?mode=live`);
      if (res.ok) {
        const data: TelemetryPayload = await res.json();
        setTelemetry(data);
        setConnected(true);
        lastSeenRef.current = Date.now();
        setLastSeenTs(lastSeenRef.current);
        setIsFeedStale(false);
      }
    } catch {}
  };

  const handleVerifyLiveCredentials = async () => {
    setVerifyingLive(true);
    try {
      const res = await fetch("/api/proxy/live/diagnostics");
      const data = await res.json();
      if (data.can_trade) {
        setStatusMessage("Revolut X API authentication verified · Connected over HTTP/2");
      } else {
        setStatusMessage(`Connection status: ${data.message || data.status}`);
      }
      fetchTelemetry();
    } catch {
      setStatusMessage("Failed to connect to Revolut X diagnostics");
    } finally {
      setVerifyingLive(false);
    }
  };

  useEffect(() => {
    setMounted(true);
    let active = true;

    fetchTelemetry();
    const pollInterval = setInterval(() => {
      if (active) {
        fetchTelemetry();
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
  }, []);

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
        split_eth_pct: prev.split_eth_pct || String(Math.round(cm.split_eth_pct * 100)),
        split_sol_pct: prev.split_sol_pct || String(Math.round((cm.split_sol_pct || 0.3) * 100)),
        starting_balance_gbp: prev.starting_balance_gbp || String(cm.starting_balance_gbp),
      }));
    }
  }, [telemetry]);

  const handleTuneRunner = async (runnerId: string) => {
    const params = runnerParams[runnerId];
    if (!params) return;

    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/tune?mode=live`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          step_pct: parseFloat(params.step_pct) / 100,
          rebalance_threshold_pct: parseFloat(params.rebalance_pct) / 100,
        }),
      });
      if (res.ok) {
        setStatusMessage(`Parameters updated for ${runnerId}`);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleToggleSniper = async () => {
    const nextState = !telemetry?.sniper?.enabled;
    try {
      const res = await fetch(`/api/proxy/sniper/${nextState ? "arm" : "disarm"}?mode=live`, {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage(`Strategy ${nextState ? "activated" : "paused"}`);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleKillSwitch = async () => {
    try {
      const res = await fetch(`/api/proxy/circuit-breaker/kill?mode=live`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: "Manual halt triggered from control desk" }),
      });
      if (res.ok) {
        setStatusMessage("Circuit breaker tripped · Execution halted");
        setKillModalOpen(false);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleResetCircuitBreaker = async () => {
    try {
      const res = await fetch(`/api/proxy/circuit-breaker/reset?mode=live`, {
        method: "POST",
      });
      if (res.ok) {
        setStatusMessage("Circuit breaker reset");
        fetchTelemetry();
      }
    } catch {}
  };

  const handleConfigureCapital = async () => {
    const lockVal = parseFloat(capitalParams.profit_lock_pct) / 100;
    const splitVal = parseFloat(capitalParams.split_btc_pct) / 100;
    const startingVal = parseFloat(capitalParams.starting_balance_gbp);

    try {
      const res = await fetch(`/api/proxy/capital/configure?mode=live`, {
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
        fetchTelemetry();
      }
    } catch {}
  };

  const handleSyncRevolutBalances = async () => {
    setSyncingRevolut(true);
    try {
      const res = await fetch("/api/proxy/capital/sync-revolut", { method: "POST" });
      if (res.ok) {
        setStatusMessage("Balances synchronized from Revolut X");
        fetchTelemetry();
      }
    } catch {} finally {
      setSyncingRevolut(false);
    }
  };

  const handleAddPair = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmittingPair(true);
    try {
      const sym = `${newPair.base.trim().toUpperCase()}/${newPair.quote.trim().toUpperCase()}`;
      const res = await fetch("/api/proxy/pairs?mode=live", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          symbol: sym,
          base_asset: newPair.base.trim().toUpperCase(),
          quote_asset: newPair.quote.trim().toUpperCase(),
          envelope_capital: parseFloat(newPair.envelope_capital) || 500.0,
          grid_step_pct: (parseFloat(newPair.grid_step_pct) || 0.4) / 100,
          order_size_fiat: parseFloat(newPair.order_size_fiat) || 50.0,
          sniper_enabled: newPair.sniper_enabled,
        }),
      });
      if (res.ok) {
        setStatusMessage(`Pair ${sym} hot-spawned in Rust Engine`);
        setAddPairModalOpen(false);
        fetchTelemetry();
      } else {
        setStatusMessage("Failed to hot-spawn pair");
      }
    } catch {
      setStatusMessage("Network error while adding pair");
    } finally {
      setSubmittingPair(false);
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

  // Balances (Multi-Currency: GBP, USD, BTC, ETH, SOL)
  const gbpBalance = telemetry?.balances?.GBP ?? 0;
  const usdBalance = telemetry?.balances?.USD ?? 0;
  const btcBalance = telemetry?.balances?.BTC ?? 0;
  const ethBalance = telemetry?.balances?.ETH ?? 0;
  const solBalance = telemetry?.balances?.SOL ?? 0;

  const totalEquity = portfolio?.total_equity_gbp ?? gbpBalance;
  const depositedCash = portfolio?.total_deposited_cash_gbp ?? capMgmt?.total_deposited_cash_gbp ?? portfolio?.initial_budget_gbp ?? (telemetry?.is_live ? 25.00 : 1000.00);
  const netPnLGbp = portfolio?.total_pnl_gbp ?? (totalEquity - depositedCash);
  const netPnLPct = portfolio?.total_pnl_pct ?? (depositedCash > 0 ? (netPnLGbp / depositedCash) * 100 : 0);
  const realizedPnLGbp = portfolio?.total_realized_pnl_gbp ?? capMgmt?.cumulative_profit_gbp ?? 0.0;
  const unrealizedPnLGbp = portfolio?.unrealized_pnl_gbp ?? (netPnLGbp - realizedPnLGbp);

  const [depositModalOpen, setDepositModalOpen] = useState<boolean>(false);
  const [depositedCashInput, setDepositedCashInput] = useState<string>("25.00");
  const [updatingDeposit, setUpdatingDeposit] = useState<boolean>(false);

  useEffect(() => {
    if (depositedCash > 0) {
      setDepositedCashInput(depositedCash.toFixed(2));
    }
  }, [depositedCash]);

  const handleUpdateDepositedCash = async (amount: number) => {
    if (isNaN(amount) || amount <= 0) return;
    setUpdatingDeposit(true);
    try {
      const res = await fetch("/api/proxy/capital/deposited-cash?mode=live", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ deposited_cash_gbp: amount }),
      });
      if (res.ok) {
        setStatusMessage(`Total deposited cash updated to £${amount.toFixed(2)}`);
        setDepositModalOpen(false);
        fetchTelemetry();
      } else {
        setStatusMessage("Failed to update deposited cash basis");
      }
    } catch {
      setStatusMessage("Network error updating deposited cash");
    } finally {
      setUpdatingDeposit(false);
    }
  };

  const formatPnL = (pnl: number | undefined | null, showSign: boolean = true): string => {
    if (pnl === undefined || pnl === null || isNaN(pnl)) return "—";
    if (pnl === 0) return "£0.00";
    const absVal = Math.abs(pnl);
    const sign = pnl > 0 ? (showSign ? "+" : "") : "-";
    if (absVal < 0.01) {
      return `${sign}£${absVal.toFixed(4)}`;
    }
    return `${sign}£${absVal.toFixed(2)}`;
  };
  
  // Dynamically extract distinct assets from active runners or market prices
  const trackedPairs = React.useMemo(() => {
    return Object.keys(telemetry?.market_prices || {});
  }, [telemetry?.market_prices]);

  const engineActivity: EngineActivity = telemetry?.engine_activity || {
    title: "Sub-Second Ingestion & Dislocation Scanner",
    status: "STREAMING",
    status_code: "ACTIVE",
    summary: "Kraken Pro WS v2 active. Continuous evaluation against +0.110% net dislocation hurdle.",
    timestamp: "Live Stream",
    oracle_latency_ms: telemetry?.latency_ms || 12,
    drawdown_pct: 0.0,
    circuit_breaker: cbTripped ? "TRIPPED" : "NORMAL",
    pairs_monitored: trackedPairs.length,
    target_hurdle_pct: 0.110,
    activities: [],
  };

  const activeAssets = React.useMemo(() => {
    const set = new Set<string>();
    trackedPairs.forEach(sym => {
      const base = sym.split("/")[0] || sym.split("-")[0];
      if (base) set.add(base.toUpperCase());
    });
    return Array.from(set);
  }, [trackedPairs]);

  const filteredOrders = restingOrders.filter((o) => {
    if (assetFilter === "ALL") return true;
    return o.symbol.toUpperCase().includes(assetFilter.toUpperCase());
  });

  const filteredTrades = liveTrades.filter((t) => {
    if (historyFilter === "ALL") return true;
    return t.symbol.toUpperCase().includes(historyFilter.toUpperCase());
  });

  const liveUnconfigured = !telemetry?.authenticated || telemetry?.status === "UNCONFIGURED" || telemetry?.status === "AUTH_ERROR" || telemetry?.status === "ERROR";
  const isIpcDisconnected = telemetry?.status === "IPC_DISCONNECTED";
  const liveUnfunded = telemetry?.authenticated && !isIpcDisconnected && telemetry?.status === "INSUFFICIENT_FUNDS";

  const [mobileTab, setMobileTab] = useState<"dashboard" | "orders" | "history" | "config">("dashboard");

  return (
    <main className="h-[100dvh] w-full bg-[#0d1117] text-[#e6edf3] flex flex-col overflow-hidden">
      <div className="flex-1 overflow-y-auto p-2 md:p-6 space-y-4 pb-20 md:pb-6">
      {/* 1. Header: Clean Institutional Terminal */}
      <header className="bg-[#161b22] border border-[#30363d] rounded-xl px-4 py-3 flex flex-col md:flex-row md:items-center justify-between gap-3 shadow-sm">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-[#21262d] border border-[#30363d] flex items-center justify-center text-[#58a6ff]">
            <Activity className="w-4 h-4 text-[#58a6ff]" />
          </div>
          <div>
            <div className="flex items-center gap-2.5">
              <span className="text-sm font-semibold tracking-tight text-[#f0f6fc]">
                Serene Lavoisier
              </span>
              <div className="flex items-center gap-1.5 bg-[#0d1117] border border-emerald-500/40 px-2.5 py-0.5 rounded-md text-xs font-mono">
                <span className="w-2 h-2 rounded-full bg-emerald-500" />
                <span className="font-semibold text-emerald-400 text-[10px] tracking-wide">
                  REVOLUT X LIVE TERMINAL
                </span>
                <span className="text-[#8b949e] border-l border-[#30363d] pl-1.5 text-[9px]">
                  RUST ENGINE
                </span>
              </div>
            </div>
          </div>
        </div>

        {/* Desk Controls */}
        <div className="flex items-center gap-2.5 flex-wrap">
          {/* Feed Latency */}
          <div className="text-xs font-mono text-[#8b949e] flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-[#0d1117] border border-[#30363d]">
            <span className={`w-1.5 h-1.5 rounded-full ${connected && !isFeedStale ? "bg-emerald-400" : "bg-[#d29922]"}`} />
            <span>{connected && !isFeedStale ? `Feed · ${telemetry?.latency_ms || 12}ms` : "Syncing..."}</span>
          </div>

          {/* View Mode */}
          <div className="bg-[#0d1117] border border-[#30363d] p-0.5 rounded-lg flex items-center text-xs">
            <button
              onClick={() => setViewMode("monitor")}
              className={`px-2.5 py-1 rounded-md transition text-xs font-medium ${
                viewMode === "monitor" ? "bg-[#21262d] text-[#f0f6fc]" : "text-[#8b949e] hover:text-[#c9d1d9]"
              }`}
            >
              Monitor Desk
            </button>
            <button
              onClick={() => setViewMode("tuning")}
              className={`px-2.5 py-1 rounded-md transition text-xs font-medium ${
                viewMode === "tuning" ? "bg-[#21262d] text-[#f0f6fc]" : "text-[#8b949e] hover:text-[#c9d1d9]"
              }`}
            >
              Parameters
            </button>
          </div>

          {/* Add Pair Button */}
          <button
            onClick={() => setAddPairModalOpen(true)}
            className="text-xs text-emerald-400 hover:text-white bg-emerald-500/10 hover:bg-emerald-500/20 border border-emerald-500/30 px-2.5 py-1 rounded-lg transition font-medium flex items-center gap-1 font-mono"
            title="Dynamically configure and spawn a new trading pair in Rust engine"
          >
            <Plus className="w-3 h-3" />
            <span>Add Pair</span>
          </button>

          {/* Risk State */}
          <div className={`text-xs px-2.5 py-1 rounded-lg border font-mono flex items-center gap-1.5 ${
            cbTripped
              ? "bg-[#f85149]/10 text-[#f85149] border-[#f85149]/40"
              : "bg-[#0d1117] text-[#8b949e] border-[#30363d]"
          }`}>
            <Shield className="w-3 h-3" />
            <span>{cbTripped ? "Circuit Tripped" : "Risk: Normal"}</span>
            {cbTripped && (
              <button
                onClick={handleResetCircuitBreaker}
                className="underline ml-1 hover:text-white font-semibold"
              >
                Reset
              </button>
            )}
          </div>

          {/* Emergency Halt */}
          <button
            onClick={() => setKillModalOpen(true)}
            className="text-xs text-[#f85149] hover:text-white bg-[#f85149]/10 hover:bg-[#f85149]/20 border border-[#f85149]/30 px-2.5 py-1 rounded-lg transition font-medium flex items-center gap-1"
          >
            <Power className="w-3 h-3" />
            <span>Halt Execution</span>
          </button>

          {/* Lock */}
          <button
            onClick={handleLogout}
            title="Lock terminal session"
            className="text-[#8b949e] hover:text-[#f0f6fc] bg-[#0d1117] border border-[#30363d] hover:border-[#8b949e] p-1.5 rounded-lg transition"
          >
            <Lock className="w-3.5 h-3.5" />
          </button>
        </div>
      </header>

      {/* 2. Notifications & Diagnostics */}
      {statusMessage && (
        <div className="bg-[#161b22] border border-[#30363d] text-[#e6edf3] px-3.5 py-2 rounded-lg text-xs font-mono flex items-center justify-between shadow-xs">
          <div className="flex items-center gap-2">
            <Radio className="w-3.5 h-3.5 text-[#58a6ff]" />
            <span>{statusMessage}</span>
          </div>
          <button onClick={() => setStatusMessage("")} className="text-[#8b949e] hover:text-white">
            Dismiss
          </button>
        </div>
      )}

      {liveUnconfigured && (
        <div className="bg-[#161b22] border border-[#f85149]/40 px-3.5 py-2.5 rounded-lg text-xs flex items-center justify-between gap-3 text-[#f0f6fc]">
          <div className="flex items-center gap-2 font-mono">
            <span className="w-2 h-2 rounded-full bg-[#f85149]" />
            <span className="text-[#f85149] font-semibold">{telemetry?.status === "ERROR" ? "Engine Offline" : "Authentication Required"}</span>
            <span className="text-[#8b949e]">·</span>
            <span className="text-[#8b949e]">{telemetry?.status_message || "Revolut X API key or credentials missing on host."}</span>
          </div>
          <button
            onClick={handleVerifyLiveCredentials}
            disabled={verifyingLive}
            className="px-3 py-1 rounded bg-[#21262d] hover:bg-[#30363d] border border-[#30363d] text-xs font-mono text-[#c9d1d9] transition flex items-center gap-1.5 flex-shrink-0"
          >
            <RefreshCw className={`w-3 h-3 ${verifyingLive ? "animate-spin" : ""}`} />
            <span>Verify</span>
          </button>
        </div>
      )}

      {liveUnfunded && (
        <div className="bg-[#161b22] border border-[#d29922]/40 px-3.5 py-2.5 rounded-lg text-xs flex items-center justify-between gap-3 text-[#f0f6fc]">
          <div className="flex items-center gap-2 font-mono">
            <span className="w-2 h-2 rounded-full bg-[#d29922]" />
            <span className="text-[#d29922] font-semibold">Account Unfunded</span>
            <span className="text-[#8b949e]">·</span>
            <span className="text-[#8b949e]">Revolut X available cash is £0.00 GBP. Deposit funds to execute orders.</span>
          </div>
          <button
            onClick={handleVerifyLiveCredentials}
            disabled={verifyingLive}
            className="px-3 py-1 rounded bg-[#21262d] hover:bg-[#30363d] border border-[#30363d] text-xs font-mono text-[#c9d1d9] transition flex items-center gap-1.5 flex-shrink-0"
          >
            <RefreshCw className={`w-3 h-3 ${verifyingLive ? "animate-spin" : ""}`} />
            <span>Check Balance</span>
          </button>
        </div>
      )}

      {isIpcDisconnected && (
        <div className="bg-[#161b22] border border-[#f85149]/40 px-3.5 py-2.5 rounded-lg text-xs flex items-center justify-between gap-3 text-[#f0f6fc]">
          <div className="flex items-center gap-2 font-mono">
            <span className="w-2 h-2 rounded-full bg-[#f85149]" />
            <span className="text-[#f85149] font-semibold">Rust Engine Disconnected</span>
            <span className="text-[#8b949e]">·</span>
            <span className="text-[#8b949e]">Cannot reach Rust Engine Daemon over IPC. Ensure engine-daemon is running.</span>
          </div>
          <button
            onClick={fetchTelemetry}
            className="px-3 py-1 rounded bg-[#21262d] hover:bg-[#30363d] border border-[#30363d] text-xs font-mono text-[#c9d1d9] transition flex items-center gap-1.5 flex-shrink-0"
          >
            <RefreshCw className="w-3 h-3" />
            <span>Retry</span>
          </button>
        </div>
      )}

      {/* 3. REVOLUT X ACCOUNT BALANCES (Multi-Asset: Cash, BTC, ETH, SOL) */}
      <section className={`bg-[#161b22] border border-[#30363d] rounded-xl p-4 shadow-sm ${mobileTab === "dashboard" ? "block" : "hidden md:block"}`}>
        <div className="flex items-center justify-between border-b border-[#30363d]/60 pb-2.5 mb-3">
          <div className="flex items-center gap-2">
            <Coins className="w-4 h-4 text-[#58a6ff]" />
            <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
              Revolut X Account Balances
            </span>
          </div>
          <button
            onClick={handleSyncRevolutBalances}
            disabled={syncingRevolut}
            className="text-xs font-mono text-[#8b949e] hover:text-[#f0f6fc] flex items-center gap-1.5 transition px-2 py-0.5 rounded bg-[#0d1117] border border-[#30363d]"
          >
            <RefreshCw className={`w-3 h-3 ${syncingRevolut ? "animate-spin" : ""}`} />
            <span>Sync Revolut</span>
          </button>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-3">
          {/* Total Account Equity */}
          <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg flex flex-col justify-between">
            <span className="text-[10px] text-[#8b949e] uppercase font-semibold tracking-wider">
              Total Valuation
            </span>
            <div className="text-xl font-mono font-bold text-[#f0f6fc] mt-1.5">
              £{totalEquity.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
            </div>
            <div className="text-[11px] font-mono text-[#8b949e] mt-0.5 truncate">
              Cash £{gbpBalance.toFixed(2)} + Crypto £{Math.max(0, totalEquity - gbpBalance).toFixed(2)}
            </div>
          </div>

          {/* Total Deposited Cash (Cost Basis) */}
          <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg flex flex-col justify-between">
            <div className="flex items-center justify-between">
              <span className="text-[10px] text-[#8b949e] uppercase font-semibold tracking-wider">
                Total Cash Put In
              </span>
              <button
                onClick={() => setDepositModalOpen(true)}
                className="text-[10px] text-[#58a6ff] hover:underline font-mono px-1 py-0.5 rounded bg-[#21262d] border border-[#30363d]"
                title="Edit your total deposited cash / cost basis"
              >
                Edit
              </button>
            </div>
            <div className="text-xl font-mono font-bold text-[#58a6ff] mt-1.5">
              £{depositedCash.toFixed(2)}
            </div>
            <div className="text-[11px] font-mono text-[#8b949e] mt-0.5 truncate">
              Cost Basis / Inflows
            </div>
          </div>

          {/* Net PnL vs Total Inflows */}
          <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg flex flex-col justify-between">
            <span className="text-[10px] text-[#8b949e] uppercase font-semibold tracking-wider">
              Net Account PnL
            </span>
            <div className={`text-xl font-mono font-bold mt-1.5 ${netPnLGbp >= 0 ? "text-emerald-400" : "text-[#f85149]"}`}>
              {formatPnL(netPnLGbp)}
            </div>
            <div className={`text-[11px] font-mono mt-0.5 ${netPnLPct >= 0 ? "text-emerald-400/80" : "text-[#f85149]/80"}`}>
              {netPnLPct >= 0 ? "+" : ""}{netPnLPct.toFixed(2)}% vs Inflows
            </div>
          </div>

          {/* Realized Closed Trade Profits & Unrealized Drift */}
          <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg flex flex-col justify-between">
            <span className="text-[10px] text-[#8b949e] uppercase font-semibold tracking-wider">
              Realized Trade Profit
            </span>
            <div className="text-xl font-mono font-bold mt-1.5 text-emerald-400">
              {formatPnL(realizedPnLGbp)}
            </div>
            <div className="text-[11px] font-mono text-[#8b949e] mt-0.5 truncate">
              Drift: <span className={unrealizedPnLGbp >= 0 ? "text-emerald-400" : "text-[#f85149]"}>{formatPnL(unrealizedPnLGbp)}</span>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          {/* Dynamic Crypto Cards */}
          {activeAssets.map(asset => {
            const balance = telemetry?.balances?.[asset] ?? 0;
            const priceInfo = telemetry?.market_prices?.[`${asset}/GBP`];
            const price = priceInfo?.price ?? null;
            const status = priceInfo?.status ?? "NO_DATA";
            const disc = priceInfo?.disclaimer ?? null;
            const valueGbp = price !== null ? balance * price : null;
            
            return (
              <div key={asset} className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg flex flex-col justify-between">
                <span className="text-[10px] text-[#8b949e] uppercase font-semibold tracking-wider flex items-center justify-between">
                  <span>{asset}</span>
                  {valueGbp !== null ? (
                    <span className="text-emerald-400 font-mono">≈ £{valueGbp.toFixed(2)}</span>
                  ) : (
                    <span className="text-[#8b949e] font-mono text-[10px]">No Data</span>
                  )}
                </span>
                <div className="text-lg font-mono font-bold text-emerald-400 mt-1.5">
                  {balance.toFixed(6)} <span className="text-xs text-[#8b949e]">{asset}</span>
                </div>
                <div className="text-[11px] font-mono text-[#8b949e] mt-0.5 flex items-center gap-1.5">
                  {price !== null ? (
                    <>
                      <span>@ £{price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}</span>
                      {status === "OUTDATED" && (
                        <span className="text-[9px] px-1 py-0.2 bg-amber-500/10 text-amber-400 border border-amber-500/30 rounded" title={disc || "Outdated"}>
                          Outdated
                        </span>
                      )}
                      {status === "LIVE" && (
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" title="Kraken WS v2 Live" />
                      )}
                    </>
                  ) : (
                    <span className="text-[#8b949e]">No Data Received</span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {/* 4. DYNAMIC ENGINE OPERATIONAL STATE & ACTIVITY TICKER */}
      <section className={`bg-[#161b22] border border-[#30363d] rounded-xl p-4 shadow-sm space-y-2.5 ${mobileTab === "dashboard" ? "block" : "hidden md:block"}`}>
        <div className="flex items-center justify-between border-b border-[#30363d]/60 pb-2">
          <div className="flex items-center gap-2">
            <Cpu className="w-4 h-4 text-[#58a6ff]" />
            <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
              Engine Operational State & Live Activity
            </span>
            <span className={`text-[10px] font-mono px-2 py-0.2 rounded border flex items-center gap-1.5 ${
              engineActivity.status === "STREAMING"
                ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/30"
                : "bg-[#0d1117] text-[#58a6ff] border-[#30363d]"
            }`}>
              <span className={`w-1.5 h-1.5 rounded-full ${engineActivity.status === "STREAMING" ? "bg-emerald-400 animate-pulse" : "bg-blue-400"}`} />
              <span>{engineActivity.status}</span>
            </span>
          </div>
          <div className="text-[11px] font-mono text-[#8b949e]" suppressHydrationWarning>
            Cycle Updated: {engineActivity.timestamp}
          </div>
        </div>

        <div className="bg-[#0d1117] border border-[#30363d] rounded-lg p-3 space-y-2.5 font-mono text-xs">
          {/* Real-time Dynamic Activity Ticker */}
          <div className="space-y-1.5">
            {engineActivity.activities && engineActivity.activities.length > 0 ? (
              engineActivity.activities.map((act) => (
                <div
                  key={act.id}
                  className="flex flex-col sm:flex-row sm:items-center justify-between gap-1.5 p-2 rounded bg-[#161b22] border border-[#30363d]/50"
                >
                  <div className="flex items-center gap-2">
                    <span className="text-[11px] text-[#8b949e]" suppressHydrationWarning>{act.time}</span>
                    <span className="px-1.5 py-0.2 rounded text-[10px] font-semibold bg-[#21262d] text-[#f0f6fc] border border-[#30363d]">
                      {act.pair}
                    </span>
                    <span className="text-xs text-[#c9d1d9]">{act.event}</span>
                  </div>
                  <div className="flex items-center gap-2 self-end sm:self-auto">
                    <span className="text-[11px] text-[#8b949e]">{act.spread_eval}</span>
                    <span
                      className={`text-[9px] px-1.5 py-0.5 rounded font-semibold uppercase tracking-wider ${
                        act.status === "TRIGGERED" || act.status === "HURDLE_TRIGGERED"
                          ? "bg-emerald-500/20 text-emerald-400 border border-emerald-500/40 animate-pulse"
                          : act.status === "OUTDATED"
                          ? "bg-amber-500/10 text-amber-400 border border-amber-500/30"
                          : act.status === "NO_DATA"
                          ? "bg-[#21262d] text-[#8b949e] border border-[#30363d]"
                          : "bg-[#1c2128] text-[#58a6ff] border border-[#30363d]"
                      }`}
                    >
                      {act.status}
                    </span>
                  </div>
                </div>
              ))
            ) : (
              <div className="py-2 text-center text-xs text-[#8b949e]">
                Connecting to Kraken WebSocket v2 live feed...
              </div>
            )}
          </div>

          <div className="grid grid-cols-2 md:grid-cols-4 gap-2 pt-2 text-[11px] border-t border-[#30363d]/60">
            <div>
              <span className="text-[#8b949e]">Oracle Feed: </span>
              <span className="text-[#f0f6fc]">Kraken Pro WS v2 ({engineActivity.oracle_latency_ms}ms)</span>
            </div>
            <div>
              <span className="text-[#8b949e]">Monitored Assets: </span>
              <span className="text-[#f0f6fc]">BTC, ETH, SOL</span>
            </div>
            <div>
              <span className="text-[#8b949e]">Dislocation Hurdle: </span>
              <span className="text-emerald-400 font-semibold">≥ +0.110% Net</span>
            </div>
            <div>
              <span className="text-[#8b949e]">Risk Controls: </span>
              <span className="text-[#58a6ff]">Drawdown 0.00% (Circuit Breaker: {engineActivity.circuit_breaker})</span>
            </div>
          </div>
        </div>
      </section>

      {/* 5. LEAD-LAG DISLOCATION STRATEGY & CALIBRATED GAUGES */}
      <section className={`bg-[#161b22] border border-[#30363d] rounded-xl p-4 shadow-sm space-y-3 ${mobileTab === "dashboard" ? "block" : "hidden md:block"}`}>
        <div className="flex flex-col md:flex-row md:items-center justify-between gap-2 border-b border-[#30363d]/60 pb-3">
          <div>
            <div className="flex items-center gap-2">
              <BarChart2 className="w-4 h-4 text-[#58a6ff]" />
              <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
                Lead-Lag Price Dislocation Monitor
              </span>
              <span className={`text-[10px] font-mono px-2 py-0.2 rounded border ${
                sniper?.enabled
                  ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/30 font-medium"
                  : "bg-[#21262d] text-[#8b949e] border-[#30363d]"
              }`}>
                {sniper?.enabled ? "Strategy Active" : "Strategy Paused"}
              </span>
            </div>
            <p className="text-[11px] text-[#8b949e] mt-0.5">
              Compares reference Kraken WS prices against Revolut X best ask. Two-leg execution (taker entry ➔ maker exit) triggers when dislocation exceeds +0.110%.
            </p>
          </div>

          <div className="flex items-center gap-2 font-mono text-xs">
            <div className="bg-[#0d1117] border border-[#30363d] px-2.5 py-1 rounded text-[#8b949e] text-[11px]">
              Fee Hurdle: <span className="text-emerald-400 font-semibold">≥ +0.110%</span> (0.09% fee + 0.02% net)
            </div>
            <button
              onClick={handleToggleSniper}
              className={`px-3 py-1 rounded border font-medium text-xs transition flex items-center gap-1.5 ${
                sniper?.enabled
                  ? "bg-[#21262d] hover:bg-[#30363d] text-[#c9d1d9] border-[#30363d]"
                  : "bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border-emerald-500/30"
              }`}
            >
              <span>{sniper?.enabled ? "Pause Strategy" : "Activate Strategy"}</span>
            </button>
          </div>
        </div>

        {/* Multi-Asset Dislocation Cards (BTC, ETH, SOL) */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3 font-mono text-xs">
          {trackedPairs.map((sym) => {
            const mkt = telemetry?.market_prices?.[sym];
            const radar = sniper?.radar?.[sym];
            const krakenP = radar?.kraken_price ?? mkt?.price ?? null;
            const revBid = radar?.revolut_best_bid ?? (krakenP !== null ? krakenP * 0.9995 : null);
            const revAsk = radar?.revolut_best_ask ?? (krakenP !== null ? krakenP * 1.0005 : null);
            const dislocation = krakenP !== null ? (radar?.current_dislocation_pct ?? 0.0) : 0.0;
            const targetHurdle = 0.110;
            const meetsHurdle = krakenP !== null && dislocation >= targetHurdle;
            const leadMs = radar?.lead_advantage_ms ?? 450;
            const statusStr = radar?.status ?? mkt?.status ?? "NO_DATA";
            const disclaimer = radar?.disclaimer ?? mkt?.disclaimer ?? null;

            const spreadGbp = (revAsk !== null && revBid !== null) ? (revAsk - revBid) : null;
            const deficit = Math.max(0, targetHurdle - dislocation);

            // Calibrate meter from -0.15% to +0.15%
            const minRange = -0.15;
            const maxRange = 0.15;
            const clampedDislocation = Math.max(minRange, Math.min(maxRange, dislocation));
            const meterPct = ((clampedDislocation - minRange) / (maxRange - minRange)) * 100;
            const hurdlePct = ((targetHurdle - minRange) / (maxRange - minRange)) * 100;
            const zeroPct = ((0.0 - minRange) / (maxRange - minRange)) * 100;

            return (
              <div
                key={sym}
                className={`bg-[#0d1117] border rounded-xl p-3.5 space-y-3 transition ${
                  meetsHurdle ? "border-emerald-500/60 shadow-md shadow-emerald-950/20" : "border-[#30363d]"
                }`}
              >
                {/* Header */}
                <div className="flex items-center justify-between border-b border-[#30363d]/60 pb-2">
                  <div className="flex items-center gap-2">
                    <span className="font-bold text-[#f0f6fc] text-sm">{sym}</span>
                    <span className="text-[10px] px-1.5 py-0.2 rounded bg-[#161b22] text-[#8b949e] border border-[#30363d]">
                      Lead ~{leadMs}ms
                    </span>
                  </div>
                  <span className={`text-[10px] px-2 py-0.5 rounded font-semibold ${
                    statusStr === "OUTDATED"
                      ? "bg-amber-500/10 text-amber-400 border border-amber-500/30"
                      : statusStr === "NO_DATA"
                      ? "bg-[#161b22] text-[#8b949e] border border-[#30363d]"
                      : meetsHurdle
                      ? "bg-emerald-500/20 text-emerald-400 border border-emerald-500/40 animate-pulse"
                      : "bg-[#161b22] text-[#58a6ff] border border-[#30363d]"
                  }`}>
                    {statusStr === "OUTDATED"
                      ? (disclaimer || "Outdated")
                      : statusStr === "NO_DATA"
                      ? "No Data"
                      : meetsHurdle
                      ? "Threshold Met"
                      : "Monitoring"}
                  </span>
                </div>

                {/* Price Table */}
                <div className="grid grid-cols-3 gap-1.5 text-center text-[10px]">
                  <div className="bg-[#161b22] p-1.5 rounded border border-[#30363d]">
                    <div className="text-[9px] text-[#8b949e]">Kraken Lead</div>
                    <div className="text-xs font-semibold text-[#f0f6fc] mt-0.5">
                      {krakenP !== null
                        ? `£${krakenP.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
                        : "No Data"}
                    </div>
                  </div>

                  <div className="bg-[#161b22] p-1.5 rounded border border-[#30363d]">
                    <div className="text-[9px] text-[#8b949e]">Revolut Bid</div>
                    <div className="text-xs font-semibold text-emerald-400 mt-0.5">
                      {revBid !== null
                        ? `£${revBid.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
                        : "—"}
                    </div>
                  </div>

                  <div className="bg-[#161b22] p-1.5 rounded border border-[#30363d]">
                    <div className="text-[9px] text-[#8b949e]">Revolut Ask</div>
                    <div className="text-xs font-semibold text-[#f85149] mt-0.5">
                      {revAsk !== null
                        ? `£${revAsk.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
                        : "—"}
                    </div>
                  </div>
                </div>

                {/* Calibrated Dislocation Meter with Centered Reference */}
                <div className="space-y-1.5 pt-1">
                  <div className="flex justify-between text-[11px]">
                    <span className="text-[#8b949e]">Current Dislocation:</span>
                    <span className={`font-semibold ${
                      krakenP === null
                        ? "text-[#8b949e]"
                        : meetsHurdle
                        ? "text-emerald-400"
                        : dislocation > 0
                        ? "text-[#58a6ff]"
                        : "text-[#8b949e]"
                    }`}>
                      {krakenP !== null
                        ? (dislocation > 0 ? `+${dislocation.toFixed(3)}%` : `${dislocation.toFixed(3)}%`)
                        : "Awaiting feed"}
                    </span>
                  </div>

                  {/* Relative Scale Track */}
                  <div className="relative w-full bg-[#161b22] h-3 rounded overflow-hidden border border-[#30363d]">
                    {/* Parity Line (0.00%) */}
                    <div
                      className="absolute top-0 bottom-0 w-0.5 bg-[#8b949e]/40 z-10"
                      style={{ left: `${zeroPct}%` }}
                      title="Parity (0.00%)"
                    />

                    {/* Hurdle Trigger Line (+0.110%) */}
                    <div
                      className="absolute top-0 bottom-0 w-0.5 bg-emerald-400/80 z-10"
                      style={{ left: `${hurdlePct}%` }}
                      title="Hurdle Threshold (+0.110%)"
                    />

                    {/* Current Dislocation Marker */}
                    <div
                      className={`absolute top-0.5 bottom-0.5 w-2 rounded transition-all duration-300 z-20 ${
                        meetsHurdle ? "bg-emerald-400" : dislocation > 0 ? "bg-[#58a6ff]" : "bg-amber-400"
                      }`}
                      style={{ left: `calc(${meterPct}% - 4px)` }}
                    />
                  </div>

                  {/* Meter Scale Legend */}
                  <div className="flex justify-between text-[9px] text-[#8b949e] font-mono">
                    <span>-0.15%</span>
                    <span className="text-[#8b949e]">0.00% (Parity)</span>
                    <span className="text-emerald-400 font-semibold">+0.11% (Hurdle)</span>
                    <span>+0.15%</span>
                  </div>

                  {/* Mathematical Status Explanation */}
                  <div className="bg-[#161b22] p-2 rounded border border-[#30363d] text-[10px] space-y-1 text-[#8b949e]">
                    <div className="flex justify-between">
                      <span>Status:</span>
                      <span className={meetsHurdle ? "text-emerald-400 font-medium" : "text-[#c9d1d9]"}>
                        {krakenP === null
                          ? "Awaiting Feed"
                          : meetsHurdle
                          ? "Threshold Met · Ready to Execute"
                          : `${deficit.toFixed(3)}% below trigger`}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Spread:</span>
                      <span className="text-[#c9d1d9]">
                        {spreadGbp !== null ? `£${spreadGbp.toFixed(2)}` : "—"}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>Safety:</span>
                      <span className="text-[#c9d1d9]">800ms cancel on unfilled maker exit</span>
                    </div>
                  </div>

                  {/* Lifecycle Management */}
                  <div className="flex gap-2 pt-2 border-t border-[#30363d]/60">
                    <button
                      onClick={async () => {
                        const runnerId = `runner_${sym.toLowerCase().replace("/", "_").replace("-", "_")}`;
                        await fetch(`/api/proxy/runners/${runnerId}/mode`, {
                          method: "POST",
                          headers: { "Content-Type": "application/json" },
                          body: JSON.stringify({ mode: "WIND_DOWN" }),
                        });
                        alert(`Wind Down initiated for ${sym}. No more buys will be placed.`);
                      }}
                      className="flex-1 py-1 text-[9px] font-semibold rounded bg-[#21262d] text-[#8b949e] hover:bg-amber-500/20 hover:text-amber-400 border border-[#30363d] transition"
                    >
                      SOFT SELL
                    </button>
                    <button
                      onClick={async () => {
                        const runnerId = `runner_${sym.toLowerCase().replace("/", "_").replace("-", "_")}`;
                        if(confirm(`Are you sure you want to hard sell all ${sym} inventory at market price?`)) {
                          await fetch(`/api/proxy/runners/${runnerId}/liquidate`, { method: "POST" });
                        }
                      }}
                      className="flex-1 py-1 text-[9px] font-semibold rounded bg-[#21262d] text-[#8b949e] hover:bg-[#f85149]/20 hover:text-[#f85149] border border-[#30363d] transition"
                    >
                      HARD SELL
                    </button>
                    <button
                      onClick={async () => {
                        const runnerId = `runner_${sym.toLowerCase().replace("/", "_").replace("-", "_")}`;
                        if(confirm(`Are you sure you want to stop tracking ${sym} completely?`)) {
                          await fetch(`/api/proxy/pairs/${runnerId}`, { method: "DELETE" });
                        }
                      }}
                      className="flex-1 py-1 text-[9px] font-semibold rounded bg-[#21262d] text-[#8b949e] hover:bg-[#8b949e]/20 hover:text-[#f0f6fc] border border-[#30363d] transition"
                    >
                      UNTRACK
                    </button>
                  </div>

                </div>
              </div>
            );
          })}
        </div>
      </section>

      {/* 6. COMPREHENSIVE TRADE & EXECUTION HISTORY TABLE */}
      <section className={`bg-[#161b22] border border-[#30363d] rounded-xl p-4 shadow-sm space-y-3 ${mobileTab === "history" ? "block" : "hidden md:block"}`}>
        <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5">
          <div className="flex items-center gap-2">
            <FileText className="w-4 h-4 text-[#8b949e]" />
            <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
              Execution & Trade History
            </span>
            <span className="text-[10px] px-2 py-0.2 rounded bg-[#0d1117] text-[#8b949e] font-mono border border-[#30363d]">
              {filteredTrades.length} Recorded
            </span>
          </div>

          <div className="flex gap-1 text-[10px] font-mono">
            {["ALL", ...activeAssets].map((f) => (
              <button
                key={f}
                onClick={() => setHistoryFilter(f)}
                className={`px-2 py-0.5 rounded transition ${
                  historyFilter === f
                    ? "bg-[#21262d] text-[#f0f6fc] border border-[#30363d]"
                    : "text-[#8b949e] hover:text-[#c9d1d9]"
                }`}
              >
                {f}
              </button>
            ))}
          </div>
        </div>

        {/* Mobile Card List View (Touch-friendly for small screens) */}
        <div className="block md:hidden space-y-2.5">
          {filteredTrades.length === 0 ? (
            <div className="py-8 text-center text-[#8b949e] text-xs font-sans border border-[#30363d]/30 rounded-lg">
              No executed trades recorded yet. Awaiting fills on Revolut X.
            </div>
          ) : (
            filteredTrades.map((t) => {
              const isBuy = (t.side || t.action || "").toUpperCase().includes("BUY");
              const price = typeof t.price === "number" ? t.price : parseFloat(String(t.price)) || 0;
              const qty = typeof t.qty === "number" ? t.qty : parseFloat(String(t.qty)) || 0;
              const val = t.value_gbp ?? (price * qty);
              const pnl = t.pnl_gbp ?? t.profit ?? 0.0;
              const timeStr = String(t.time_str || t.timestamp || "12:00:00");
              const strat = t.strategy || (t.action?.includes("SNIPE") ? "Lead-Lag" : "Maker Grid");
              const pnlPct = t.pnl_pct ?? (val > 0 && pnl !== 0 ? (pnl / val) * 100 : null);

              return (
                <div key={t.id} className="bg-[#0d1117] border border-[#30363d] rounded-lg p-3 space-y-2 font-mono text-xs">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${
                        isBuy ? "text-emerald-400 bg-emerald-500/10 border border-emerald-500/20" : "text-[#f85149] bg-[#f85149]/10 border border-[#f85149]/20"
                      }`}>
                        {isBuy ? "BUY" : "SELL"}
                      </span>
                      <span className="font-bold text-sm text-[#f0f6fc]">{t.symbol}</span>
                      <span className="text-[10px] text-[#8b949e] bg-[#161b22] px-1.5 py-0.5 rounded border border-[#30363d]">
                        {strat}
                      </span>
                    </div>
                    <span className="text-[10px] text-[#8b949e]">{timeStr}</span>
                  </div>

                  <div className="grid grid-cols-2 gap-2 text-[11px] pt-2 border-t border-[#30363d]/40">
                    <div>
                      <span className="text-[#8b949e]">Exec Price: </span>
                      <span className="text-[#f0f6fc] font-semibold">
                        £{price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 4 })}
                      </span>
                    </div>
                    <div className="text-right">
                      <span className="text-[#8b949e]">Cost / Val: </span>
                      <span className="text-[#f0f6fc] font-semibold">£{val.toFixed(2)}</span>
                    </div>
                    <div>
                      <span className="text-[#8b949e]">Quantity: </span>
                      <span className="text-[#8b949e]">{qty}</span>
                    </div>
                    <div className="text-right">
                      <span className="text-[#8b949e]">Net PnL: </span>
                      {isBuy ? (
                        <span className="text-[#58a6ff] text-[10px] font-semibold bg-[#58a6ff]/10 px-1.5 py-0.5 rounded">
                          BUY · OPEN
                        </span>
                      ) : (
                        <span className={`font-bold ${pnl > 0 ? "text-emerald-400" : pnl < 0 ? "text-[#f85149]" : "text-[#8b949e]"}`}>
                          {formatPnL(pnl)} {pnlPct !== null && <span className="text-[10px] font-normal font-sans">({pnlPct > 0 ? "+" : ""}{pnlPct.toFixed(2)}%)</span>}
                        </span>
                      )}
                    </div>
                  </div>
                </div>
              );
            })
          )}
        </div>

        {/* Desktop 10-Column Financial Table */}
        <div className="hidden md:block overflow-x-auto">
          <table className="w-full text-left font-mono text-xs">
            <thead className="text-[10px] text-[#8b949e] border-b border-[#30363d]/60 bg-[#161b22]">
              <tr>
                <th className="pb-2 font-normal">TIME</th>
                <th className="pb-2 font-normal">ASSET</th>
                <th className="pb-2 font-normal">SIDE</th>
                <th className="pb-2 font-normal text-right">PRICE</th>
                <th className="pb-2 font-normal text-right">QUANTITY</th>
                <th className="pb-2 font-normal text-right">EXEC FX</th>
                <th className="pb-2 font-normal text-right">COST BASIS (GBP)</th>
                <th className="pb-2 font-normal text-right">FEE (GBP)</th>
                <th className="pb-2 font-normal text-right">NET PNL</th>
                <th className="pb-2 font-normal text-right">STRATEGY</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[#30363d]/30 text-[11px]">
              {filteredTrades.length === 0 ? (
                <tr>
                  <td colSpan={10} className="py-12 text-center text-[#8b949e] text-xs font-sans">
                    No executed trades recorded yet. Awaiting fills on Revolut X.
                  </td>
                </tr>
              ) : (
                filteredTrades.map((t) => {
                  const isBuy = (t.side || t.action || "").toUpperCase().includes("BUY");
                  const price = typeof t.price === "number" ? t.price : parseFloat(String(t.price)) || 0;
                  const qty = typeof t.qty === "number" ? t.qty : parseFloat(String(t.qty)) || 0;
                  const val = t.value_gbp ?? (price * qty);
                  const fee = t.fee_gbp ?? (val * 0.0009);
                  const pnl = t.pnl_gbp ?? t.profit ?? 0.0;
                  const fx_rate = t.fx_rate ?? 1.0;
                  const timeStr = String(t.time_str || t.timestamp || "12:00:00");
                  const strat = t.strategy || (t.action?.includes("SNIPE") ? "Lead-Lag Dislocation" : "Maker Grid");
                  const pnlPct = t.pnl_pct ?? (val > 0 && pnl !== 0 ? (pnl / val) * 100 : null);

                  return (
                    <tr key={t.id} className="hover:bg-[#21262d]/40 transition">
                      <td className="py-2 text-[#8b949e]">{timeStr}</td>
                      <td className="py-2 font-semibold text-[#f0f6fc]">{t.symbol}</td>
                      <td className="py-2">
                        <span className={`px-1.5 py-0.2 rounded text-[10px] font-bold ${
                          isBuy ? "text-emerald-400 bg-emerald-500/10" : "text-[#f85149] bg-[#f85149]/10"
                        }`}>
                          {isBuy ? "BUY" : "SELL"}
                        </span>
                      </td>
                      <td className="py-2 text-right text-[#f0f6fc]">
                        {price.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 6 })}
                      </td>
                      <td className="py-2 text-right text-[#8b949e]">{qty}</td>
                      <td className="py-2 text-right text-[#8b949e]">
                        {fx_rate !== 1.0 ? fx_rate.toFixed(4) : "1.0000"}
                      </td>
                      <td className="py-2 text-right font-medium text-[#f0f6fc]">
                        £{val.toFixed(2)}
                      </td>
                      <td className="py-2 text-right text-[#8b949e]">
                        £{fee.toFixed(3)}
                      </td>
                      <td className={`py-2 text-right font-medium`}>
                        {isBuy ? (
                          <span className="text-[#58a6ff] text-[10px] font-medium bg-[#58a6ff]/10 px-1.5 py-0.2 rounded">
                            BUY · OPEN
                          </span>
                        ) : (
                          <span className={`${pnl > 0 ? "text-emerald-400" : pnl < 0 ? "text-[#f85149]" : "text-[#8b949e]"}`}>
                            {formatPnL(pnl)} {pnlPct !== null && <span className="text-[10px] font-normal text-[#8b949e]">({pnlPct > 0 ? "+" : ""}{pnlPct.toFixed(2)}%)</span>}
                          </span>
                        )}
                      </td>
                      <td className="py-2 text-right text-[#8b949e] text-[10px]">
                        {strat}
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </section>

      {/* 7. LIVE RESTING ORDERS (Revolut X Spot Book) */}
      <section className={`bg-[#161b22] border border-[#30363d] rounded-xl p-4 shadow-sm space-y-3 ${mobileTab === "orders" ? "block" : "hidden md:block"}`}>
        <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5">
          <div className="flex items-center gap-2">
            <Clock className="w-4 h-4 text-[#8b949e]" />
            <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
              Live Resting Orders (Revolut X Spot Book)
            </span>
            <span className="text-[10px] px-1.5 py-0.2 rounded bg-[#21262d] text-[#8b949e] font-mono">
              {filteredOrders.length} Active
            </span>
          </div>

          <div className="flex gap-1 text-[10px] font-mono">
            {["ALL", ...activeAssets].map((f) => (
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

        <div className="max-h-[500px] overflow-y-auto pr-1">
          {filteredOrders.length === 0 ? (
            <div className="py-12 text-center text-[#8b949e] text-xs font-sans border border-[#30363d]/30 rounded-lg">
              No open resting limit orders on Revolut X.
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {Array.from(new Set(filteredOrders.map(o => o.symbol))).map(sym => {
                const symOrders = filteredOrders.filter(o => o.symbol === sym);
                const asks = symOrders
                  .filter(o => o.side === "SELL")
                  .sort((a, b) => (Number(b.price) || 0) - (Number(a.price) || 0));
                const bids = symOrders
                  .filter(o => o.side === "BUY")
                  .sort((a, b) => (Number(b.price) || 0) - (Number(a.price) || 0));
                const maxQty = Math.max(...symOrders.map(o => Number(o.qty) || 0), 0.000001);
                
                return (
                  <div key={sym} className="bg-[#0d1117] border border-[#30363d] rounded-lg overflow-hidden flex flex-col font-mono text-[10px]">
                    <div className="bg-[#21262d] px-3 py-1.5 font-bold text-[#f0f6fc] text-xs border-b border-[#30363d] flex justify-between">
                      <span>{sym} Order Book</span>
                      <span className="text-[#8b949e] text-[9px]">{symOrders.length} Resting</span>
                    </div>
                    
                    <div className="flex text-[#8b949e] px-2 py-1 border-b border-[#30363d]/40">
                      <div className="flex-1">PRICE</div>
                      <div className="flex-1 text-right">SIZE</div>
                      <div className="flex-1 text-right">DIST</div>
                    </div>

                    <div className="flex flex-col flex-1 pb-1">
                      {/* ASKS */}
                      {asks.map(ask => {
                        const pNum = typeof ask.price === "number" ? ask.price : parseFloat(String(ask.price)) || 0;
                        const qNum = typeof ask.qty === "number" ? ask.qty : parseFloat(String(ask.qty)) || 0;
                        const dNum = typeof ask.distance_pct === "number" ? ask.distance_pct : parseFloat(String(ask.distance_pct)) || 0;
                        const w = maxQty > 0 ? (qNum / maxQty) * 100 : 0;
                        return (
                          <div key={ask.id} className="relative flex px-2 py-0.5 group">
                            <div className="absolute top-0 bottom-0 right-0 bg-[#f85149]/10" style={{ width: `${w}%` }} />
                            <div className="flex-1 text-[#f85149] z-10">{pNum.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}</div>
                            <div className="flex-1 text-right text-[#c9d1d9] z-10">{qNum.toFixed(6)}</div>
                            <div className="flex-1 text-right text-[#f85149] z-10">+{dNum.toFixed(2)}%</div>
                          </div>
                        )
                      })}
                      
                      {/* SPREAD INDICATOR */}
                      <div className="my-1.5 flex items-center justify-center border-y border-[#30363d]/40 py-1 bg-[#161b22]">
                        <span className="text-[10px] text-[#8b949e] font-semibold">
                          MARKET MID
                        </span>
                      </div>

                      {/* BIDS */}
                      {bids.map(bid => {
                        const pNum = typeof bid.price === "number" ? bid.price : parseFloat(String(bid.price)) || 0;
                        const qNum = typeof bid.qty === "number" ? bid.qty : parseFloat(String(bid.qty)) || 0;
                        const dNum = typeof bid.distance_pct === "number" ? bid.distance_pct : parseFloat(String(bid.distance_pct)) || 0;
                        const w = maxQty > 0 ? (qNum / maxQty) * 100 : 0;
                        return (
                          <div key={bid.id} className="relative flex px-2 py-0.5 group">
                            <div className="absolute top-0 bottom-0 right-0 bg-emerald-500/10" style={{ width: `${w}%` }} />
                            <div className="flex-1 text-emerald-400 z-10">{pNum.toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}</div>
                            <div className="flex-1 text-right text-[#c9d1d9] z-10">{qNum.toFixed(6)}</div>
                            <div className="flex-1 text-right text-emerald-400 z-10">{dNum.toFixed(2)}%</div>
                          </div>
                        )
                      })}
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      </section>

      {/* 8. PARAMETER CONFIGURATION (When viewMode === "tuning" or mobileTab === "config") */}
      {(viewMode === "tuning" || mobileTab === "config") && (
        <section className={`bg-[#161b22] border border-[#30363d] rounded-xl p-4 space-y-4 shadow-sm ${mobileTab === "config" ? "block" : "hidden md:block"}`}>
          <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5">
            <div className="flex items-center gap-2">
              <Sliders className="w-4 h-4 text-[#8b949e]" />
              <span className="text-xs font-semibold text-[#f0f6fc] uppercase tracking-wider">
                Strategy Parameter Configuration
              </span>
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-4 gap-4 text-xs font-mono">
            {(telemetry?.runners || []).map((r) => {
              const rId = r.runner_id;
              const currentStep = runnerParams[rId]?.step_pct || (r.step_pct ? (r.step_pct * 100).toFixed(2) : "0.40");
              const currentRebal = runnerParams[rId]?.rebalance_pct || (r.rebalance_threshold_pct ? (r.rebalance_threshold_pct * 100).toFixed(1) : "2.0");

              return (
                <div key={rId} className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg space-y-2.5">
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-[#f0f6fc]">{r.symbol} Grid</span>
                    <span className={`text-[10px] px-1.5 py-0.2 rounded font-mono ${r.is_paused ? "bg-amber-500/10 text-amber-400" : "bg-emerald-500/10 text-emerald-400"}`}>
                      {r.is_paused ? "PAUSED" : "ACTIVE"}
                    </span>
                  </div>
                  <div className="space-y-2 text-[#8b949e]">
                    <div>
                      <label className="text-[10px] block mb-1 uppercase tracking-wider">Step Spacing (%)</label>
                      <input
                        type="number"
                        step="0.05"
                        value={currentStep}
                        onChange={(e) =>
                          setRunnerParams({
                            ...runnerParams,
                            [rId]: {
                              step_pct: e.target.value,
                              rebalance_pct: currentRebal,
                            },
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
                        value={currentRebal}
                        onChange={(e) =>
                          setRunnerParams({
                            ...runnerParams,
                            [rId]: {
                              step_pct: currentStep,
                              rebalance_pct: e.target.value,
                            },
                          })
                        }
                        className="w-full bg-[#161b22] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-[#58a6ff] outline-none"
                      />
                    </div>
                  </div>
                  <button
                    onClick={() => handleTuneRunner(rId)}
                    className="w-full bg-[#21262d] hover:bg-[#30363d] text-[#c9d1d9] border border-[#30363d] py-1.5 rounded transition text-xs font-medium"
                  >
                    Apply {r.symbol} Parameters
                  </button>
                </div>
              );
            })}

            {/* Capital Allocation */}
            <div className="bg-[#0d1117] border border-[#30363d] p-3 rounded-lg space-y-2.5">
              <span className="font-medium text-[#f0f6fc]">Capital Allocation</span>
              <div className="space-y-2 text-[#8b949e]">
                <div>
                  <div className="flex justify-between text-[10px] uppercase tracking-wider mb-1">
                    <span>Profit Retain</span>
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
                    <span>BTC Allocation</span>
                    <span className="text-[#c9d1d9]">{capitalParams.split_btc_pct}%</span>
                  </div>
                  <input
                    type="range"
                    min="10"
                    max="80"
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
        </section>
      )}

      {/* 9. Emergency Halt Modal */}
      {killModalOpen && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="bg-[#161b22] border border-[#30363d] rounded-xl max-w-sm w-full p-5 space-y-4 shadow-xl">
            <div className="space-y-1">
              <h3 className="text-sm font-semibold text-[#f0f6fc]">Emergency Halt Strategy Execution</h3>
              <p className="text-xs text-[#8b949e]">
                This will trip the central circuit breaker, cancel resting orders across Revolut X, and halt automated entries.
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
                Halt Execution
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 10. Add Trading Pair Modal */}
      {addPairModalOpen && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="bg-[#161b22] border border-[#30363d] rounded-xl max-w-md w-full p-5 space-y-4 shadow-xl font-mono text-xs">
            <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5">
              <div className="flex items-center gap-2">
                <Coins className="w-4 h-4 text-emerald-400" />
                <h3 className="text-sm font-semibold text-[#f0f6fc]">Hot-Spawn Trading Pair</h3>
              </div>
              <button
                onClick={() => setAddPairModalOpen(false)}
                className="text-[#8b949e] hover:text-[#f0f6fc]"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <form onSubmit={handleAddPair} className="space-y-3">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[10px] text-[#8b949e] block mb-1 uppercase tracking-wider">Base Asset</label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. BTC, ETH, SOL, XRP"
                    value={newPair.base}
                    onChange={(e) => setNewPair({ ...newPair, base: e.target.value.toUpperCase() })}
                    className="w-full bg-[#0d1117] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-emerald-500 outline-none uppercase font-semibold"
                  />
                </div>
                <div>
                  <label className="text-[10px] text-[#8b949e] block mb-1 uppercase tracking-wider">Quote Currency</label>
                  <select
                    value={newPair.quote}
                    onChange={(e) => setNewPair({ ...newPair, quote: e.target.value })}
                    className="w-full bg-[#0d1117] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-emerald-500 outline-none font-semibold"
                  >
                    <option value="USD">USD ($)</option>
                    <option value="GBP">GBP (£)</option>
                    <option value="EUR">EUR (€)</option>
                  </select>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[10px] text-[#8b949e] block mb-1 uppercase tracking-wider">Capital Envelope</label>
                  <input
                    type="number"
                    step="10"
                    required
                    value={newPair.envelope_capital}
                    onChange={(e) => setNewPair({ ...newPair, envelope_capital: e.target.value })}
                    className="w-full bg-[#0d1117] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-emerald-500 outline-none"
                  />
                </div>
                <div>
                  <label className="text-[10px] text-[#8b949e] block mb-1 uppercase tracking-wider">Order Size (Fiat)</label>
                  <input
                    type="number"
                    step="5"
                    required
                    value={newPair.order_size_fiat}
                    onChange={(e) => setNewPair({ ...newPair, order_size_fiat: e.target.value })}
                    className="w-full bg-[#0d1117] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-emerald-500 outline-none"
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[10px] text-[#8b949e] block mb-1 uppercase tracking-wider">Grid Step (%)</label>
                  <input
                    type="number"
                    step="0.05"
                    required
                    value={newPair.grid_step_pct}
                    onChange={(e) => setNewPair({ ...newPair, grid_step_pct: e.target.value })}
                    className="w-full bg-[#0d1117] border border-[#30363d] px-2.5 py-1.5 rounded text-[#f0f6fc] focus:border-emerald-500 outline-none"
                  />
                </div>
                <div className="flex items-center gap-2 pt-4">
                  <input
                    type="checkbox"
                    id="sniper_cb"
                    checked={newPair.sniper_enabled}
                    onChange={(e) => setNewPair({ ...newPair, sniper_enabled: e.target.checked })}
                    className="accent-emerald-500 w-4 h-4 cursor-pointer"
                  />
                  <label htmlFor="sniper_cb" className="text-xs text-[#f0f6fc] cursor-pointer">
                    Enable Sniper
                  </label>
                </div>
              </div>

              <p className="text-[10px] text-[#8b949e] bg-[#0d1117] p-2 rounded border border-[#30363d]">
                Hot-spawning registers capital in CentralRiskEngine, subscribes to Kraken WS v2 live ticker, and launches Grid &amp; Sniper Tokio tasks with zero daemon downtime.
              </p>

              <div className="flex items-center justify-end gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => setAddPairModalOpen(false)}
                  className="px-3 py-1.5 rounded-lg text-[#8b949e] hover:text-[#f0f6fc] transition"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={submittingPair}
                  className="px-3 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-medium transition flex items-center gap-1.5"
                >
                  {submittingPair && <RefreshCw className="w-3 h-3 animate-spin" />}
                  <span>Hot-Spawn Pair</span>
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* 11. Edit Deposited Cash (Cost Basis) Modal */}
      {depositModalOpen && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-xs flex items-center justify-center p-4 z-50">
          <div className="bg-[#161b22] border border-[#30363d] rounded-xl max-w-sm w-full p-5 space-y-4 shadow-xl font-mono text-xs">
            <div className="flex items-center justify-between border-b border-[#30363d] pb-2.5">
              <div className="flex items-center gap-2">
                <Wallet className="w-4 h-4 text-[#58a6ff]" />
                <h3 className="text-sm font-semibold text-[#f0f6fc]">Set Total Deposited Cash</h3>
              </div>
              <button
                onClick={() => setDepositModalOpen(false)}
                className="text-[#8b949e] hover:text-[#f0f6fc]"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <p className="text-[11px] text-[#8b949e]">
              Enter the exact total GBP cash you have deposited into your Revolut X account. The terminal uses this cost basis to accurately compute your true Net PnL vs Inflows.
            </p>

            <div className="space-y-2">
              <label className="text-[10px] text-[#8b949e] block uppercase tracking-wider">Total Cash Deposited (£ GBP)</label>
              <div className="relative">
                <span className="absolute left-3 top-2 text-[#8b949e] font-bold">£</span>
                <input
                  type="number"
                  step="0.01"
                  min="1"
                  value={depositedCashInput}
                  onChange={(e) => setDepositedCashInput(e.target.value)}
                  placeholder="e.g. 25.00"
                  className="w-full bg-[#0d1117] border border-[#30363d] pl-7 pr-3 py-1.5 rounded text-lg font-bold text-[#f0f6fc] focus:border-[#58a6ff] outline-none"
                />
              </div>

              {/* Quick Presets */}
              <div className="flex gap-1.5 pt-1">
                {[10, 25, 50, 100, 250].map((amt) => (
                  <button
                    key={amt}
                    type="button"
                    onClick={() => setDepositedCashInput(amt.toFixed(2))}
                    className={`flex-1 py-1 rounded text-[10px] border transition ${
                      parseFloat(depositedCashInput) === amt
                        ? "bg-[#21262d] text-[#f0f6fc] border-[#58a6ff]"
                        : "bg-[#0d1117] text-[#8b949e] border-[#30363d] hover:text-[#c9d1d9]"
                    }`}
                  >
                    £{amt}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 pt-2 border-t border-[#30363d]/50">
              <button
                type="button"
                onClick={() => setDepositModalOpen(false)}
                className="px-3 py-1.5 rounded-lg text-[#8b949e] hover:text-[#f0f6fc] transition"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={updatingDeposit}
                onClick={() => handleUpdateDepositedCash(parseFloat(depositedCashInput) || 25.00)}
                className="px-3 py-1.5 rounded-lg bg-[#58a6ff] hover:bg-blue-500 text-white font-medium transition flex items-center gap-1.5"
              >
                {updatingDeposit && <RefreshCw className="w-3 h-3 animate-spin" />}
                <span>Save Cost Basis</span>
              </button>
            </div>
          </div>
        </div>
      )}
      </div>

      {/* Mobile Bottom Navigation */}
      <nav className="md:hidden fixed bottom-0 left-0 right-0 bg-[#161b22] border-t border-[#30363d] flex items-center justify-around pb-safe pt-1 z-40">
        <button
          onClick={() => setMobileTab("dashboard")}
          className={`flex flex-col items-center p-2 flex-1 ${mobileTab === "dashboard" ? "text-emerald-400" : "text-[#8b949e]"}`}
        >
          <Activity className="w-5 h-5 mb-1" />
          <span className="text-[9px] font-medium uppercase tracking-widest">Dash</span>
        </button>
        <button
          onClick={() => setMobileTab("orders")}
          className={`flex flex-col items-center p-2 flex-1 ${mobileTab === "orders" ? "text-emerald-400" : "text-[#8b949e]"}`}
        >
          <Clock className="w-5 h-5 mb-1" />
          <span className="text-[9px] font-medium uppercase tracking-widest">Orders</span>
        </button>
        <button
          onClick={() => setMobileTab("history")}
          className={`flex flex-col items-center p-2 flex-1 ${mobileTab === "history" ? "text-emerald-400" : "text-[#8b949e]"}`}
        >
          <BarChart2 className="w-5 h-5 mb-1" />
          <span className="text-[9px] font-medium uppercase tracking-widest">History</span>
        </button>
        <button
          onClick={() => setMobileTab("config")}
          className={`flex flex-col items-center p-2 flex-1 ${mobileTab === "config" ? "text-emerald-400" : "text-[#8b949e]"}`}
        >
          <Sliders className="w-5 h-5 mb-1" />
          <span className="text-[9px] font-medium uppercase tracking-widest">Config</span>
        </button>
      </nav>
    </main>
  );
}
