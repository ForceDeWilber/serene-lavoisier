import React, { useEffect, useRef } from "react";
import { TelemetryPayload } from "../../types/telemetry";

interface Props {
  telemetry: TelemetryPayload | null;
  statusMessage: string;
}

export function TerminalLogPane({ telemetry, statusMessage }: Props) {
  const engineActivity = telemetry?.engine_activity;
  const decisions = telemetry?.engine_decisions || [];
  
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new logs arrive
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [decisions, statusMessage]);

  return (
    <div className="flex flex-col bg-[#050505] border border-gray-800 font-mono text-xs overflow-hidden h-full">
      <div className="bg-gray-900 border-b border-gray-800 px-2 py-1 flex items-center justify-between">
        <span className="text-gray-300 font-semibold uppercase">TERMINAL_LOG</span>
        <span className="text-gray-500 text-[10px]">
          {engineActivity?.oracle_latency_ms ?? '--'}ms LATENCY
        </span>
      </div>

      <div ref={scrollRef} className="flex-1 overflow-y-auto p-2 space-y-1 bg-black text-[10px]">
        {/* Connection status message if present */}
        {statusMessage && (
          <div className="text-yellow-400">
            <span className="text-gray-600">[{new Date().toLocaleTimeString()}]</span> SYS: {statusMessage}
          </div>
        )}

        {/* Engine state dump */}
        <div className="text-blue-400/70 py-1">
          -------------------------------------------------<br/>
          RUST EXECUTION CORE ACTIVE<br/>
          TRADING_MODE: {String(telemetry?.mode || 'LIVE').toUpperCase()}<br/>
          CIRCUIT_BREAKER: {telemetry?.circuit_breaker_tripped ? 'TRIPPED' : 'NORMAL'}<br/>
          ACTIVE_RUNNERS: {telemetry?.runners?.length || 0}<br/>
          -------------------------------------------------
        </div>

        {/* Decisions / Logs */}
        {decisions.length === 0 ? (
          <div className="text-gray-600 italic">AWAITING_EVENTS...</div>
        ) : (
          decisions.map(d => {
            let colorClass = 'text-gray-400';
            if (d.badge === 'FILLED' || d.category === 'EXECUTION') colorClass = 'text-emerald-400';
            if (d.badge === 'REJECTED' || d.badge === 'ERROR') colorClass = 'text-red-400';
            if (d.badge === 'CANCELED' || d.badge === 'HALTED') colorClass = 'text-yellow-400';

            return (
              <div key={d.id} className="leading-tight mb-1">
                <span className="text-gray-600">[{d.timestamp}]</span>{' '}
                <span className={colorClass}>[{d.badge}]</span>{' '}
                <span className="text-gray-300">{d.title}</span>{' '}
                {d.detail && <span className="text-gray-500">- {d.detail}</span>}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
