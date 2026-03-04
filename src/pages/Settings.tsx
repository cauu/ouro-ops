import { useEffect, useState, type FormEvent } from "react";
import { poolUpdate } from "../lib/ipc";
import type { Pool, PoolUpdatePayload } from "../lib/types";

interface SettingsProps {
  pool: Pool;
  onUpdated: (pool: Pool) => void;
}

export default function Settings({ pool, onUpdated }: SettingsProps) {
  const [ticker, setTicker] = useState(pool.ticker);
  const [margin, setMargin] = useState(pool.margin?.toString() ?? "");
  const [fixedCost, setFixedCost] = useState(pool.fixed_cost?.toString() ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setTicker(pool.ticker);
    setMargin(pool.margin?.toString() ?? "");
    setFixedCost(pool.fixed_cost?.toString() ?? "");
  }, [pool]);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const payload: PoolUpdatePayload = {};
      if (ticker.trim() !== "") {
        payload.ticker = ticker.trim().toUpperCase();
      }
      if (margin.trim() !== "") {
        payload.margin = Number(margin);
      }
      if (fixedCost.trim() !== "") {
        payload.fixed_cost = Number(fixedCost);
      }
      const updated = await poolUpdate(payload);
      onUpdated(updated);
      setMessage("Pool settings updated.");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="mt-1 text-sm text-zinc-400">
          Created at: {pool.created_at} · Updated at: {pool.updated_at}
        </p>
      </header>

      <form onSubmit={handleSubmit} className="max-w-xl rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
        <div className="space-y-4">
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Ticker</span>
            <input
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              value={ticker}
              onChange={(e) => setTicker(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              maxLength={5}
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Margin</span>
            <input
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              value={margin}
              onChange={(e) => setMargin(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              inputMode="decimal"
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-zinc-300">Fixed Cost</span>
            <input
              className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              value={fixedCost}
              onChange={(e) => setFixedCost(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              inputMode="numeric"
            />
          </label>
        </div>

        {error && (
          <p className="mt-4 rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
            {error}
          </p>
        )}
        {message && (
          <p className="mt-4 rounded-md border border-emerald-700/60 bg-emerald-900/20 px-3 py-2 text-sm text-emerald-300">
            {message}
          </p>
        )}

        <button
          type="submit"
          disabled={saving}
          className="mt-4 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-70"
        >
          {saving ? "Saving..." : "Save"}
        </button>
      </form>
    </section>
  );
}
