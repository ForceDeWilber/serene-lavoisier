"use client";

import React, { useEffect, useState, useRef } from "react";
import { TelemetryPayload } from "../../types/telemetry";

import { TerminalHeader } from "../../components/dashboard/TerminalHeader";
import { AssetManagementPane } from "../../components/dashboard/AssetManagementPane";
import { PortfolioPane } from "../../components/dashboard/PortfolioPane";
import { SniperPane } from "../../components/dashboard/SniperPane";
import { GridPane } from "../../components/dashboard/GridPane";
import { OrderBookPane } from "../../components/dashboard/OrderBookPane";
import { TerminalLogPane } from "../../components/dashboard/TerminalLogPane";

export default function ProductionDashboard() {
  const [telemetry, setTelemetry] = useState<TelemetryPayload | null>(null);
  const [connected, setConnected] = useState<boolean>(false);
  const [isFeedStale, setIsFeedStale] = useState<boolean>(false);
  const [statusMessage, setStatusMessage] = useState<string>("");
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
        setIsFeedStale(false);
      }
    } catch {}
  };

  useEffect(() => {
    let active = true;

    fetchTelemetry();
    const pollInterval = setInterval(() => {
      if (active) fetchTelemetry();
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

  const handleKillSwitch = async () => {
    try {
      const res = await fetch(`/api/proxy/circuit-breaker/kill?mode=live`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason: "Manual halt triggered from terminal" }),
      });
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
        setStatusMessage("SYS: Circuit breaker reset");
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
      const res = await fetch("/api/proxy/pairs?mode=live", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ symbol: sym, ...config }),
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

  return (
    <main className="h-[100dvh] w-full bg-black text-gray-200 flex flex-col overflow-hidden font-mono text-xs">
      {/* 1. Terminal Header */}
      <TerminalHeader
        telemetry={telemetry}
        connected={connected}
        isFeedStale={isFeedStale}
        onKillSwitch={handleKillSwitch}
        onResetCircuitBreaker={handleResetCircuitBreaker}
        onLogout={handleLogout}
      />

      {/* 2. Grid Layout Body */}
      <div className="flex-1 grid grid-cols-12 grid-rows-[45%_55%] gap-1 p-1 overflow-hidden min-h-0">
        
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
