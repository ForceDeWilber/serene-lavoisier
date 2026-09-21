"use client";

import React, { useEffect, useState, useRef } from "react";
import { TelemetryPayload } from "../../types/telemetry";
import { formatGbp, formatPct, toNum } from "../../lib/format";

import { TerminalHeader } from "../../components/dashboard/TerminalHeader";
import { AssetManagementPane } from "../../components/dashboard/AssetManagementPane";
import { PortfolioPane } from "../../components/dashboard/PortfolioPane";
import { SniperPane } from "../../components/dashboard/SniperPane";
import { GridPane } from "../../components/dashboard/GridPane";
import { OrderBookPane } from "../../components/dashboard/OrderBookPane";
import { TerminalLogPane } from "../../components/dashboard/TerminalLogPane";

type MobileTab = "overview" | "sniper" | "grid" | "capital" | "orders" | "assets" | "logs";

export default function ProductionDashboard() {
  const [telemetry, setTelemetry] = useState<TelemetryPayload | null>(null);
  const [connected, setConnected] = useState<boolean>(false);
  const [isFeedStale, setIsFeedStale] = useState<boolean>(false);
  const [statusMessage, setStatusMessage] = useState<string>("");
  const [syncingRevolut, setSyncingRevolut] = useState<boolean>(false);
  const [mobileTab, setMobileTab] = useState<MobileTab>("overview");

  const lastSeenRef = useRef<number>(Date.now());
  const stalenessCheckTimer = useRef<NodeJS.Timeout | null>(null);

  const REFRESH_INTERVAL_MS = 3000;
  const STALE_THRESHOLD_MS = 7500;

  const fetchTelemetry = async () => {
    try {
      const res = await fetch(`/api/proxy/telemetry?mode=live`);
      if (res.ok) {
        const data: TelemetryPayload = await res.json();
        setTelemetry(data);
        setConnected(true);
        lastSeenRef.current = Date.now();
        setIsFeedStale(false);
      }
    } catch {}
  };

  useEffect(() => {
    let active = true;

    const tick = () => {
      if (active && typeof document !== "undefined" && document.visibilityState === "visible") {
        fetchTelemetry();
      }
    };

    tick();
    const pollInterval = setInterval(tick, REFRESH_INTERVAL_MS);

    const onVisibilityChange = () => {
      if (typeof document !== "undefined" && document.visibilityState === "visible") {
        tick();
      }
    };
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }

    stalenessCheckTimer.current = setInterval(() => {
      if (typeof document !== "undefined" && document.visibilityState !== "visible") return;
      const elapsed = Date.now() - lastSeenRef.current;
      setIsFeedStale(elapsed > STALE_THRESHOLD_MS);
    }, 1000);

    return () => {
      active = false;
      clearInterval(pollInterval);
      if (stalenessCheckTimer.current) clearInterval(stalenessCheckTimer.current);
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
    };
  }, []);

  const handleKillSwitch = async () => {
    try {
      let res = await fetch(`/api/proxy/emergency/kill-switch?mode=live`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: "Manual halt triggered from terminal" }),
      });
      if (!res.ok) {
        res = await fetch(`/api/proxy/circuit-breaker/kill?mode=live`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ reason: "Manual halt triggered from terminal" }),
        });
      }
      if (res.ok) {
        setStatusMessage("SYS: Circuit breaker tripped · Execution halted");
        fetchTelemetry();
      }
    } catch {}
  };

  const handleResetCircuitBreaker = async () => {
    try {
      const res = await fetch(`/api/proxy/circuit-breaker/reset?mode=live`, { method: "POST" });
      if (res.ok) {
        setStatusMessage("SYS: Circuit breaker reset · Execution resumed");
        fetchTelemetry();
      }
    } catch {}
  };

  const handleToggleSniper = async () => {
    const nextState = !telemetry?.sniper?.enabled;
    try {
      const res = await fetch(`/api/proxy/sniper/${nextState ? "arm" : "disarm"}?mode=live`, { method: "POST" });
      if (res.ok) {
        setStatusMessage(`SYS: Sniper ${nextState ? "ARMED" : "DISARMED"}`);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleSyncRevolutBalances = async () => {
    setSyncingRevolut(true);
    try {
      const res = await fetch("/api/proxy/capital/sync-revolut", { method: "POST" });
      if (res.ok) {
        setStatusMessage("SYS: Balances synchronized from Revolut X");
        fetchTelemetry();
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

  // Asset Management
  const handleAddPair = async (config: any) => {
    try {
      const sym = `${config.base}/${config.quote}`;
      const rawStep = parseFloat(String(config.grid_step_pct)) || 0.4;
      const stepPct = rawStep > 0.05 ? rawStep / 100 : rawStep;
      const res = await fetch("/api/proxy/pairs?mode=live", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          symbol: sym,
          base_asset: config.base,
          quote_asset: config.quote,
          envelope_capital: parseFloat(String(config.envelope_capital)) || 500.0,
          grid_step_pct: stepPct,
          grid_rungs: 5,
          order_size_fiat: parseFloat(String(config.order_size_fiat)) || 50.0,
          sniper_enabled: Boolean(config.sniper_enabled),
        }),
      });
      if (res.ok) {
        setStatusMessage(`SYS: Pair ${sym} hot-spawned`);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleRemovePair = async (runnerId: string) => {
    try {
      const res = await fetch(`/api/proxy/pairs/${runnerId}?mode=live`, { method: "DELETE" });
      if (res.ok) {
        setStatusMessage(`SYS: Removed pair ${runnerId}`);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleLiquidatePair = async (runnerId: string) => {
    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/liquidate?mode=live`, { method: "POST" });
      if (res.ok) {
        setStatusMessage(`SYS: Liquidated pair ${runnerId}`);
        fetchTelemetry();
      }
    } catch {}
  };

  const handleTuneRunner = async (runnerId: string, stepPct: string, rebalancePct: string) => {
    try {
      const res = await fetch(`/api/proxy/runners/${runnerId}/tune?mode=live`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          step_pct: parseFloat(stepPct) / 100,
          rebalance_threshold_pct: parseFloat(rebalancePct) / 100,
        }),
      });
      if (res.ok) {
        setStatusMessage(`SYS: Tuned ${runnerId}`);
        fetchTelemetry();
      }
    } catch {}
  };

  // Quick mobile metrics
  const gbpBalance = toNum(telemetry?.balances?.GBP, 0);
  const portfolio = telemetry?.portfolio;
  const totalEquity = toNum(portfolio?.total_equity_gbp, gbpBalance);
  const depositedCash = toNum(portfolio?.total_deposited_cash_gbp ?? portfolio?.initial_budget_gbp, 35.00);
  const netPnLGbp = toNum(portfolio?.total_pnl_gbp, totalEquity - depositedCash);
  const netPnLPct = toNum(portfolio?.total_pnl_pct, depositedCash > 0 ? (netPnLGbp / depositedCash) * 100 : 0);
  const isSniperArmed = Boolean(telemetry?.sniper?.enabled);

  return (
    <main className="h-[100dvh] w-full bg-black text-gray-200 flex flex-col overflow-hidden font-mono text-xs select-none">
      {/* 1. Terminal Header */}
      <TerminalHeader
        telemetry={telemetry}
        connected={connected}
        isFeedStale={isFeedStale}
        onKillSwitch={handleKillSwitch}
        onResetCircuitBreaker={handleResetCircuitBreaker}
        onLogout={handleLogout}
      />

      {/* 2. Mobile Glance Ticker (visible on mobile only) */}
      <div className="md:hidden bg-[#09090b] border-b border-gray-800 px-2 py-1.5 flex items-center justify-between text-[11px]">
        <div className="flex items-center gap-2">
          <div>
            <span className="text-gray-500 text-[9px] block leading-none">EQUITY</span>
            <span className="font-bold text-gray-100">{formatGbp(totalEquity, 2)}</span>
          </div>
          <div className="border-l border-gray-800 pl-2">
            <span className="text-gray-500 text-[9px] block leading-none">NET PNL</span>
            <span className={`font-semibold ${netPnLGbp >= 0 ? 'text-emerald-400' : 'text-red-400'}`}>
              {formatGbp(netPnLGbp, 2, true)} ({formatPct(netPnLPct, 1, true)})
            </span>
          </div>
        </div>
        <button
          onClick={handleToggleSniper}
          className={`px-2 py-0.5 text-[10px] border ${
            isSniperArmed
              ? 'bg-emerald-950/70 text-emerald-400 border-emerald-800'
              : 'bg-red-950/70 text-red-400 border-red-800'
          }`}
        >
          SNIPER: {isSniperArmed ? 'ARMED' : 'OFF'}
        </button>
      </div>

      {/* 3. Mobile Navigation Tabs (visible on mobile only) */}
      <div className="md:hidden bg-black border-b border-gray-800 flex overflow-x-auto scrollbar-none py-1 px-1.5 gap-1 text-[11px] flex-shrink-0">
        {(
          [
            { key: "overview", label: "OVERVIEW" },
            { key: "sniper", label: "SNIPER" },
            { key: "grid", label: "GRID" },
            { key: "capital", label: "CAPITAL" },
            { key: "orders", label: "ORDERS" },
            { key: "assets", label: "ASSETS" },
            { key: "logs", label: "LOGS" },
          ] as { key: MobileTab; label: string }[]
        ).map((tab) => (
          <button
            key={tab.key}
            onClick={() => setMobileTab(tab.key)}
            className={`px-2.5 py-1 transition-colors whitespace-nowrap ${
              mobileTab === tab.key
                ? "bg-gray-800 text-white font-bold border border-gray-700"
                : "text-gray-500 hover:text-gray-300 border border-transparent"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* 4. Mobile Body (visible on mobile only) */}
      <div className="md:hidden flex-1 overflow-hidden min-h-0 flex flex-col">
        {mobileTab === "overview" && (
          <div className="flex-1 overflow-y-auto p-1.5 space-y-2 pb-16">
            <div className="min-h-[250px]"><PortfolioPane telemetry={telemetry} onSyncRevolut={handleSyncRevolutBalances} syncingRevolut={syncingRevolut} /></div>
            <div className="min-h-[280px]"><SniperPane telemetry={telemetry} onToggleSniper={handleToggleSniper} /></div>
            <div className="min-h-[280px]"><GridPane telemetry={telemetry} onTuneRunner={handleTuneRunner} /></div>
            <div className="min-h-[250px]"><OrderBookPane telemetry={telemetry} /></div>
            <div className="min-h-[280px]"><AssetManagementPane telemetry={telemetry} onAddPair={handleAddPair} onRemovePair={handleRemovePair} onLiquidatePair={handleLiquidatePair} /></div>
            <div className="min-h-[250px]"><TerminalLogPane telemetry={telemetry} statusMessage={statusMessage} /></div>
          </div>
        )}

        {mobileTab === "sniper" && (
          <div className="flex-1 overflow-hidden p-1">
            <SniperPane telemetry={telemetry} onToggleSniper={handleToggleSniper} />
          </div>
        )}

        {mobileTab === "grid" && (
          <div className="flex-1 overflow-hidden p-1">
            <GridPane telemetry={telemetry} onTuneRunner={handleTuneRunner} />
          </div>
        )}

        {mobileTab === "capital" && (
          <div className="flex-1 overflow-hidden p-1">
            <PortfolioPane telemetry={telemetry} onSyncRevolut={handleSyncRevolutBalances} syncingRevolut={syncingRevolut} />
          </div>
        )}

        {mobileTab === "orders" && (
          <div className="flex-1 overflow-hidden p-1">
            <OrderBookPane telemetry={telemetry} />
          </div>
        )}

        {mobileTab === "assets" && (
          <div className="flex-1 overflow-hidden p-1">
            <AssetManagementPane telemetry={telemetry} onAddPair={handleAddPair} onRemovePair={handleRemovePair} onLiquidatePair={handleLiquidatePair} />
          </div>
        )}

        {mobileTab === "logs" && (
          <div className="flex-1 overflow-hidden p-1">
            <TerminalLogPane telemetry={telemetry} statusMessage={statusMessage} />
          </div>
        )}
      </div>

      {/* 5. Desktop 12-Column Grid Body (visible on desktop md+ only) */}
      <div className="hidden md:grid md:grid-cols-12 md:grid-rows-[45%_55%] gap-1 p-1 overflow-hidden min-h-0 flex-1">
        {/* Top Row */}
        <div className="col-span-3 h-full min-h-0">
          <PortfolioPane 
            telemetry={telemetry} 
            onSyncRevolut={handleSyncRevolutBalances} 
            syncingRevolut={syncingRevolut} 
          />
        </div>
        <div className="col-span-3 h-full min-h-0">
          <AssetManagementPane 
            telemetry={telemetry}
            onAddPair={handleAddPair}
            onRemovePair={handleRemovePair}
            onLiquidatePair={handleLiquidatePair}
          />
        </div>
        <div className="col-span-6 h-full min-h-0">
          <SniperPane 
            telemetry={telemetry}
            onToggleSniper={handleToggleSniper}
          />
        </div>

        {/* Bottom Row */}
        <div className="col-span-5 h-full min-h-0">
          <GridPane 
            telemetry={telemetry}
            onTuneRunner={handleTuneRunner}
          />
        </div>
        <div className="col-span-3 h-full min-h-0">
          <OrderBookPane telemetry={telemetry} />
        </div>
        <div className="col-span-4 h-full min-h-0">
          <TerminalLogPane 
            telemetry={telemetry} 
            statusMessage={statusMessage} 
          />
        </div>
      </div>
    </main>
  );
}
