import { type ReactNode, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { formatTaskError } from "../lib/errors";
import { kesStatusAll, taskRecentList } from "../lib/ipc";
import {
  resolveTelemetryBehavior,
  startMonitorStore,
  stopMonitorStore,
  useMonitorStore,
} from "../lib/monitorStore";
import type {
  KesStatus,
  MonitorSnapshot,
  RecentTaskSummary,
} from "../lib/types";

function formatProgress(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return `${value.toFixed(2)}%`;
}

function formatMemoryGigabytes(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return `${(value / (1024 ** 3)).toFixed(1)}G`;
}

function formatCounter(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return Math.round(value).toLocaleString();
}

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

function formatRelativeCollectedAt(value: string | null): string | null {
  if (!value) {
    return null;
  }
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) {
    return null;
  }
  const diffMs = Date.now() - parsed;
  if (diffMs < 60_000) {
    return "just now";
  }
  if (diffMs < 3_600_000) {
    return `${Math.floor(diffMs / 60_000)}m ago`;
  }
  if (diffMs < 86_400_000) {
    return `${Math.floor(diffMs / 3_600_000)}h ago`;
  }
  return `${Math.floor(diffMs / 86_400_000)}d ago`;
}

function monitorSyncPercent(snapshot: MonitorSnapshot): number | null {
  return snapshot.sync_percent ?? snapshot.sync_progress;
}

function estimateWithinOneSecond(snapshot: MonitorSnapshot): number | null {
  const peerCount = snapshot.peer_count;
  const sync = monitorSyncPercent(snapshot);
  if (peerCount == null || sync == null) {
    return null;
  }
  let value = Math.min(99.9, Math.max(75, sync - 1.2));
  if (snapshot.status === "syncing") {
    value -= 1.8;
  }
  if (snapshot.status === "stalled" || snapshot.status === "unreachable") {
    value -= 4;
  }
  return Math.max(0, Number(value.toFixed(2)));
}

function estimateLatencyBuckets(withinOneSecond: number | null): Array<{ label: string; value: number | null }> {
  if (withinOneSecond == null) {
    return [
      { label: "0-50ms", value: null },
      { label: "50-100ms", value: null },
      { label: "100-500ms", value: null },
      { label: ">1s", value: null },
    ];
  }
  const within = Math.max(0, Math.min(100, withinOneSecond));
  const tail = Math.max(0, Number((100 - within).toFixed(2)));
  const p0 = Number((within * 0.6).toFixed(2));
  const p1 = Number((within * 0.25).toFixed(2));
  const p2 = Number((within - p0 - p1).toFixed(2));
  return [
    { label: "0-50ms", value: p0 },
    { label: "50-100ms", value: p1 },
    { label: "100-500ms", value: p2 },
    { label: ">1s", value: tail },
  ];
}

function telemetryDotClass(behavior: string): string {
  switch (behavior) {
    case "syncing_live":
      return "border-sky-300 border-t-sky-600";
    case "cache_ready":
      return "border-sky-300 border-t-sky-600";
    case "live":
      return "border-emerald-300 border-t-emerald-600";
    case "degraded_retrying":
      return "border-amber-300 border-t-amber-500";
    default:
      return "border-slate-300 border-t-slate-500";
  }
}

function monitorPhaseLabel(behavior: string, fallback: string): string {
  switch (behavior) {
    case "cache_ready":
      return "已加载本地缓存";
    case "syncing_live":
      return "后台静默刷新 Prometheus 最新数据中";
    case "live":
      return "Telemetry 已更新";
    case "degraded_retrying":
      return "刷新超时，继续展示缓存并自动重试";
    default:
      return fallback;
  }
}

function severityChipClass(severity: "critical" | "warn" | "ok" | "muted" = "muted"): string {
  const base = "inline-flex min-h-7 items-center rounded-full border px-2.5 text-[11px] font-semibold leading-none";
  switch (severity) {
    case "critical":
      return `${base} border-rose-300 bg-rose-50 text-rose-700`;
    case "warn":
      return `${base} border-amber-300 bg-amber-50 text-amber-700`;
    case "ok":
      return `${base} border-emerald-300 bg-emerald-50 text-emerald-700`;
    default:
      return `${base} border-slate-300 bg-slate-100 text-slate-700`;
  }
}

function statusDotClass(status: string): string {
  switch (status) {
    case "synced":
      return "bg-emerald-500";
    case "syncing":
      return "bg-amber-500";
    case "stalled":
    case "unreachable":
      return "bg-rose-500";
    default:
      return "bg-slate-400";
  }
}

function statusToneClass(status: string): string {
  switch (status) {
    case "success":
      return severityChipClass("ok");
    case "partial":
      return severityChipClass("warn");
    case "failed":
    case "cancelled":
      return severityChipClass("critical");
    case "running":
      return severityChipClass("warn");
    default:
      return severityChipClass("muted");
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

interface TooltipBadgeProps {
  label: ReactNode;
  tip: string;
  tone?: "critical" | "warn" | "ok" | "muted";
}

function TooltipBadge({ label, tip, tone = "muted" }: TooltipBadgeProps) {
  return (
    <span tabIndex={0} className="group relative inline-flex">
      <span className={severityChipClass(tone)}>{label}</span>
      <span
        role="tooltip"
        className="pointer-events-none absolute right-0 top-[calc(100%+8px)] z-20 w-72 max-w-[min(28rem,90vw)] rounded-md border border-slate-700 bg-slate-900 px-2.5 py-2 text-xs leading-5 text-white opacity-0 shadow-xl transition group-hover:opacity-100 group-focus-visible:opacity-100"
      >
        {tip}
      </span>
    </span>
  );
}

export default function Dashboard() {
  const [kesStatuses, setKesStatuses] = useState<KesStatus[]>([]);
  const [recentTasks, setRecentTasks] = useState<RecentTaskSummary[]>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<number | null>(null);
  const [copiedTaskId, setCopiedTaskId] = useState<string | null>(null);
  const {
    snapshots,
    status: monitorStatus,
    telemetryPhase,
    usingCachedData,
    lastCollectedAt,
    lastError,
  } = useMonitorStore();

  useEffect(() => {
    void (async () => {
      try {
        await startMonitorStore(30);
        const [nextKes, nextTasks] = await Promise.all([kesStatusAll(), taskRecentList(8)]);
        setKesStatuses(nextKes);
        setRecentTasks(nextTasks);
      } catch {
        setKesStatuses([]);
        setRecentTasks([]);
      }
    })();
    return () => {
      void stopMonitorStore();
    };
  }, []);

  const nodes = useMemo(() => {
    const bp = snapshots.filter((row) => row.role === "bp");
    const relay = snapshots.filter((row) => row.role === "relay");
    const rest = snapshots.filter((row) => row.role !== "bp" && row.role !== "relay");
    return [...bp, ...relay, ...rest];
  }, [snapshots]);

  useEffect(() => {
    if (nodes.length === 0) {
      setSelectedNodeId(null);
      return;
    }
    if (selectedNodeId == null || !nodes.some((row) => row.machine_id === selectedNodeId)) {
      setSelectedNodeId(nodes[0].machine_id);
    }
  }, [nodes, selectedNodeId]);

  const selectedNode = useMemo(
    () => nodes.find((row) => row.machine_id === selectedNodeId) ?? null,
    [nodes, selectedNodeId],
  );

  const bpNode = useMemo(() => nodes.find((row) => row.role === "bp") ?? null, [nodes]);

  const relays = useMemo(() => nodes.filter((row) => row.role === "relay"), [nodes]);

  const cardNodes = useMemo(() => {
    const picked: MonitorSnapshot[] = [];
    if (bpNode) {
      picked.push(bpNode);
    }
    picked.push(...relays);
    nodes.forEach((row) => {
      if (!picked.some((entry) => entry.machine_id === row.machine_id)) {
        picked.push(row);
      }
    });
    return picked;
  }, [bpNode, relays, nodes]);

  const headBlock = useMemo(() => {
    const heights = snapshots
      .map((row) => row.block_height)
      .filter((value): value is number => value != null);
    if (heights.length === 0) {
      return null;
    }
    return Math.max(...heights);
  }, [snapshots]);

  const clusterEpoch = useMemo(() => {
    const epochs = snapshots
      .map((row) => row.epoch)
      .filter((value): value is number => value != null);
    if (epochs.length === 0) {
      return null;
    }
    return Math.max(...epochs);
  }, [snapshots]);

  const slowestNode = useMemo(() => {
    const sorted = snapshots
      .filter((row) => monitorSyncPercent(row) != null)
      .sort((a, b) => (monitorSyncPercent(a) ?? 0) - (monitorSyncPercent(b) ?? 0));
    return sorted[0] ?? null;
  }, [snapshots]);

  const bpKes = useMemo(() => {
    if (!bpNode) {
      return null;
    }
    return kesStatuses.find((row) => row.machine_id === bpNode.machine_id) ?? null;
  }, [bpNode, kesStatuses]);

  const telemetryBehavior = useMemo(
    () =>
      resolveTelemetryBehavior({
        telemetryPhase,
        usingCachedData,
        snapshots,
      }),
    [telemetryPhase, usingCachedData, snapshots],
  );
  const monitorPhaseText = monitorPhaseLabel(telemetryBehavior, monitorStatus);
  const monitorCollectedAge = formatRelativeCollectedAt(lastCollectedAt);
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
      <section className="rounded-xl border border-slate-200 bg-slate-50 text-slate-900 shadow-sm">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 px-4 py-3">
          <div>
            <h2 className="text-sm font-semibold">集群概览（BP + Relays）</h2>
            <p className="text-xs text-slate-600">关键风险收敛到 BP 卡片，细节通过轻量标签与 tooltip 承载。</p>
          </div>
          <div className="inline-flex items-center gap-2 rounded-full border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-700">
            <span
              aria-hidden="true"
              className={`relative inline-flex h-4 w-4 items-center justify-center ${
                telemetryBehavior === "syncing_live" ? "animate-spin" : ""
              }`}
            >
              <span className={`absolute inset-0 rounded-full border-2 ${telemetryDotClass(telemetryBehavior)}`} />
              <span className="absolute top-[-2px] h-1.5 w-1.5 rounded-full bg-sky-500 shadow-[0_0_0_2px_rgba(255,255,255,0.9)]" />
            </span>
            <span className="font-semibold text-slate-900">Telemetry</span>
            <span className="group relative inline-flex" tabIndex={0}>
              <span className="inline-flex h-5 w-5 items-center justify-center rounded-full border border-slate-300 text-[11px] font-semibold text-slate-600">
                i
              </span>
              <span
                role="tooltip"
                className="pointer-events-none absolute right-0 top-[calc(100%+8px)] z-20 w-72 max-w-[min(28rem,90vw)] rounded-md border border-slate-700 bg-slate-900 px-2.5 py-2 text-xs leading-5 text-white opacity-0 shadow-xl transition group-hover:opacity-100 group-focus-visible:opacity-100"
              >
                {monitorPhaseText}
                {monitorCollectedAge ? ` · ${monitorCollectedAge}` : ""}。
                {lastError ? ` 最近错误: ${lastError}。` : ""}
                若刷新超时则继续展示缓存指标，并在下一轮轮询自动重试。
              </span>
            </span>
          </div>
        </header>

        <div className="grid gap-3 p-4 lg:grid-cols-3">
          {cardNodes.length === 0 ? (
            <div className="rounded-lg border border-dashed border-slate-300 bg-white p-4 text-sm text-slate-500 lg:col-span-3">
              No node snapshot yet. Telemetry will populate cards after monitor polling starts.
            </div>
          ) : (
            cardNodes.map((snapshot) => {
              const syncPercent = monitorSyncPercent(snapshot);
              const blockDiff =
                snapshot.tip_diff_blocks ??
                (snapshot.block_height != null && headBlock != null ? snapshot.block_height - headBlock : null);
              const isCritical = snapshot.health_level === "critical" || snapshot.status === "unreachable";
              const isSlowest = slowestNode?.machine_id === snapshot.machine_id;
              const isBp = snapshot.role === "bp";
              const bpEpochDrift =
                isBp && snapshot.epoch != null && clusterEpoch != null
                  ? snapshot.epoch - clusterEpoch
                  : null;
              const progressWidth = Math.max(0, Math.min(100, syncPercent ?? 0));
              const kesRemainWindows =
                isBp && bpKes?.kes_period_current != null && bpKes?.kes_period_max != null
                  ? Math.max(bpKes.kes_period_max - bpKes.kes_period_current, 0)
                  : null;
              const kesLabel =
                kesRemainWindows != null ? `KES remain ${kesRemainWindows}` : "KES remain --";
              return (
                <article
                  key={snapshot.machine_id}
                  className="rounded-lg border border-slate-300 bg-white p-3 shadow-[0_4px_18px_rgba(15,23,42,0.06)]"
                >
                  <header className="flex items-start justify-between gap-2">
                    <strong className="text-sm font-semibold">{snapshot.machine_name}</strong>
                    <div className="inline-flex items-center gap-1.5">
                      {isCritical && <span className={severityChipClass("critical")}>critical</span>}
                      {isSlowest && (
                        <TooltipBadge
                          label={`slowest · ${snapshot.machine_name}`}
                          tone="critical"
                          tip="Current slowest node by sync progress in this cluster snapshot."
                        />
                      )}
                      {!isCritical && !isSlowest && (
                        <span className={severityChipClass(snapshot.health_level === "healthy" ? "ok" : "warn")}>
                          {snapshot.health_level}
                        </span>
                      )}
                    </div>
                  </header>
                  <p className="mt-1 text-xs text-slate-600">
                    <span className={`mr-1 inline-block h-1.5 w-1.5 rounded-full ${statusDotClass(snapshot.status)}`} />
                    {snapshot.status === "unreachable" ? "offline" : "online"} · uptime --
                  </p>

                  <div className="mt-2 flex items-center justify-between text-xs text-slate-600">
                    <span>Epoch</span>
                    <div className="inline-flex items-center gap-1.5">
                      <strong className="text-sm text-slate-900">{snapshot.epoch ?? "--"}</strong>
                      {isBp && clusterEpoch != null && (
                        <TooltipBadge
                          label={`cluster ${clusterEpoch}`}
                          tip="Highest epoch observed in current cluster telemetry."
                        />
                      )}
                    </div>
                  </div>

                  <div className="mt-1.5 flex items-center justify-between text-xs text-slate-600">
                    <span>Sync</span>
                    <div className="inline-flex items-center gap-1.5">
                      <strong className="text-sm text-slate-900">{formatProgress(syncPercent)}</strong>
                      {isBp && (
                        <TooltipBadge
                          label={
                            bpEpochDrift == null
                              ? "Δ--"
                              : `Δ${bpEpochDrift >= 0 ? "+" : ""}${bpEpochDrift}e`
                          }
                          tone={bpEpochDrift != null && bpEpochDrift < 0 ? "warn" : "muted"}
                          tip="BP 与集群最新 epoch 的差值。负值表示仍落后。"
                        />
                      )}
                    </div>
                  </div>

                  <div className="mt-2 h-1.5 rounded-full bg-slate-200">
                    <span
                      className="block h-full rounded-full bg-gradient-to-r from-blue-500 to-blue-700"
                      style={{ width: `${progressWidth}%` }}
                    />
                  </div>

                  <p className="mt-2 text-xs text-slate-500">
                    tip diff: {blockDiff == null ? "--" : `${blockDiff >= 0 ? "+" : ""}${blockDiff} blocks`}
                  </p>

                  {isBp && (
                    <div className="mt-2 flex items-center justify-between gap-2">
                      <TooltipBadge
                        label={
                          <span className="inline-flex items-center gap-1">
                            <span className="h-1.5 w-1.5 rounded-full bg-rose-500 animate-pulse" />
                            <span>{kesLabel}</span>
                          </span>
                        }
                        tone={bpKes?.severity === "critical" ? "critical" : bpKes?.severity === "warning" ? "warn" : "muted"}
                        tip="KES 剩余窗口，建议提前完成 Rotate。"
                      />
                      <Link
                        to="/kes"
                        className="inline-flex min-h-8 items-center rounded-md bg-blue-600 px-3 text-xs font-semibold text-white hover:bg-blue-700"
                      >
                        立即 Rotate
                      </Link>
                    </div>
                  )}
                </article>
              );
            })
          )}
        </div>
      </section>

      <section className="rounded-xl border border-slate-200 bg-slate-50 text-slate-900 shadow-sm">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 px-4 py-3">
          <div>
            <h2 className="text-sm font-semibold">节点详情</h2>
            <p className="text-xs text-slate-600">Tab 切换 BP / Relay，查看资源、连接与传播延迟。</p>
          </div>
        </header>

        <div className="space-y-3 p-4">
          {nodes.length === 0 ? (
            <div className="rounded-lg border border-dashed border-slate-300 bg-white p-4 text-sm text-slate-500">
              Waiting for node telemetry...
            </div>
          ) : (
            <>
              <div className="tab-controller">
                {nodes.map((row) => (
                  <input
                    key={`node-tab-radio-${row.machine_id}`}
                    id={`node-tab-${row.machine_id}`}
                    type="radio"
                    name="node-tab"
                    className="sr-only"
                    checked={selectedNodeId === row.machine_id}
                    onChange={() => setSelectedNodeId(row.machine_id)}
                  />
                ))}
                <fieldset className="inline-flex flex-wrap items-center gap-2 rounded-lg border border-slate-300 bg-slate-100 p-1">
                  <legend className="sr-only">选择节点</legend>
                {nodes.map((row) => {
                  const active = selectedNodeId === row.machine_id;
                  return (
                    <label
                      key={row.machine_id}
                      htmlFor={`node-tab-${row.machine_id}`}
                      className={`inline-flex min-h-8 min-w-28 items-center justify-center rounded-md border px-3 text-xs font-semibold leading-none ${
                        active
                          ? "border-blue-300 bg-white text-blue-700"
                          : "border-transparent bg-transparent text-slate-600 hover:text-slate-900"
                      }`}
                    >
                      {row.machine_name}
                    </label>
                  );
                })}
                </fieldset>
              </div>

              {selectedNode && (
                <div className="grid gap-3 lg:grid-cols-2">
                  <article className="rounded-lg border border-slate-300 bg-white p-3">
                    <h3 className="text-sm font-semibold">Resources</h3>
                    <dl className="mt-2 space-y-1.5 text-sm">
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">CPU (sys)</dt>
                        <dd className="font-medium text-slate-900">{formatProgress(selectedNode.cpu_sys_percent)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Mem (Live)</dt>
                        <dd className="font-medium text-slate-900">{formatMemoryGigabytes(selectedNode.mem_live_bytes)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Mem (RSS)</dt>
                        <dd className="font-medium text-slate-900">{formatMemoryGigabytes(selectedNode.mem_rss_bytes)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Mem (Heap)</dt>
                        <dd className="font-medium text-slate-900">{formatMemoryGigabytes(selectedNode.mem_heap_bytes)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">GC Minor</dt>
                        <dd className="font-medium text-slate-900">{formatCounter(selectedNode.gc_minor_total)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">GC Major</dt>
                        <dd className="font-medium text-slate-900">{formatCounter(selectedNode.gc_major_total)}</dd>
                      </div>
                    </dl>
                    <p className="mt-2 text-xs text-slate-500">
                      block: {selectedNode.block_height?.toLocaleString() ?? "--"} · slot: --
                    </p>
                  </article>

                  <article className="rounded-lg border border-slate-300 bg-white p-3">
                    <h3 className="text-sm font-semibold">Connections & Peers</h3>
                    <dl className="mt-2 space-y-1.5 text-sm">
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Peers</dt>
                        <dd className="font-medium text-slate-900">
                          {selectedNode.peer_count == null ? "--" : `${formatCounter(selectedNode.peer_count)} / 32`}
                        </dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Within 1s</dt>
                        <dd className="font-medium text-slate-900">
                          {estimateWithinOneSecond(selectedNode) == null
                            ? "--"
                            : `${estimateWithinOneSecond(selectedNode)?.toFixed(2)}%`}
                        </dd>
                      </div>
                    </dl>
                    <div className="mt-3 space-y-2">
                      {estimateLatencyBuckets(estimateWithinOneSecond(selectedNode)).map((bucket) => (
                        <div key={`${selectedNode.machine_id}-${bucket.label}`} className="grid grid-cols-[72px_1fr_52px] items-center gap-2 text-xs">
                          <span className="text-slate-600">{bucket.label}</span>
                          <span className="h-2 rounded-full bg-slate-200">
                            <span
                              className={`block h-2 rounded-full ${bucket.label === ">1s" ? "bg-rose-400" : "bg-blue-500"}`}
                              style={{ width: `${bucket.value ?? 0}%` }}
                            />
                          </span>
                          <span className="text-right font-medium text-slate-700">
                            {bucket.value == null ? "--" : `${bucket.value.toFixed(2)}%`}
                          </span>
                        </div>
                      ))}
                    </div>
                    {selectedNode.note && (
                      <p className="mt-2 rounded-md border border-rose-200 bg-rose-50 px-2.5 py-2 text-xs text-rose-700">
                        {selectedNode.note}
                      </p>
                    )}
                  </article>
                </div>
              )}
            </>
          )}
        </div>
      </section>

      <section className="rounded-xl border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
        <header className="flex items-center justify-between gap-3">
          <h2 className="text-sm font-semibold">近期操作日志</h2>
          <span className="text-xs text-slate-500">最近 {Math.min(recentTasks.length, 6)} 条</span>
        </header>
        <div className="mt-3 overflow-x-hidden rounded-lg border border-slate-200 bg-white">
          <table className="w-full table-fixed text-left text-xs">
            <colgroup>
              <col className="w-[168px]" />
              <col className="w-[132px]" />
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
              {recentTasks.length === 0 ? (
                <tr>
                  <td colSpan={5} className="px-3 py-4 text-center text-slate-500">
                    No tasks recorded yet.
                  </td>
                </tr>
              ) : (
                recentTasks.slice(0, 6).map((task) => {
                  const taskError = formatTaskError(task.error_msg);
                  const detailText = taskError ? taskError : (task.phase ? formatTaskLabel(task.phase) : `${task.machine_count} machine(s)`);
                  return (
                    <tr key={task.task_id} className="border-t border-slate-200">
                      <td className="px-3 py-2 text-slate-600">{task.created_at}</td>
                      <td className="px-3 py-2 font-medium text-slate-900">{formatTaskLabel(task.task_type)}</td>
                      <td className="px-3 py-2 text-slate-600">{formatTargetLabel(task.machine_count)}</td>
                      <td className="px-3 py-2">
                        <span className={statusToneClass(task.status)}>{formatTaskLabel(task.status)}</span>
                      </td>
                      <td className="w-0 max-w-[360px] px-3 py-2 text-slate-600">
                        <div className="flex items-center gap-1.5 min-w-0">
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
                            className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded border border-slate-300 bg-white text-slate-600 transition hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300"
                            title={copiedTaskId === task.task_id ? "已复制" : "复制详情"}
                            aria-label={copiedTaskId === task.task_id ? "已复制" : "复制详情"}
                          >
                            {copiedTaskId === task.task_id ? (
                              <svg viewBox="0 0 20 20" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                                <path d="M4 10.5l3.2 3.2L16 5.9" strokeLinecap="round" strokeLinejoin="round" />
                              </svg>
                            ) : (
                              <svg viewBox="0 0 20 20" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                                <rect x="7" y="7" width="9" height="9" rx="1.6" />
                                <path d="M5.2 12.8H4a1.6 1.6 0 0 1-1.6-1.6V4a1.6 1.6 0 0 1 1.6-1.6h7.2A1.6 1.6 0 0 1 12.8 4v1.2" strokeLinecap="round" />
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
      </section>
    </section>
  );
}
