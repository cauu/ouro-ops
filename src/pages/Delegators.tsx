import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useDelegatorListQuery } from "../lib/queries";

interface DelegatorsProps {
  onchainPoolId?: string | null;
}

const PAGE_SIZE = 20;

function truncateAddress(addr: string): string {
  if (addr.length <= 20) return addr;
  return `${addr.slice(0, 12)}...${addr.slice(-8)}`;
}

export default function Delegators({ onchainPoolId }: DelegatorsProps) {
  const delegatorsQuery = useDelegatorListQuery(onchainPoolId);
  const delegators = delegatorsQuery.data ?? [];

  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [sortDesc, setSortDesc] = useState(true);

  const filtered = useMemo(() => {
    let list = delegators;
    if (search.trim()) {
      const term = search.trim().toLowerCase();
      list = list.filter((d) => d.stake_address.toLowerCase().includes(term));
    }
    if (!sortDesc) {
      list = [...list].reverse();
    }
    return list;
  }, [delegators, search, sortDesc]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const safePage = Math.min(page, totalPages - 1);
  const pageSlice = filtered.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);

  if (!onchainPoolId) {
    return (
      <section className="flex h-64 flex-col items-center justify-center gap-3">
        <p className="text-sm text-slate-500">绑定链上矿池后可查看质押用户</p>
        <Link
          to="/bind-pool"
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-blue-700"
        >
          绑定链上矿池
        </Link>
      </section>
    );
  }

  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold text-slate-900">Delegators</h2>
          <p className="text-xs text-slate-500">
            {delegatorsQuery.isSuccess
              ? `共 ${delegators.length} 个质押用户`
              : delegatorsQuery.isError
                ? "数据加载失败，将自动重试"
                : "加载中..."}
          </p>
        </div>
        <input
          type="text"
          value={search}
          onChange={(e) => { setSearch(e.target.value); setPage(0); }}
          placeholder="搜索 stake address..."
          className="w-64 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs text-slate-800 placeholder:text-slate-400 focus:border-blue-400 focus:outline-none focus:ring-2 focus:ring-blue-200"
        />
      </div>

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <table className="w-full text-left text-xs">
          <thead>
            <tr className="border-b border-slate-200 bg-slate-50 text-slate-500">
              <th className="px-4 py-2.5 font-semibold">#</th>
              <th className="px-4 py-2.5 font-semibold">Stake Address</th>
              <th className="px-4 py-2.5 font-semibold text-right">
                <button
                  type="button"
                  onClick={() => setSortDesc((v) => !v)}
                  className="inline-flex items-center gap-1 hover:text-slate-900"
                >
                  Amount (ADA)
                  <span className="text-[10px]">{sortDesc ? "▼" : "▲"}</span>
                </button>
              </th>
              <th className="px-4 py-2.5 font-semibold text-right">Since Epoch</th>
            </tr>
          </thead>
          <tbody>
            {pageSlice.length === 0 ? (
              <tr>
                <td colSpan={4} className="px-4 py-8 text-center text-slate-400">
                  {search.trim() ? "无匹配结果" : "暂无数据"}
                </td>
              </tr>
            ) : (
              pageSlice.map((d, i) => (
                <tr key={d.stake_address} className="border-b border-slate-100 last:border-0 hover:bg-slate-50">
                  <td className="px-4 py-2.5 tabular-nums text-slate-400">
                    {safePage * PAGE_SIZE + i + 1}
                  </td>
                  <td className="px-4 py-2.5 font-mono text-slate-700" title={d.stake_address}>
                    {truncateAddress(d.stake_address)}
                  </td>
                  <td className="px-4 py-2.5 text-right tabular-nums font-medium text-slate-900">
                    {d.amount_ada.toLocaleString(undefined, { maximumFractionDigits: 2 })}
                  </td>
                  <td className="px-4 py-2.5 text-right tabular-nums text-slate-500">
                    E{d.active_epoch_no}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {totalPages > 1 && (
        <div className="flex items-center justify-between text-xs text-slate-500">
          <span>
            Showing {safePage * PAGE_SIZE + 1}–{Math.min((safePage + 1) * PAGE_SIZE, filtered.length)} of {filtered.length}
          </span>
          <div className="flex items-center gap-1">
            <button
              type="button"
              disabled={safePage === 0}
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              className="rounded border border-slate-300 px-2 py-1 hover:bg-slate-100 disabled:opacity-40"
            >
              ‹
            </button>
            {Array.from({ length: Math.min(totalPages, 7) }, (_, i) => {
              const pageNum = totalPages <= 7 ? i : Math.max(0, Math.min(safePage - 3, totalPages - 7)) + i;
              return (
                <button
                  key={pageNum}
                  type="button"
                  onClick={() => setPage(pageNum)}
                  className={`rounded border px-2 py-1 ${
                    pageNum === safePage
                      ? "border-blue-300 bg-blue-50 font-semibold text-blue-700"
                      : "border-slate-300 hover:bg-slate-100"
                  }`}
                >
                  {pageNum + 1}
                </button>
              );
            })}
            <button
              type="button"
              disabled={safePage >= totalPages - 1}
              onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              className="rounded border border-slate-300 px-2 py-1 hover:bg-slate-100 disabled:opacity-40"
            >
              ›
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
