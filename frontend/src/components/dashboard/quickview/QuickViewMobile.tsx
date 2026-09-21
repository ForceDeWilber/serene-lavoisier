"use client";

import React from "react";
import { TelemetryPayload } from "../../../types/telemetry";
import { HeroPerformanceCard } from "./HeroPerformanceCard";
import { AllocationDonutCard } from "./AllocationDonutCard";
import { OracleRadarCard } from "./OracleRadarCard";
import { OrderLadderCard } from "./OrderLadderCard";
import { RefreshCw, Zap } from "lucide-react";

interface QuickViewMobileProps {
  telemetry: TelemetryPayload | null;
  onSyncRevolut: () => void;
  syncingRevolut: boolean;
  onToggleSniper: () => void;
}

export const QuickViewMobile: React.FC<QuickViewMobileProps> = ({
  telemetry,
  onSyncRevolut,
  syncingRevolut,
  onToggleSniper,
}) => {
  const isSniperArmed = Boolean(telemetry?.sniper?.enabled);

  return (
    <div className="flex-1 overflow-y-auto overscroll-contain px-3 py-2.5 space-y-3 pb-24 select-none">
      {/* Quick Action Top Glance Bar */}
      <div className="flex items-center justify-between px-3 py-2 rounded-xl bg-slate-900/90 border border-slate-800 shadow-lg">
        <div className="flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-teal-400 animate-pulse" />
          <span className="text-xs font-semibold text-slate-200">
            SOL/GBP Pure Maker
          </span>
        </div>

        <div className="flex items-center gap-2">
          {/* Quick Balance Sync */}
          <button
            onClick={onSyncRevolut}
            disabled={syncingRevolut}
            className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 text-[11px] font-medium border border-slate-700 transition-all disabled:opacity-50"
          >
            <RefreshCw
              className={`w-3 h-3 text-sky-400 ${
                syncingRevolut ? "animate-spin" : ""
              }`}
            />
            <span>{syncingRevolut ? "Syncing..." : "Sync API"}</span>
          </button>

          {/* Sniper Toggle Badge */}
          <button
            onClick={onToggleSniper}
            className={`flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-bold border transition-all ${
              isSniperArmed
                ? "bg-teal-950 text-teal-300 border-teal-800"
                : "bg-slate-800/80 text-slate-400 border-slate-700"
            }`}
          >
            <Zap className={`w-3 h-3 ${isSniperArmed ? "text-teal-400" : "text-slate-500"}`} />
            <span>{isSniperArmed ? "ARMED" : "OFF"}</span>
          </button>
        </div>
      </div>

      {/* 1. Hero Performance & Compounding Scrubber */}
      <HeroPerformanceCard telemetry={telemetry} />

      {/* 2. Revolut X Capital Allocation Donut Ring */}
      <AllocationDonutCard telemetry={telemetry} />

      {/* 3. Dual-Oracle Arbitrage & Spread Radar */}
      <OracleRadarCard telemetry={telemetry} />

      {/* 4. Visual Order Depth Ladder & Live Fills */}
      <OrderLadderCard telemetry={telemetry} />
    </div>
  );
};
