import React, { useState } from "react";
import { Plus, Trash2, Zap } from "lucide-react";
import { TelemetryPayload } from "../../types/telemetry";
import { formatNum, formatGbp, toNum } from "../../lib/format";

interface Props {
  telemetry: TelemetryPayload | null;
  onAddPair: (config: any) => Promise<void>;
  onRemovePair: (runnerId: string) => Promise<void>;
  onLiquidatePair: (runnerId: string) => Promise<void>;
}

export function AssetManagementPane({ telemetry, onAddPair, onRemovePair, onLiquidatePair }: Props) {
  const [baseAsset, setBaseAsset] = useState("BTC");
  const [quoteAsset, setQuoteAsset] = useState("GBP");
  
  const activeRunners = telemetry?.runners || [];

  const handleSpawn = () => {
    onAddPair({
      base: baseAsset,
      quote: quoteAsset,
      envelope_capital: "500",
      grid_step_pct: "0.40",
      order_size_fiat: "50",
      sniper_enabled: true,
    });
  };

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <span className="text-gray-300 font-semibold uppercase">ASSET_MANAGEMENT</span>
        <span className="text-gray-500">{activeRunners.length} ACTIVE</span>
      </div>

      <div className="p-2 space-y-3 flex-1 overflow-y-auto">
        {/* Spawn New Pair */}
        <div className="border border-gray-800 p-2 space-y-2">
          <div className="text-gray-500 uppercase text-[10px]">SPAWN_NEW_PAIR</div>
          <div className="flex gap-2">
            <input 
              value={baseAsset} 
              onChange={e => setBaseAsset(e.target.value.toUpperCase())}
              className="bg-black border border-gray-700 px-2 py-1 w-16 text-center focus:border-blue-500 outline-none uppercase text-gray-200" 
              placeholder="BASE" 
            />
            <span className="text-gray-600 flex items-center">/</span>
            <input 
              value={quoteAsset} 
              onChange={e => setQuoteAsset(e.target.value.toUpperCase())}
              className="bg-black border border-gray-700 px-2 py-1 w-16 text-center focus:border-blue-500 outline-none uppercase text-gray-200" 
              placeholder="QUOTE" 
            />
            <button 
              onClick={handleSpawn}
              className="flex-1 bg-blue-900/30 text-blue-400 border border-blue-900/50 hover:bg-blue-900 hover:text-white transition-colors flex items-center justify-center gap-1"
            >
              <Plus className="w-3 h-3" />
              <span>SPAWN</span>
            </button>
          </div>
        </div>

        {/* Active Pairs List */}
        <div className="space-y-1">
          <div className="text-gray-500 uppercase text-[10px] mb-1">ACTIVE_RUNNERS</div>
          {activeRunners.length === 0 ? (
            <div className="text-gray-600 italic px-2 py-1">NO_PAIRS_ACTIVE</div>
          ) : (
            activeRunners.map(runner => {
              const pnlVal = toNum(runner.realized_pnl, 0);
              return (
                <div key={runner.runner_id} className="border border-gray-800 p-2 flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <span className="text-gray-200 font-bold">{runner.symbol}</span>
                    <span className={`px-1 text-[10px] ${runner.is_paused ? 'bg-yellow-900 text-yellow-400' : 'bg-emerald-900/50 text-emerald-400'}`}>
                      {runner.is_paused ? 'PAUSED' : 'ACTIVE'}
                    </span>
                  </div>
                  <div className="grid grid-cols-2 gap-1 text-[10px] text-gray-400">
                    <div>INV: <span className="text-gray-200">{formatNum(runner.inventory_base, 4)}</span></div>
                    <div>PNL: <span className={pnlVal >= 0 ? 'text-emerald-400' : 'text-red-400'}>{formatGbp(runner.realized_pnl, 2, true)}</span></div>
                  </div>
                  <div className="flex gap-1 mt-1">
                    <button 
                      onClick={() => onLiquidatePair(runner.runner_id)}
                      className="flex-1 bg-red-950/30 text-red-400 border border-red-900/50 hover:bg-red-900 hover:text-white flex items-center justify-center py-1 gap-1"
                      title="Sell all inventory to quote currency instantly"
                    >
                      <Zap className="w-3 h-3" />
                      <span>LIQUIDATE</span>
                    </button>
                    <button 
                      onClick={() => onRemovePair(runner.runner_id)}
                      className="px-2 bg-gray-900 text-gray-400 border border-gray-800 hover:bg-gray-800 hover:text-white flex items-center justify-center"
                      title="Remove pair from engine"
                    >
                      <Trash2 className="w-3 h-3" />
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
