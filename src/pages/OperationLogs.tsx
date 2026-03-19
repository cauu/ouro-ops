import { useMemo, useState } from "react";
import { formatTaskError } from "../lib/errors";
import { useTaskLogQuery } from "../lib/queries";
import type { RecentTaskSummary } from "../lib/types";

const STATUS_OPTIONS = ["pending", "running", "paused", "success", "failed", "cancelled"];
const TASK_TYPE_OPTIONS = [
  "deploy",
  "upgrade",
  "kes_rotation",
  "rollback",
  "health_check",
  "hardening",
  "runtime_config",
  "runtime_restart",
  "observability_bootstrap",
  "observability_rollback",
];

function formatTaskLabel(value: string): string {
  return value.split("_").join(" ");
}

function formatTargetLabel(machineCount: number): string {
  if (machineCount <= 0) {
    return "--";
  }
  if (machineCount === 1) {
    return "单节点";
  }
  return `集群 (${machineCount})`;
}

function statusToneClass(status: string): string {
  const base = "inline-flex min-h-6 items-center rounded-full border px-2 text-[11px] font-semibold leading-none";
  switch (status) {
    case "success":
      return `${base} border-emerald-300 bg-emerald-50 text-emerald-700`;
    case "partial":
      return `${base} border-amber-300 bg-amber-50 text-amber-700`;
    case "failed":
    case "cancelled":
      return `${base} border-rose-300 bg-rose-50 text-rose-700`;
    case "running":
      return `${base} border-sky-300 bg-sky-50 text-sky-700`;
    default:
      return `${base} border-slate-300 bg-slate-100 text-slate-700`;
  }
}

async function copyPlainText(value: string): Promise<boolean> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return true;
    }
  } catch {
    // fallback below
  }

  try {
    if (typeof document === "undefined") {
      return false;
    }
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.setAttribute("readonly", "true");
    textarea.style.position = "absolute";
    textarea.style.left = "-9999px";
    document.body.appendChild(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    document.body.removeChild(textarea);
    return copied;
  } catch {
    return false;
  }
}

export default function OperationLogs() {
  const [copiedTaskId, setCopiedTaskId] = useState<string | null>(null);

  const [keywordInput, setKeywordInput] = useState("");
  const [statusInput, setStatusInput] = useState("");
  const [taskTypeInput, setTaskTypeInput] = useState("");

  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState("");
  const [taskType, setTaskType] = useState("");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);

  const { data: pageData, isLoading: loading, error: queryError } = useTaskLogQuery({
    page,
    page_size: pageSize,
    keyword: keyword || undefined,
    status: status || undefined,
    task_type: taskType || undefined,
  });
  const error = queryError ? String(queryError) : null;

  const currentItems = useMemo<RecentTaskSummary[]>(() => pageData?.items ?? [], [pageData]);

  const total = pageData?.total ?? 0;
  const totalPages = pageData?.total_pages ?? 1;

  const handleCopyDetail = async (taskId: string, detailText: string) => {
    const copied = await copyPlainText(detailText);
    if (!copied) {
      return;
    }
    setCopiedTaskId(taskId);
    window.setTimeout(() => {
      setCopiedTaskId((current) => (current === taskId ? null : current));
    }, 1200);
  };

  return (
    <section className="space-y-5">
      <section className="rounded-xl border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
        <header className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold">操作日志</h2>
            <p className="text-xs text-slate-600">支持分页浏览与条件查询，覆盖全部任务执行记录。</p>
          </div>
          <span className="text-xs text-slate-500">total {total}</span>
        </header>

        <form
          className="mt-3 grid gap-2 rounded-lg border border-slate-200 bg-white p-3 md:grid-cols-[minmax(0,1.4fr)_180px_220px_auto_auto]"
          onSubmit={(event) => {
            event.preventDefault();
            setPage(1);
            setKeyword(keywordInput.trim());
            setStatus(statusInput);
            setTaskType(taskTypeInput);
          }}
        >
          <input
            value={keywordInput}
            onChange={(event) => {
              setKeywordInput(event.target.value);
            }}
            className="h-9 min-w-0 rounded border border-slate-300 px-3 text-sm text-slate-700 outline-none transition focus:border-blue-300 focus:ring-2 focus:ring-blue-100 focus-visible:border-blue-300 focus-visible:ring-2 focus-visible:ring-blue-100"
            placeholder="查询 task_id / task_type / status / phase / error"
          />
          <select
            value={statusInput}
            onChange={(event) => {
              setStatusInput(event.target.value);
            }}
            className="h-9 rounded border border-slate-300 px-2 text-sm text-slate-700 outline-none transition focus:border-blue-300 focus:ring-2 focus:ring-blue-100 focus-visible:border-blue-300 focus-visible:ring-2 focus-visible:ring-blue-100"
          >
            <option value="">全部状态</option>
            {STATUS_OPTIONS.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
          <select
            value={taskTypeInput}
            onChange={(event) => {
              setTaskTypeInput(event.target.value);
            }}
            className="h-9 rounded border border-slate-300 px-2 text-sm text-slate-700 outline-none transition focus:border-blue-300 focus:ring-2 focus:ring-blue-100 focus-visible:border-blue-300 focus-visible:ring-2 focus-visible:ring-blue-100"
          >
            <option value="">全部操作类型</option>
            {TASK_TYPE_OPTIONS.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
          <button
            type="submit"
            className="inline-flex h-9 items-center justify-center rounded border border-slate-300 bg-white px-3 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
          >
            查询
          </button>
          <button
            type="button"
            onClick={() => {
              setKeywordInput("");
              setStatusInput("");
              setTaskTypeInput("");
              setPage(1);
              setKeyword("");
              setStatus("");
              setTaskType("");
            }}
            className="inline-flex h-9 items-center justify-center rounded border border-slate-300 bg-white px-3 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
          >
            重置
          </button>
        </form>

        {error && (
          <p role="alert" className="mt-3 text-xs font-medium text-rose-700">
            {error}
          </p>
        )}

        <div className="mt-3 overflow-x-auto rounded-lg border border-slate-200 bg-white">
          <p className="px-3 py-1.5 text-xs text-slate-500" role="note">
            表格较宽，小屏可左右滑动查看。
          </p>
          <table className="w-full min-w-[960px] table-fixed text-left text-xs">
            <colgroup>
              <col className="w-[170px]" />
              <col className="w-[160px]" />
              <col className="w-[120px]" />
              <col className="w-[110px]" />
              <col className="w-[360px]" />
            </colgroup>
            <thead className="bg-slate-100 text-slate-600">
              <tr>
                <th className="px-3 py-2">时间</th>
                <th className="px-3 py-2">操作</th>
                <th className="px-3 py-2">目标</th>
                <th className="px-3 py-2">状态</th>
                <th className="px-3 py-2">详情</th>
              </tr>
            </thead>
            <tbody>
              {loading ? (
                <tr>
                  <td colSpan={5} className="px-3 py-4 text-center text-slate-500">
                    Loading...
                  </td>
                </tr>
              ) : currentItems.length === 0 ? (
                <tr>
                  <td colSpan={5} className="px-3 py-4 text-center text-slate-500">
                    No matching logs.
                  </td>
                </tr>
              ) : (
                currentItems.map((task) => {
                  const taskError = formatTaskError(task.error_msg);
                  const detailText = taskError
                    ? taskError
                    : task.phase
                      ? formatTaskLabel(task.phase)
                      : `${task.machine_count} machine(s)`;
                  return (
                    <tr key={task.task_id} className="border-t border-slate-200">
                      <td className="px-3 py-2 text-slate-600">{task.created_at}</td>
                      <td className="px-3 py-2 font-medium text-slate-900">{formatTaskLabel(task.task_type)}</td>
                      <td className="px-3 py-2 text-slate-600">{formatTargetLabel(task.machine_count)}</td>
                      <td className="px-3 py-2">
                        <span className={statusToneClass(task.status)}>{formatTaskLabel(task.status)}</span>
                      </td>
                      <td className="w-0 max-w-[360px] px-3 py-2 text-slate-600">
                        <div className="flex min-w-0 items-center gap-1.5">
                          <span
                            title={detailText}
                            className={`block min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap select-text ${
                              taskError ? "text-rose-700" : "text-slate-600"
                            }`}
                          >
                            {detailText}
                          </span>
                          <button
                            type="button"
                            onClick={() => {
                              void handleCopyDetail(task.task_id, detailText);
                            }}
                            className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded border border-slate-300 bg-white text-slate-600 transition hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
                            title={copiedTaskId === task.task_id ? "已复制" : "复制详情"}
                            aria-label={copiedTaskId === task.task_id ? "已复制" : "复制详情"}
                          >
                            {copiedTaskId === task.task_id ? (
                              <svg
                                viewBox="0 0 20 20"
                                className="h-3.5 w-3.5"
                                fill="none"
                                stroke="currentColor"
                                strokeWidth="1.8"
                                aria-hidden="true"
                              >
                                <path d="M4 10.5l3.2 3.2L16 5.9" strokeLinecap="round" strokeLinejoin="round" />
                              </svg>
                            ) : (
                              <svg
                                viewBox="0 0 20 20"
                                className="h-3.5 w-3.5"
                                fill="none"
                                stroke="currentColor"
                                strokeWidth="1.6"
                                aria-hidden="true"
                              >
                                <rect x="7" y="7" width="9" height="9" rx="1.6" />
                                <path
                                  d="M5.2 12.8H4a1.6 1.6 0 0 1-1.6-1.6V4a1.6 1.6 0 0 1 1.6-1.6h7.2A1.6 1.6 0 0 1 12.8 4v1.2"
                                  strokeLinecap="round"
                                />
                              </svg>
                            )}
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>

        <footer className="mt-3 flex flex-wrap items-center justify-between gap-2">
          <div className="inline-flex items-center gap-2 text-xs text-slate-600">
            <span>
              page {pageData?.page ?? page} / {totalPages}
            </span>
            <span>·</span>
            <span>{total} records</span>
          </div>

          <div className="inline-flex items-center gap-2">
            <select
              value={String(pageSize)}
              onChange={(event) => {
                const next = Number(event.target.value);
                if (!Number.isFinite(next) || next <= 0) {
                  return;
                }
                setPage(1);
                setPageSize(next);
              }}
              className="h-8 rounded border border-slate-300 px-2 text-xs text-slate-700"
            >
              <option value="20">20 / page</option>
              <option value="50">50 / page</option>
              <option value="100">100 / page</option>
            </select>
            <button
              type="button"
              onClick={() => {
                setPage((current) => Math.max(1, current - 1));
              }}
              disabled={(pageData?.page ?? page) <= 1}
              className="inline-flex h-8 items-center rounded border border-slate-300 bg-white px-3 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
            >
              上一页
            </button>
            <button
              type="button"
              onClick={() => {
                setPage((current) => Math.min(totalPages, current + 1));
              }}
              disabled={(pageData?.page ?? page) >= totalPages}
              className="inline-flex h-8 items-center rounded border border-slate-300 bg-white px-3 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
            >
              下一页
            </button>
          </div>
        </footer>
      </section>
    </section>
  );
}
