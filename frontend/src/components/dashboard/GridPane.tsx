import React, { useState, useEffect } from "react";
import { TelemetryPayload } from "../../types/telemetry";

interface Props {
  telemetry: TelemetryPayload | null;
  onTuneRunner: (runnerId: string, stepPct: string, rebalancePct: string) => Promise<void>;
}

export function GridPane({ telemetry, onTuneRunner }: Props) {
  const runners = telemetry?.runners || [];
  
  const [localParams, setLocalParams] = useState<Record<string, { step_pct: string; rebalance_pct: string }>>({});

  useEffect(() => {
    if (runners.length > 0) {
      const updated = { ...localParams };
      runners.forEach((r) => {
        if (r.step_pct && !localParams[r.runner_id]) {
          updated[r.runner_id] = {
            step_pct: (r.step_pct * 100).toFixed(2),
            rebalance_pct: (r.rebalance_threshold_pct ? r.rebalance_threshold_pct * 100 : 2.0).toFixed(1),
          };
        }
      });
      setLocalParams(updated);
    }
  }, [telemetry]);

  const handleTune = (runnerId: string) => {
    const p = localParams[runnerId];
    if (p) onTuneRunner(runnerId, p.step_pct, p.rebalance_pct);
  };

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <span className="text-gray-300 font-semibold uppercase">GRID_STRATEGY</span>
      </div>

      <div className="flex-1 overflow-y-auto p-2">
        <div className="space-y-2">
          {runners.length === 0 ? (
            <div className="text-gray-600 italic">NO_GRID_RUNNERS_ACTIVE</div>
          ) : (
            runners.map(runner => {
              const p = localParams[runner.runner_id] || { step_pct: '0.40', rebalance_pct: '2.0' };
              return (
                <div key={runner.runner_id} className="border border-gray-800 p-2 flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <span className="font-bold text-blue-400">{runner.symbol}</span>
                    <span className={runner.realized_pnl >= 0 ? 'text-emerald-400' : 'text-red-400'}>
                      £{runner.realized_pnl.toFixed(2)}
                    </span>
                  </div>
                  
                  <div className="grid grid-cols-2 gap-2 text-[10px]">
                    <div>CENTER: <span className="text-gray-200">£{runner.center_price?.toFixed(2) || '---'}</span></div>
                    <div>INV: <span className="text-gray-200">{runner.inventory_base.toFixed(4)}</span></div>
                    <div>ORDERS: <span className="text-gray-200">{runner.active_orders_count}</span></div>
                    <div>TRADES: <span className="text-gray-200">{runner.total_trades}</span></div>
                  </div>

                  <div className="flex items-center gap-1 mt-1">
                    <input 
                      value={p.step_pct}
                      onChange={e => setLocalParams({...localParams, [runner.runner_id]: { ...p, step_pct: e.target.value }})}
                      className="w-12 bg-black border border-gray-700 px-1 py-0.5 text-center text-[10px] text-gray-200" 
                      placeholder="STEP%" 
                    />
                    <span className="text-gray-600">%</span>
                    <button 
                      onClick={() => handleTune(runner.runner_id)}
                      className="ml-auto px-2 py-0.5 bg-gray-900 border border-gray-700 text-gray-300 hover:bg-gray-800"
                    >
                      [APPLY]
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
