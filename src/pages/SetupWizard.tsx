import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { toUserError } from "../lib/errors";
import { poolInit } from "../lib/ipc";
import type { Pool, PoolInitPayload } from "../lib/types";

interface SetupWizardProps {
  onCreated: (pool: Pool) => void;
}

export default function SetupWizard({ onCreated }: SetupWizardProps) {
  const navigate = useNavigate();
  const [submittingTarget, setSubmittingTarget] = useState<"/" | "/deploy" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ticker, setTicker] = useState("OURO");
  const [network, setNetwork] = useState<PoolInitPayload["network"]>("preprod");
  const [margin, setMargin] = useState("0.02");
  const [fixedCost, setFixedCost] = useState("340000000");

  const buildPayload = (): PoolInitPayload => {
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
    return payload;
  };

  const initializeWorkspace = async (target: "/" | "/deploy") => {
    setSubmittingTarget(target);
    setError(null);
    try {
      const pool = await poolInit(buildPayload());
      onCreated(pool);
      navigate(target, { replace: true });
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setSubmittingTarget(null);
    }
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await initializeWorkspace("/");
  };

  const handleStartDeploy = async () => {
    await initializeWorkspace("/deploy");
  };

  const isSubmitting = submittingTarget != null;

  return (
    <div className="min-h-screen bg-slate-100 p-6 text-slate-900">
      <div className="mx-auto max-w-2xl space-y-4 rounded-xl border border-slate-200 bg-white p-6 shadow-sm">
        <section className="rounded-lg border border-slate-200 bg-slate-50 p-4">
          <h1 className="text-2xl font-semibold tracking-tight">Welcome</h1>
          <p className="mt-2 text-sm text-slate-700">
            No pool is configured for this workspace yet. Create the workspace pool and continue with the
            pool-first operation flow.
          </p>
          <p className="mt-2 text-xs text-slate-500">
            `Start Deploy` will initialize the pool record first, then jump to Deploy Step 1.
          </p>
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void handleStartDeploy()}
              disabled={isSubmitting}
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-70"
            >
              {submittingTarget === "/deploy" ? "Preparing Deploy..." : "Start Deploy"}
            </button>
            <button
              type="button"
              disabled
              className="rounded-md border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-500 disabled:cursor-not-allowed"
              title="Import flow will be enabled in a later stage."
            >
              Import Existing Config
            </button>
          </div>
        </section>

        <form className="space-y-4 rounded-lg border border-slate-200 bg-slate-50 p-4" onSubmit={handleSubmit}>
          <h2 className="text-sm font-medium text-slate-900">Workspace Pool Setup</h2>
          <label className="block text-sm">
            <span className="mb-1 block text-slate-700">Ticker (3-5 chars)</span>
            <input
              className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900 outline-none ring-blue-500 focus:ring"
              value={ticker}
              onChange={(e) => setTicker(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              maxLength={5}
              required
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-slate-700">Network</span>
            <select
              className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900 outline-none ring-blue-500 focus:ring"
              value={network}
              onChange={(e) => setNetwork(e.target.value as PoolInitPayload["network"])}
            >
              <option value="mainnet">mainnet</option>
              <option value="preprod">preprod</option>
              <option value="preview">preview</option>
            </select>
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-slate-700">Margin (optional)</span>
            <input
              className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900 outline-none ring-blue-500 focus:ring"
              value={margin}
              onChange={(e) => setMargin(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              inputMode="decimal"
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1 block text-slate-700">Fixed Cost (optional)</span>
            <input
              className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900 outline-none ring-blue-500 focus:ring"
              value={fixedCost}
              onChange={(e) => setFixedCost(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              inputMode="numeric"
            />
          </label>
          {error && (
            <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
              {error}
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <button
              type="submit"
              disabled={isSubmitting}
              className="rounded-md border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-900 hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-70"
            >
              {submittingTarget === "/" ? "Creating..." : "Create Workspace"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
