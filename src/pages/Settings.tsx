import { Link } from "react-router-dom";
import type { Pool } from "../lib/types";

interface SettingsProps {
  pool: Pool;
}

export default function Settings({ pool }: SettingsProps) {
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
                <dt className="text-slate-500">Ticker</dt>
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
            <h2 className="text-sm font-medium text-slate-900">说明</h2>
            <ul className="mt-3 space-y-2 text-sm text-slate-600">
              <li>ticker, margin 和 fixed cost 来自链上注册，不可在此编辑。</li>
              <li>节点运维操作在 Deploy、KES、Upgrade 流程中进行。</li>
            </ul>
          </div>
        </div>

        <div className={`mt-4 flex items-center justify-between rounded-md border px-3 py-3 text-sm ${isBound ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-amber-200 bg-amber-50 text-amber-700"}`}>
          <span>
            {isBound
              ? "已绑定链上矿池。Dashboard 展示最新的链上数据与质押信息。"
              : "尚未绑定链上矿池。"}
          </span>
          <Link
            to="/bind-pool"
            className="shrink-0 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 shadow-sm hover:bg-slate-50"
          >
            {isBound ? "更改绑定" : "去绑定"}
          </Link>
        </div>
      </section>
    </section>
  );
}
