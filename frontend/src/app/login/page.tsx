"use client";

import React, { useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { Lock, ArrowRight } from "lucide-react";

export default function LoginPage() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const returnUrl = searchParams.get("returnUrl") || "/dashboard";

  const [pin, setPin] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!pin || submitting) return;

    setSubmitting(true);
    setError("");

    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ pin }),
      });

      const data = await res.json();
      if (!res.ok) {
        setError(data.error || "Authentication failed.");
        setPin("");
      } else {
        router.push(returnUrl);
        router.refresh();
      }
    } catch {
      setError("Unable to reach authentication service.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <main className="min-h-screen bg-[#0d1117] flex items-center justify-center p-4 text-[#e6edf3]">
      <div className="w-full max-w-xs bg-[#161b22] border border-[#30363d] rounded-xl p-6 space-y-5 shadow-lg">
        {/* Header */}
        <div className="space-y-1 text-center">
          <div className="inline-flex items-center justify-center w-9 h-9 rounded-lg bg-[#21262d] border border-[#30363d] text-[#8b949e] mb-2">
            <Lock className="w-4 h-4 text-[#7d8590]" />
          </div>
          <h1 className="text-sm font-semibold tracking-tight text-[#f0f6fc]">
            Serene Lavoisier
          </h1>
          <p className="text-xs text-[#8b949e]">
            Trading Terminal Access
          </p>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <input
              type="password"
              inputMode="numeric"
              autoFocus
              value={pin}
              onChange={(e) => {
                setPin(e.target.value);
                if (error) setError("");
              }}
              placeholder="Enter PIN"
              className="w-full text-center bg-[#0d1117] border border-[#30363d] focus:border-[#58a6ff] focus:ring-1 focus:ring-[#58a6ff] rounded-lg py-2.5 px-3 text-sm font-mono tracking-widest text-[#f0f6fc] placeholder:text-[#484f58] outline-none transition"
            />
          </div>

          {error && (
            <p className="text-xs text-[#f85149] text-center font-mono">
              {error}
            </p>
          )}

          <button
            type="submit"
            disabled={submitting || pin.length === 0}
            className="w-full bg-[#238636] hover:bg-[#2ea043] disabled:bg-[#21262d] text-white disabled:text-[#484f58] font-medium py-2 rounded-lg text-xs flex items-center justify-center gap-1.5 transition disabled:cursor-not-allowed"
          >
            <span>{submitting ? "Verifying..." : "Authenticate"}</span>
            <ArrowRight className="w-3.5 h-3.5" />
          </button>
        </form>
      </div>
    </main>
  );
}
