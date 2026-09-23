"use client";

import React from "react";
import { TelemetryPayload } from "../../../types/telemetry";
import { formatGbp, toNum } from "../../../lib/format";
import { Layers, ArrowDownRight, ArrowUpRight, Activity } from "lucide-react";

interface OrderLadderCardProps {
  telemetry: TelemetryPayload | null;
}

export const OrderLadderCard: React.FC<OrderLadderCardProps> = ({ telemetry }) => {
  const runner = telemetry?.runners?.[0];
  const activeOrders = telemetry?.active_orders || telemetry?.resting_orders || [];
  const trades = telemetry?.trades || telemetry?.live_trades || [];

  const midPrice = toNum(runner?.effective_center ?? runner?.center_price, 0);
  const assetSymbol = runner?.symbol?.split("/")[0] || "XRP";
  const isSubTen = midPrice < 10 && midPrice > 0;
  const priceDecimals = isSubTen ? 4 : 2;
  const qtyDecimals = assetSymbol === "XRP" ? 2 : 4;

  // Split orders into SELLs (above mid) and BUYs (below mid)
  const sellOrders = activeOrders
    .filter((o) => o.side === "SELL")
    .sort((a, b) => toNum(b.price, 0) - toNum(a.price, 0));

  const buyOrders = activeOrders
    .filter((o) => o.side === "BUY")
    .sort((a, b) => toNum(b.price, 0) - toNum(a.price, 0));

  // Recent 3 filled trades
  const recentFills = trades.slice(0, 3);

  return (
    <div className="nordic-glass rounded-2xl p-4 relative overflow-hidden shadow-2xl transition-all duration-300">
      {/* Header */}
      <div className="flex items-center justify-between mb-3 text-xs">
        <div className="flex items-center gap-1.5 text-slate-400 font-medium">
          <Layers className="w-3.5 h-3.5 text-sky-400" />
          <span>ORDER DEPTH & LIVE FILLS</span>
        </div>
        <span className="text-[10px] text-sky-400 bg-slate-900 px-2 py-0.5 rounded-full border border-sky-900/40 font-mono">
          {activeOrders.length} RESTING
        </span>
      </div>

      {/* Visual Price Depth Ladder */}
      <div className="space-y-1.5 mt-2 text-xs">
        {/* Resting Sell Rungs */}
        {sellOrders.length > 0 ? (
          sellOrders.map((ord) => (
            <div
              key={ord.id}
              className="flex items-center justify-between p-2 rounded-xl bg-rose-950/20 border border-rose-900/30"
            >
              <div className="flex items-center gap-1.5 text-rose-400 font-medium">
                <ArrowUpRight className="w-3.5 h-3.5" />
                <span>SELL @ {formatGbp(toNum(ord.price, 0), toNum(ord.price, 0) < 10 ? 4 : 2)}</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-slate-400 text-[10px] font-mono">
                  {toNum(ord.qty, 0).toFixed(qtyDecimals)} {assetSymbol}
                </span>
                <span className="font-bold text-rose-300 font-sans">
                  {formatGbp(toNum(ord.value_gbp, 0), 2)}
                </span>
              </div>
            </div>
          ))
        ) : (
          <div className="text-[11px] text-slate-500 italic p-1">
            No resting sell orders (Inventory 100% liquidated)
          </div>
        )}

        {/* Center Mid Price Divider */}
        <div className="py-2 flex items-center justify-between px-2.5 my-1 bg-sky-950/40 border border-sky-500/30 rounded-xl">
          <div className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-cyan-400 animate-ping" />
            <span className="text-[11px] font-bold text-sky-300">
              ORACLE MID
            </span>
          </div>
          <span className="text-base font-bold font-sans text-white">
            {formatGbp(midPrice, priceDecimals)}
          </span>
        </div>

        {/* Resting Buy Rungs */}
        {buyOrders.slice(0, 4).map((ord) => (
          <div
            key={ord.id}
            className="flex items-center justify-between p-2 rounded-xl bg-sky-950/20 border border-sky-900/30"
          >
            <div className="flex items-center gap-1.5 text-sky-400 font-medium">
              <ArrowDownRight className="w-3.5 h-3.5" />
              <span>BUY @ {formatGbp(toNum(ord.price, 0), toNum(ord.price, 0) < 10 ? 4 : 2)}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-slate-400 text-[10px] font-mono">
                {toNum(ord.qty, 0).toFixed(qtyDecimals)} {assetSymbol}
              </span>
              <span className="font-bold text-sky-300 font-sans">
                {formatGbp(toNum(ord.value_gbp, 0), 2)}
              </span>
            </div>
          </div>
        ))}
      </div>

      {/* Recent Fills Mini-Stream */}
      <div className="mt-4 pt-3 border-t border-slate-800/60">
        <div className="flex items-center justify-between text-[11px] text-slate-400 mb-2">
          <span className="flex items-center gap-1">
            <Activity className="w-3 h-3 text-teal-400" />
            RECENT EXECUTIONS
          </span>
          <span className="text-[9px] text-teal-400 font-medium">
            0.00% FEE REBATE
          </span>
        </div>

        <div className="space-y-1.5">
          {recentFills.length > 0 ? (
            recentFills.map((trade) => {
              const isSell = trade.side === "SELL";
              const profit = toNum(trade.profit ?? trade.pnl_gbp, 0);
              const tPrice = toNum(trade.price, 0);
              return (
                <div
                  key={trade.id}
                  className="flex items-center justify-between p-2 rounded-xl bg-slate-900/60 border border-slate-800/60 text-xs"
                >
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-[10px] px-1.5 py-0.5 rounded font-bold ${
                        isSell
                          ? "bg-rose-950 text-rose-300 border border-rose-800/50"
                          : "bg-sky-950 text-sky-300 border border-sky-800/50"
                      }`}
                    >
                      {trade.side}
                    </span>
                    <span className="text-slate-300 font-medium">
                      {formatGbp(tPrice, tPrice < 10 ? 4 : 2)}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    {profit > 0 && (
                      <span className="text-[10px] font-bold text-teal-400 bg-teal-950/60 px-1.5 py-0.5 rounded border border-teal-800/40">
                        +{(profit * 100).toFixed(2)}p
                      </span>
                    )}
                    <span className="text-slate-400 text-[10px] font-sans">
                      {formatGbp(toNum(trade.value_gbp, 0), 2)}
                    </span>
                  </div>
                </div>
              );
            })
          ) : (
            <div className="text-[11px] text-slate-500 italic p-1">
              Awaiting next grid oscillation...
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
