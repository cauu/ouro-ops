import type { Pool } from "../lib/types";

interface SettingsProps {
  pool: Pool;
}

export default function Settings({ pool }: SettingsProps) {
  const isBound = pool.onchain_registered && Boolean(pool.onchain_pool_id);

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="mt-1 text-sm text-zinc-400">
          Created at: {pool.created_at} · Updated at: {pool.updated_at}
        </p>
      </header>

      <section className="max-w-3xl rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
        <div className="grid gap-4 md:grid-cols-2">
          <div className="rounded-md border border-zinc-800 bg-zinc-950/60 p-3">
            <h2 className="text-sm font-medium text-zinc-200">Workspace Pool Record</h2>
            <dl className="mt-3 space-y-2 text-sm text-zinc-300">
              <div className="flex items-center justify-between gap-4">
                <dt className="text-zinc-500">Network</dt>
                <dd>{pool.network}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-zinc-500">Local ticker cache</dt>
                <dd>{pool.ticker || "—"}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-zinc-500">Bound on-chain pool</dt>
                <dd>{pool.onchain_pool_id ?? "Not bound"}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-zinc-500">On-chain sync</dt>
                <dd>{pool.onchain_synced_at ?? "Never"}</dd>
              </div>
            </dl>
          </div>

          <div className="rounded-md border border-zinc-800 bg-zinc-950/60 p-3">
            <h2 className="text-sm font-medium text-zinc-200">Configuration Source of Truth</h2>
            <ul className="mt-3 space-y-2 text-sm text-zinc-300">
              <li>`ticker`, `margin` and `fixed cost` are not edited here.</li>
              <li>Chain-facing pool parameters are read from the bound on-chain registration.</li>
              <li>Use Dashboard to bind an existing `pool_id` or continue with the registration flow.</li>
              <li>Node runtime operations stay within Deploy, KES and Upgrade flows.</li>
            </ul>
          </div>
        </div>

        <div className="mt-4 rounded-md border border-amber-700/40 bg-amber-950/20 px-3 py-3 text-sm text-amber-200">
          {isBound
            ? "This workspace is bound to an on-chain pool. Dashboard shows the latest on-chain ticker, margin, fixed cost and metadata."
            : "This workspace is not yet bound to an on-chain pool. Bind an existing pool from Dashboard, or continue with pool registration there."}
        </div>
      </section>
    </section>
  );
}
