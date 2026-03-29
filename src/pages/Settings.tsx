import { Link } from "react-router-dom";
import type { Pool } from "../lib/types";

interface SettingsProps {
  pool: Pool;
}

interface SettingRowProps {
  label: string;
  action: string;
  to: string;
  description?: string;
}

function SettingRow({ label, action, to, description }: SettingRowProps) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-slate-100 px-4 py-3 last:border-0">
      <div className="min-w-0">
        <p className="text-sm text-slate-900">{label}</p>
        {description && <p className="mt-0.5 truncate text-xs text-slate-500">{description}</p>}
      </div>
      <Link
        to={to}
        className="shrink-0 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 shadow-sm hover:bg-slate-50"
      >
        {action}
      </Link>
    </div>
  );
}

export default function Settings({ pool }: SettingsProps) {
  const isBound = pool.onchain_registered && Boolean(pool.onchain_pool_id);

  return (
    <section className="mx-auto max-w-xl space-y-6">
      <section>
        <h2 className="mb-2 px-4 text-xs font-semibold uppercase tracking-wide text-slate-400">矿池</h2>
        <div className="rounded-xl border border-slate-200 bg-white shadow-sm">
          <SettingRow
            label="链上绑定"
            description={isBound ? pool.onchain_pool_id ?? undefined : "尚未绑定"}
            action={isBound ? "更改" : "绑定"}
            to="/bind-pool"
          />
        </div>
      </section>

      <section>
        <h2 className="mb-2 px-4 text-xs font-semibold uppercase tracking-wide text-slate-400">关于</h2>
        <div className="rounded-xl border border-slate-200 bg-white shadow-sm">
          <div className="flex items-center justify-between gap-4 border-b border-slate-100 px-4 py-3">
            <p className="text-sm text-slate-900">网络</p>
            <p className="text-sm text-slate-500">{pool.network}</p>
          </div>
          <div className="flex items-center justify-between gap-4 px-4 py-3">
            <p className="text-sm text-slate-900">版本</p>
            <p className="text-sm text-slate-500">0.1.0</p>
          </div>
        </div>
      </section>
    </section>
  );
}
