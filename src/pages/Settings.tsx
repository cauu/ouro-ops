import type { Pool } from "../lib/types";
import PoolRegistrationStatus from "./PoolRegistrationStatus";

interface SettingsProps {
  pool: Pool;
  onPoolUpdated: (pool: Pool) => void;
}

export default function Settings({ pool, onPoolUpdated }: SettingsProps) {
  const isBound = pool.onchain_registered && Boolean(pool.onchain_pool_id);

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-sm font-semibold">Settings</h1>
        <p className="mt-1 text-xs text-slate-500">
          Created at: {pool.created_at} · Updated at: {pool.updated_at}
        </p>
      </header>

      <section className="max-w-3xl rounded-xl border border-slate-200 bg-slate-50 p-4 shadow-sm">
        <div className="grid gap-4 md:grid-cols-2">
          <div className="rounded-md border border-slate-200 bg-white p-3">
            <h2 className="text-sm font-medium text-slate-900">Workspace Pool Record</h2>
            <dl className="mt-3 space-y-2 text-sm text-slate-700">
              <div className="flex items-center justify-between gap-4">
                <dt className="text-slate-500">Network</dt>
                <dd>{pool.network}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-slate-500">Local ticker cache</dt>
                <dd>{pool.ticker || "—"}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-slate-500">Bound on-chain pool</dt>
                <dd className="break-all font-mono text-xs">{pool.onchain_pool_id ?? "Not bound"}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-slate-500">On-chain sync</dt>
                <dd>{pool.onchain_synced_at ?? "Never"}</dd>
              </div>
            </dl>
          </div>

          <div className="rounded-md border border-slate-200 bg-white p-3">
            <h2 className="text-sm font-medium text-slate-900">Configuration Source of Truth</h2>
            <ul className="mt-3 space-y-2 text-sm text-slate-600">
              <li>ticker, margin 和 fixed cost 在此页面不可编辑。</li>
              <li>链上参数来自绑定的 on-chain registration。</li>
              <li>节点运维操作在 Deploy、KES、Upgrade 流程中进行。</li>
            </ul>
          </div>
        </div>

        <div className={`mt-4 rounded-md border px-3 py-3 text-sm ${isBound ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-amber-200 bg-amber-50 text-amber-700"}`}>
          {isBound
            ? "此工作区已绑定链上矿池。Dashboard 展示最新的链上 ticker、margin、fixed cost 和 metadata。"
            : "此工作区尚未绑定链上矿池。请在下方查询并绑定。"}
        </div>
      </section>

      <section className="max-w-3xl">
        <h2 className="mb-3 text-sm font-semibold text-slate-900">On-chain Pool Binding</h2>
        <PoolRegistrationStatus
          poolTicker={pool.ticker}
          onBound={onPoolUpdated}
          embedded
        />
      </section>
    </section>
  );
}
