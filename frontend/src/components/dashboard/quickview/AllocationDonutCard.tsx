"use client";

import React, { useMemo } from "react";
import { TelemetryPayload } from "../../../types/telemetry";
import { formatGbp, toNum } from "../../../lib/format";
import { PieChart, CheckCircle2, Wallet, Layers, Coins } from "lucide-react";

interface AllocationDonutCardProps {
  telemetry: TelemetryPayload | null;
}

export const AllocationDonutCard: React.FC<AllocationDonutCardProps> = ({ telemetry }) => {
  const balances = telemetry?.balances;
  const portfolio = telemetry?.portfolio;
  const audit = portfolio?.transfer_audit || telemetry?.capital_management?.transfer_audit;

  const totalEquity = toNum(portfolio?.total_equity_gbp, 0);
  const settledCash = toNum(
    telemetry?.capital_management?.settled_cash_gbp ?? balances?.GBP,
    0
  );
  const costBasis = toNum(
    audit?.net_deposited_cash_gbp ?? portfolio?.net_deposited_cash_gbp,
    0
  );

  // Active resting buy order capital
  const activeOrders = telemetry?.active_orders || telemetry?.resting_orders || [];
  const activeTrapsCapital = useMemo(() => {
    return activeOrders
      .filter((o) => o.side === "BUY")
      .reduce((sum, o) => sum + toNum(o.value_gbp, 0), 0);
  }, [activeOrders]);

  // Crypto holdings value
  const cryptoHoldingsGbp = toNum(
    portfolio?.crypto_holdings_value_gbp ?? telemetry?.capital_management?.crypto_holdings_value_gbp,
    Math.max(0, totalEquity - settledCash)
  );

  const freeCash = Math.max(0, settledCash - activeTrapsCapital);

  // Slice calculations for SVG circular gauge
  const segments = useMemo(() => {
    const total = Math.max(0.01, totalEquity);
    const freePct = (freeCash / total) * 100;
    const trapsPct = (activeTrapsCapital / total) * 100;
    const cryptoPct = (cryptoHoldingsGbp / total) * 100;

    const cryptoLabel =
      balances?.XRP && toNum(balances.XRP, 0) > 0
        ? "XRP Holdings"
        : balances?.SOL && toNum(balances.SOL, 0) > 0
        ? "SOL Holdings"
        : "Crypto Holdings";

    return [
      {
        id: "free_cash",
        label: "Free Cash (GBP)",
        amount: freeCash,
        pct: freePct,
        color: "#38bdf8", // Glacial Cyan
        icon: Wallet,
      },
      {
        id: "active_traps",
        label: "Resting Buy Traps",
        amount: activeTrapsCapital,
        pct: trapsPct,
        color: "#3b82f6", // Arctic Blue
        icon: Layers,
      },
      {
        id: "crypto_holdings",
        label: cryptoLabel,
        amount: cryptoHoldingsGbp,
        pct: cryptoPct,
        color: "#fbbf24", // Amber
        icon: Coins,
      },
    ];
  }, [totalEquity, freeCash, activeTrapsCapital, cryptoHoldingsGbp, balances?.SOL, balances?.XRP]);

  // SVG circular geometry
  const radius = 62;
  const strokeWidth = 14;
  const circumference = 2 * Math.PI * radius;

  // Compute SVG stroke-dasharray offsets
  let cumulativePct = 0;
  const svgArcs = segments.map((seg) => {
    const strokeDasharray = `${(seg.pct / 100) * circumference} ${circumference}`;
    const strokeDashoffset = -((cumulativePct / 100) * circumference);
    cumulativePct += seg.pct;
    return {
      ...seg,
      strokeDasharray,
      strokeDashoffset,
    };
  });

  return (
    <div className="nordic-glass rounded-2xl p-4 relative overflow-hidden shadow-2xl transition-all duration-300">
      {/* Header */}
      <div className="flex items-center justify-between mb-3 text-xs">
        <div className="flex items-center gap-1.5 text-slate-400 font-medium">
          <PieChart className="w-3.5 h-3.5 text-sky-400" />
          <span>CAPITAL ALLOCATION</span>
        </div>
        <span className="text-[10px] text-sky-400/80 bg-sky-950/60 px-2 py-0.5 rounded-full border border-sky-800/40">
          REVOLUT X LIVE
        </span>
      </div>

      {/* Circular Donut & Centered Metric */}
      <div className="flex items-center justify-center my-3 relative">
        <div className="relative w-44 h-44 flex items-center justify-center">
          <svg className="w-full h-full transform -rotate-90 select-none">
            {/* Background ring track */}
            <circle
              cx="88"
              cy="88"
              r={radius}
              stroke="rgba(30, 41, 59, 0.5)"
              strokeWidth={strokeWidth}
              fill="transparent"
            />
            {/* Colored segment arcs */}
            {svgArcs.map((arc) => (
              <circle
                key={arc.id}
                cx="88"
                cy="88"
                r={radius}
                stroke={arc.color}
                strokeWidth={strokeWidth}
                strokeDasharray={arc.strokeDasharray}
                strokeDashoffset={arc.strokeDashoffset}
                strokeLinecap="round"
                fill="transparent"
                className="transition-all duration-700 ease-out"
              />
            ))}
          </svg>

          {/* Centered Equity Figure */}
          <div className="absolute flex flex-col items-center justify-center text-center">
            <span className="text-[10px] font-medium text-slate-400 uppercase tracking-wider">
              Total Equity
            </span>
            <span className="text-xl font-bold text-white font-sans tracking-tight">
              {formatGbp(totalEquity, 2)}
            </span>
            <span className="text-[9px] text-teal-400 font-medium mt-0.5">
              100% Reinvesting
            </span>
          </div>
        </div>
      </div>

      {/* Asset Breakdown Legend */}
      <div className="space-y-2 mt-4 pt-3 border-t border-slate-800/60 text-xs">
        {segments.map((seg) => {
          const Icon = seg.icon;
          return (
            <div
              key={seg.id}
              className="flex items-center justify-between p-2 rounded-xl bg-slate-900/40 border border-slate-800/50"
            >
              <div className="flex items-center gap-2">
                <span
                  className="w-2.5 h-2.5 rounded-full"
                  style={{ backgroundColor: seg.color }}
                />
                <Icon className="w-3.5 h-3.5 text-slate-400" />
                <span className="text-slate-300 font-medium">{seg.label}</span>
              </div>
              <div className="text-right">
                <div className="font-bold text-slate-100 font-sans">
                  {formatGbp(seg.amount, 2)}
                </div>
                <div className="text-[10px] text-slate-400">
                  {seg.pct.toFixed(1)}%
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Cost Basis Auto-Audit Footer Badge */}
      <div className="mt-3.5 flex items-center justify-between px-3 py-2 rounded-xl bg-slate-900/80 border border-slate-800/80 text-[11px]">
        <div className="flex items-center gap-1.5 text-slate-400">
          <CheckCircle2 className="w-3.5 h-3.5 text-teal-400" />
          <span>Cost Basis:</span>
          <span className="text-slate-200 font-bold font-sans">
            {formatGbp(costBasis, 2)}
          </span>
        </div>
        <span className="text-[10px] text-teal-400/90 font-medium">
          Auto-Audited
        </span>
      </div>
    </div>
  );
};
