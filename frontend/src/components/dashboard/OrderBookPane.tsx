import React from "react";
import { TelemetryPayload } from "../../types/telemetry";
import { formatNum, formatPct, toNum } from "../../lib/format";

interface Props {
  telemetry: TelemetryPayload | null;
}

export function OrderBookPane({ telemetry }: Props) {
  const orders = telemetry?.resting_orders || [];

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-[10px] overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <span className="text-gray-300 font-semibold uppercase text-xs">RESTING_ORDERS</span>
        <span className="text-gray-500">{orders.length} RINGS</span>
      </div>

      <div className="flex-1 overflow-auto">
        <table className="w-full text-right whitespace-nowrap">
          <thead className="bg-[#0a0a0a] sticky top-0 border-b border-gray-800">
            <tr className="text-gray-500">
              <th className="text-left font-normal p-1 pl-2">SYM</th>
              <th className="font-normal p-1">SIDE</th>
              <th className="font-normal p-1">PRICE(£)</th>
              <th className="font-normal p-1">QTY</th>
              <th className="font-normal p-1 pr-2">DIST(%)</th>
            </tr>
          </thead>
          <tbody className="text-gray-300">
            {orders.length === 0 ? (
              <tr><td colSpan={5} className="text-center p-4 text-gray-600 italic">BOOK_EMPTY</td></tr>
            ) : (
              // Sort by price descending
              [...orders]
                .sort((a, b) => toNum(b.price) - toNum(a.price))
                .map((o) => {
                  const sideStr = String(o.side).toUpperCase();
                  return (
                    <tr key={o.id || o.client_order_id} className="border-b border-gray-800/30 hover:bg-gray-900/50">
                      <td className="text-left p-1 pl-2 font-bold text-gray-400">{String(o.symbol).replace('/GBP', '')}</td>
                      <td className={`p-1 ${sideStr === 'BUY' ? 'text-emerald-500' : 'text-red-500'}`}>{sideStr}</td>
                      <td className="p-1">{formatNum(o.price, 2)}</td>
                      <td className="p-1">{formatNum(o.qty, 4)}</td>
                      <td className="p-1 pr-2 text-gray-500">
                        {formatPct(o.distance_pct, 2, true)}
                      </td>
                    </tr>
                  );
                })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
