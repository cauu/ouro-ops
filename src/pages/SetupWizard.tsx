import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { poolInit } from "../lib/ipc";
import type { Pool, PoolInitPayload } from "../lib/types";

interface SetupWizardProps {
  onCreated: (pool: Pool) => void;
}

export default function SetupWizard({ onCreated }: SetupWizardProps) {
  const navigate = useNavigate();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ticker, setTicker] = useState("OURO");
  const [network, setNetwork] = useState<PoolInitPayload["network"]>("preprod");
  const [margin, setMargin] = useState("0.02");
  const [fixedCost, setFixedCost] = useState("340000000");

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const payload: PoolInitPayload = {
        ticker: ticker.trim().toUpperCase(),
        network,
      };
      if (margin.trim() !== "") {
        payload.margin = Number(margin);
      }
      if (fixedCost.trim() !== "") {
        payload.fixed_cost = Number(fixedCost);
      }
      const pool = await poolInit(payload);
      onCreated(pool);
      navigate("/", { replace: true });
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="min-h-screen bg-zinc-950 p-6 text-zinc-100">
      <div className="mx-auto max-w-lg rounded-xl border border-zinc-800 bg-zinc-900 p-6">
        <h1 className="text-2xl font-semibold">Setup Wizard</h1>
        <p className="mt-2 text-sm text-zinc-400">Initialize the single pool for this workspace.</p>
        <form className="mt-6 space-y-4" onSubmit={handleSubmit}>
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Ticker (3-5 chars)</span>
            <input
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100 outline-none ring-blue-500 focus:ring"
              value={ticker}
              onChange={(e) => setTicker(e.target.value)}
              maxLength={5}
              required
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Network</span>
            <select
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100 outline-none ring-blue-500 focus:ring"
              value={network}
              onChange={(e) => setNetwork(e.target.value as PoolInitPayload["network"])}
            >
              <option value="mainnet">mainnet</option>
              <option value="preprod">preprod</option>
              <option value="preview">preview</option>
            </select>
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Margin (optional)</span>
            <input
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100 outline-none ring-blue-500 focus:ring"
              value={margin}
              onChange={(e) => setMargin(e.target.value)}
              inputMode="decimal"
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Fixed Cost (optional)</span>
            <input
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100 outline-none ring-blue-500 focus:ring"
              value={fixedCost}
              onChange={(e) => setFixedCost(e.target.value)}
              inputMode="numeric"
            />
          </label>
          {error && (
            <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
              {error}
            </p>
          )}
          <button
            type="submit"
            disabled={submitting}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-70"
          >
            {submitting ? "Initializing..." : "Initialize Pool"}
          </button>
        </form>
      </div>
    </div>
  );
}
