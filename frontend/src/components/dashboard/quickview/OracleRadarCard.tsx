"use client";

import React from "react";
import { TelemetryPayload } from "../../../types/telemetry";
import { formatGbp, toNum } from "../../../lib/format";
import { Radar, Zap, ShieldAlert, Cpu } from "lucide-react";

interface OracleRadarCardProps {
  telemetry: TelemetryPayload | null;
}

export const OracleRadarCard: React.FC<OracleRadarCardProps> = ({ telemetry }) => {
  const runner = telemetry?.runners?.[0];
  const sniper = telemetry?.sniper;
  const radarItem = sniper?.radar?.["XRP/GBP"] || sniper?.radar?.["SOL/GBP"] || (sniper?.radar ? Object.values(sniper.radar)[0] : undefined);

  const revolutMid = toNum(runner?.effective_center ?? runner?.center_price, 0);
  const binancePrice = toNum(radarItem?.kraken_price, revolutMid > 0 ? revolutMid : 0); // Dual oracle lead price
  const dislocationPct = toNum(
    radarItem?.current_dislocation_pct,
    revolutMid > 0 ? ((binancePrice - revolutMid) / revolutMid) * 100 : 0
  );

  const stepPct = toNum(runner?.dynamic_step_pct ?? runner?.step_pct, 0.0035) * 100;
  const isCircuitBreakerNormal = !telemetry?.circuit_breaker_tripped;

  return (
    <div className="nordic-glass rounded-2xl p-4 relative overflow-hidden shadow-2xl transition-all duration-300">
      {/* Top Header */}
      <div className="flex items-center justify-between mb-3 text-xs">
        <div className="flex items-center gap-1.5 text-slate-400 font-medium">
          <Radar className="w-3.5 h-3.5 text-sky-400 animate-spin-slow" />
          <span>DUAL-ORACLE RADAR</span>
        </div>
        <div className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-teal-950/60 border border-teal-800/40 text-[10px] text-teal-300">
          <Zap className="w-3 h-3 text-teal-400" />
          <span>0.00% MAKER EDGE</span>
        </div>
      </div>

      {/* Dual Price Comparison Row */}
      <div className="grid grid-cols-2 gap-2 mt-2">
        {/* Binance Leader Oracle */}
        <div className="bg-slate-900/60 p-2.5 rounded-xl border border-sky-900/40">
          <div className="flex items-center justify-between text-[10px] text-slate-400">
            <span>BINANCE ORACLE</span>
            <span className="text-sky-400 text-[9px] font-mono">LEAD</span>
          </div>
          <div className="text-lg font-bold text-sky-300 font-sans mt-0.5">
            {formatGbp(binancePrice, binancePrice < 10 ? 4 : 2)}
          </div>
          <div className="text-[10px] text-slate-400">Real-time Stream</div>
        </div>

        {/* Revolut X Resting Mid */}
        <div className="bg-slate-900/60 p-2.5 rounded-xl border border-slate-800/80">
          <div className="flex items-center justify-between text-[10px] text-slate-400">
            <span>REVOLUT X MID</span>
            <span className="text-slate-400 text-[9px] font-mono">VENUE</span>
          </div>
          <div className="text-lg font-bold text-slate-100 font-sans mt-0.5">
            {formatGbp(revolutMid, revolutMid < 10 ? 4 : 2)}
          </div>
          <div className="text-[10px] text-slate-400">Resting Center</div>
        </div>
      </div>

      {/* Dislocation Gauge Bar */}
      <div className="mt-3 p-2.5 rounded-xl bg-slate-900/50 border border-slate-800/60">
        <div className="flex items-center justify-between text-xs mb-1.5">
          <span className="text-slate-400 text-[11px]">Venue Dislocation</span>
          <span
            className={`font-mono text-[11px] font-bold ${
              Math.abs(dislocationPct) > 0.05 ? "text-cyan-400" : "text-slate-300"
            }`}
          >
            {dislocationPct >= 0 ? "+" : ""}
            {dislocationPct.toFixed(3)}%
          </span>
        </div>
        <div className="w-full bg-slate-800/80 h-1.5 rounded-full overflow-hidden relative">
          <div
            className="h-full bg-gradient-to-r from-sky-500 to-cyan-300 rounded-full transition-all duration-500"
            style={{
              width: `${Math.min(100, Math.max(10, (Math.abs(dislocationPct) / 0.2) * 100))}%`,
            }}
          />
        </div>
      </div>

      {/* Engine Architecture Metadata */}
      <div className="grid grid-cols-3 gap-2 mt-3 pt-2.5 border-t border-slate-800/50 text-[10px]">
        <div className="bg-slate-900/40 p-2 rounded-lg border border-slate-800/40">
          <span className="text-slate-400 block">GRID STEP</span>
          <span className="font-mono font-bold text-slate-200 mt-0.5 block">
            {stepPct.toFixed(3)}%
          </span>
        </div>
        <div className="bg-slate-900/40 p-2 rounded-lg border border-slate-800/40">
          <span className="text-slate-400 block">SKEW (γ)</span>
          <span className="font-mono font-bold text-cyan-400 mt-0.5 block">
            0.0800
          </span>
        </div>
        <div className="bg-slate-900/40 p-2 rounded-lg border border-slate-800/40">
          <span className="text-slate-400 block">CIRCUIT</span>
          <span
            className={`font-mono font-bold mt-0.5 block ${
              isCircuitBreakerNormal ? "text-teal-400" : "text-rose-400"
            }`}
          >
            {isCircuitBreakerNormal ? "NORMAL" : "TRIPPED"}
          </span>
        </div>
      </div>
    </div>
  );
};
