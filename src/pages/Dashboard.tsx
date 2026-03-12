import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { formatTaskError, toUserError } from "../lib/errors";
import {
  dbVersion,
  kesStatusAll,
  ping,
  poolUnbindOnchain,
  poolRefreshBoundOnchain,
  taskRecentList,
} from "../lib/ipc";
import {
  refreshMonitorStore,
  startMonitorStore,
  stopMonitorStore,
  useMonitorStore,
} from "../lib/monitorStore";
import type {
  DbVersionResult,
  KesStatus,
  MonitorSnapshot,
  Pool,
  RecentTaskSummary,
} from "../lib/types";
import PoolRegistrationStatus from "./PoolRegistrationStatus";
import PoolRegistrationWizard from "./PoolRegistrationWizard";

function formatProgress(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return `${value.toFixed(2)}%`;
}

function formatBlocksPerMinute(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return value.toFixed(2);
}

function formatLovelace(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return value.toLocaleString();
}

function formatMargin(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return `${(value * 100).toFixed(2)}%`;
}

function formatTaskLabel(value: string): string {
  return value.split("_").join(" ");
}

function formatStage(stage: string): string {
  switch (stage) {
    case "snapshot_restoring":
      return "snapshot restoring";
    case "restore_failed":
      return "restore failed";
    case "restore_timeout":
      return "restore timeout";
    case "fallback_syncing":
      return "fallback syncing";
    default:
      return stage.split("_").join(" ");
  }
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

function telemetryDotClass(phase: string): string {
  switch (phase) {
    case "syncing_live":
    case "loading_cache":
      return "border-sky-300 border-t-sky-600 animate-spin";
    case "live":
      return "border-emerald-300 border-t-emerald-600";
    case "degraded":
      return "border-amber-300 border-t-amber-500";
    default:
      return "border-slate-300 border-t-slate-500";
  }
}

function monitorPhaseLabel(phase: string, fallback: string, usingCachedData: boolean): string {
  switch (phase) {
    case "loading_cache":
      return "Loading cached telemetry";
    case "syncing_live":
      return usingCachedData
        ? "Refreshing latest telemetry in background"
        : "Fetching latest telemetry";
    case "live":
      return "Telemetry updated";
    case "degraded":
      return usingCachedData
        ? "Live telemetry delayed, using cached data"
        : "Telemetry temporarily unavailable";
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
    case "failed":
    case "cancelled":
      return severityChipClass("critical");
    case "running":
      return severityChipClass("warn");
    default:
      return severityChipClass("muted");
  }
}

interface TooltipBadgeProps {
  label: string;
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

interface DashboardProps {
  pool: Pool;
  onPoolRefreshed: (pool: Pool) => void;
}

export default function Dashboard({ pool, onPoolRefreshed }: DashboardProps) {
  const [status, setStatus] = useState<string>("loading");
  const [dbInfo, setDbInfo] = useState<DbVersionResult | null>(null);
  const [kesStatuses, setKesStatuses] = useState<KesStatus[]>([]);
  const [recentTasks, setRecentTasks] = useState<RecentTaskSummary[]>([]);
  const [unbindError, setUnbindError] = useState<string | null>(null);
  const [unbinding, setUnbinding] = useState(false);
  const [selectedNodeId, setSelectedNodeId] = useState<number | null>(null);
  const {
    snapshots,
    status: monitorStatus,
    telemetryPhase,
    usingCachedData,
    lastCollectedAt,
  } = useMonitorStore();

  const refreshMonitor = useCallback(async () => {
    await refreshMonitorStore();
    const [nextKes, nextTasks] = await Promise.all([kesStatusAll(), taskRecentList(8)]);
    setKesStatuses(nextKes);
    setRecentTasks(nextTasks);
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        await ping();
        setStatus("Sidecar OK");
      } catch (error) {
        setStatus(`Sidecar error: ${toUserError(error)}`);
      }

      try {
        const version = await dbVersion();
        setDbInfo(version);
      } catch {
        setDbInfo(null);
      }

      try {
        await startMonitorStore(30);
        const [nextKes, nextTasks] = await Promise.all([kesStatusAll(), taskRecentList(8)]);
        setKesStatuses(nextKes);
        setRecentTasks(nextTasks);
      } catch (error) {
        setStatus((prev) => `${prev} · monitor: ${toUserError(error)}`);
      }
    })();
    return () => {
      void stopMonitorStore();
    };
  }, [refreshMonitor]);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const nextPool = await poolRefreshBoundOnchain();
        if (active) {
          onPoolRefreshed(nextPool);
        }
      } catch {
        // Best-effort background refresh. Ignore when no pool is bound yet or query is unavailable.
      }
    })();
    return () => {
      active = false;
    };
  }, [onPoolRefreshed]);

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
    picked.push(...relays.slice(0, 2));
    if (picked.length < 3) {
      nodes.forEach((row) => {
        if (!picked.some((entry) => entry.machine_id === row.machine_id)) {
          picked.push(row);
        }
      });
    }
    return picked.slice(0, 3);
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

  const slowestNode = useMemo(() => {
    const sorted = snapshots
      .filter((row) => row.sync_progress != null)
      .sort((a, b) => (a.sync_progress ?? 0) - (b.sync_progress ?? 0));
    return sorted[0] ?? null;
  }, [snapshots]);

  const bestRelaySync = useMemo(() => {
    const values = relays
      .map((row) => row.sync_progress)
      .filter((value): value is number => value != null);
    if (values.length === 0) {
      return null;
    }
    return Math.max(...values);
  }, [relays]);

  const bpKes = useMemo(() => {
    if (!bpNode) {
      return null;
    }
    return kesStatuses.find((row) => row.machine_id === bpNode.machine_id) ?? null;
  }, [bpNode, kesStatuses]);

  const monitorPhaseText = monitorPhaseLabel(telemetryPhase, monitorStatus, usingCachedData);
  const monitorCollectedAge = formatRelativeCollectedAt(lastCollectedAt);

  const handleUnbindPool = async () => {
    setUnbindError(null);
    setUnbinding(true);
    try {
      const nextPool = await poolUnbindOnchain();
      onPoolRefreshed(nextPool);
    } catch (error) {
      setUnbindError(toUserError(error));
    } finally {
      setUnbinding(false);
    }
  };

  return (
    <section className="space-y-5">
      <header className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">Dashboard</h1>
        <p className="text-xs text-zinc-400">
          Pool: <span className="font-medium text-zinc-200">{pool.ticker}</span> · network {pool.network}
          {dbInfo ? ` · db v${dbInfo.user_version}` : ""} · {status}
        </p>
      </header>

      <section className="rounded-xl border border-slate-200 bg-slate-50 text-slate-900 shadow-sm">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 px-4 py-3">
          <div>
            <h2 className="text-sm font-semibold">Cluster Overview (BP + Relays)</h2>
            <p className="text-xs text-slate-600">关键风险收敛到 BP 卡片，细节通过轻量标签与 tooltip 承载。</p>
          </div>
          <div className="inline-flex items-center gap-2 rounded-full border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-700">
            <span
              aria-hidden="true"
              className={`h-3 w-3 rounded-full border-2 ${telemetryDotClass(telemetryPhase)}`}
            />
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
                {monitorCollectedAge ? ` · ${monitorCollectedAge}` : ""}. Cached data stays visible while background
                refresh retries automatically.
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
              const blockDiff =
                snapshot.block_height != null && headBlock != null ? snapshot.block_height - headBlock : null;
              const isCritical = snapshot.health_level === "critical" || snapshot.status === "unreachable";
              const isSlowest = slowestNode?.machine_id === snapshot.machine_id;
              const isBp = snapshot.role === "bp";
              const bpDrift =
                isBp && snapshot.sync_progress != null && bestRelaySync != null
                  ? snapshot.sync_progress - bestRelaySync
                  : null;
              const progressWidth = Math.max(0, Math.min(100, snapshot.sync_progress ?? 0));
              const kesLabel =
                isBp && bpKes?.remaining_days != null ? `KES remain ${bpKes.remaining_days}d` : "KES remain --";
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
                    {snapshot.status} · stage {formatStage(snapshot.sync_stage)}
                  </p>

                  <div className="mt-2 flex items-center justify-between text-xs text-slate-600">
                    <span>Height</span>
                    <div className="inline-flex items-center gap-1.5">
                      <strong className="text-sm text-slate-900">{snapshot.block_height ?? "--"}</strong>
                      {headBlock != null && (
                        <TooltipBadge
                          label={`head ${headBlock}`}
                          tip="Highest block height currently observed in cluster snapshots."
                        />
                      )}
                    </div>
                  </div>

                  <div className="mt-1.5 flex items-center justify-between text-xs text-slate-600">
                    <span>Sync</span>
                    <div className="inline-flex items-center gap-1.5">
                      <strong className="text-sm text-slate-900">{formatProgress(snapshot.sync_progress)}</strong>
                      {isBp && (
                        <TooltipBadge
                          label={bpDrift == null ? "Δ --" : `Δ ${bpDrift.toFixed(2)}%`}
                          tone={bpDrift != null && bpDrift < -0.5 ? "warn" : "muted"}
                          tip="Difference between BP sync and the fastest relay sync progress."
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
                        label={kesLabel}
                        tone={bpKes?.severity === "critical" ? "critical" : bpKes?.severity === "warning" ? "warn" : "muted"}
                        tip="KES remaining window for BP. Rotate proactively before entering critical window."
                      />
                      <Link
                        to="/kes"
                        className="inline-flex min-h-8 items-center rounded-md bg-blue-600 px-3 text-xs font-semibold text-white hover:bg-blue-700"
                      >
                        Rotate Now
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
            <h2 className="text-sm font-semibold">Node Details</h2>
            <p className="text-xs text-slate-600">Tab 切换 BP / Relay，资源指标将由 Prometheus 映射补齐。</p>
          </div>
          <button
            type="button"
            onClick={() => void refreshMonitor()}
            className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-100"
          >
            Refresh
          </button>
        </header>

        <div className="space-y-3 p-4">
          {nodes.length === 0 ? (
            <div className="rounded-lg border border-dashed border-slate-300 bg-white p-4 text-sm text-slate-500">
              Waiting for node telemetry...
            </div>
          ) : (
            <>
              <div className="inline-grid grid-cols-3 justify-start gap-2 rounded-lg border border-slate-300 bg-slate-100 p-1">
                {nodes.slice(0, 3).map((row) => {
                  const active = selectedNodeId === row.machine_id;
                  return (
                    <button
                      key={row.machine_id}
                      type="button"
                      onClick={() => setSelectedNodeId(row.machine_id)}
                      className={`inline-flex min-h-8 items-center justify-center rounded-md border px-3 text-xs font-semibold leading-none ${
                        active
                          ? "border-blue-300 bg-white text-blue-700"
                          : "border-transparent bg-transparent text-slate-600 hover:text-slate-900"
                      }`}
                    >
                      {row.machine_name}
                    </button>
                  );
                })}
              </div>

              {selectedNode && (
                <div className="grid gap-3 lg:grid-cols-2">
                  <article className="rounded-lg border border-slate-300 bg-white p-3">
                    <h3 className="text-sm font-semibold">Resources</h3>
                    <dl className="mt-2 space-y-1.5 text-sm">
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">CPU (sys)</dt>
                        <dd className="font-medium text-slate-900">--</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Mem (Live)</dt>
                        <dd className="font-medium text-slate-900">--</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Mem (RSS)</dt>
                        <dd className="font-medium text-slate-900">--</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Mem (Heap)</dt>
                        <dd className="font-medium text-slate-900">--</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">GC Minor</dt>
                        <dd className="font-medium text-slate-900">--</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">GC Major</dt>
                        <dd className="font-medium text-slate-900">--</dd>
                      </div>
                    </dl>
                    <p className="mt-2 text-xs text-slate-500">
                      Prometheus resource mapping will be enabled in `p8-5`.
                    </p>
                  </article>

                  <article className="rounded-lg border border-slate-300 bg-white p-3">
                    <h3 className="text-sm font-semibold">Connections & Sync</h3>
                    <dl className="mt-2 space-y-1.5 text-sm">
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Status</dt>
                        <dd className="font-medium text-slate-900">{selectedNode.status}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Sync</dt>
                        <dd className="font-medium text-slate-900">{formatProgress(selectedNode.sync_progress)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Blocks/min</dt>
                        <dd className="font-medium text-slate-900">
                          {formatBlocksPerMinute(selectedNode.blocks_per_minute)}
                        </dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Block Height</dt>
                        <dd className="font-medium text-slate-900">{selectedNode.block_height ?? "--"}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Sync Stage</dt>
                        <dd className="font-medium text-slate-900">{formatStage(selectedNode.sync_stage)}</dd>
                      </div>
                      <div className="flex items-center justify-between">
                        <dt className="text-slate-600">Collected</dt>
                        <dd className="font-medium text-slate-900">{selectedNode.collected_at}</dd>
                      </div>
                    </dl>
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

      <section className="rounded-xl border border-zinc-800 bg-zinc-900/70 p-4 text-zinc-100">
        {pool.onchain_registered && pool.onchain_pool_id ? (
          <>
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-sm font-semibold text-zinc-100">Bound On-chain Pool</h2>
                <p className="mt-1 text-xs text-zinc-400">
                  Dashboard silently refreshes this data in the background on each visit.
                </p>
              </div>
              <span className="rounded-full bg-emerald-900/40 px-2 py-1 text-xs font-medium text-emerald-300">
                bound
              </span>
            </div>
            <div className="mt-3 flex items-center gap-3">
              <button
                type="button"
                onClick={handleUnbindPool}
                disabled={unbinding}
                className="rounded-md border border-red-800 bg-red-950/30 px-3 py-2 text-sm font-medium text-red-200 transition hover:border-red-700 hover:bg-red-950/50 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {unbinding ? "Unbinding..." : "Unbind Pool"}
              </button>
              <p className="text-xs text-zinc-500">
                Clears the workspace&apos;s on-chain binding and cached on-chain fields. Running nodes are not changed.
              </p>
            </div>
            {unbindError && (
              <div className="mt-3 rounded-md border border-red-900/40 bg-red-950/30 px-3 py-2 text-sm text-red-200">
                {unbindError}
              </div>
            )}
            <dl className="mt-4 grid gap-3 text-sm md:grid-cols-2 xl:grid-cols-3">
              <div>
                <dt className="text-zinc-500">Pool ID</dt>
                <dd className="mt-1 break-all font-medium text-zinc-100">{pool.onchain_pool_id}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Ticker</dt>
                <dd className="mt-1 font-medium text-zinc-100">{pool.ticker}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Last Synced</dt>
                <dd className="mt-1 font-medium text-zinc-100">{pool.onchain_synced_at ?? "--"}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Margin</dt>
                <dd className="mt-1 font-medium text-zinc-100">{formatMargin(pool.margin)}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Fixed Cost</dt>
                <dd className="mt-1 font-medium text-zinc-100">{formatLovelace(pool.fixed_cost)}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Pledge</dt>
                <dd className="mt-1 font-medium text-zinc-100">{formatLovelace(pool.pledge)}</dd>
              </div>
            </dl>
          </>
        ) : (
          <section className="grid gap-4 lg:grid-cols-[1.4fr_1fr]">
            <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
              <div className="mb-4">
                <h2 className="text-sm font-semibold text-zinc-100">Bind Existing Pool</h2>
                <p className="mt-1 text-sm text-zinc-400">
                  This workspace has no on-chain pool binding yet. If the pool is already registered on-chain, query it
                  by `pool_id` and bind it here.
                </p>
              </div>
              <PoolRegistrationStatus poolTicker={pool.ticker} onBound={onPoolRefreshed} embedded />
            </div>
            <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
              <h2 className="text-sm font-semibold text-zinc-100">Register New Pool</h2>
              <p className="mt-2 text-sm text-zinc-400">
                If this workspace does not correspond to an existing on-chain `pool_id`, use the registration flow
                below. The hot node only prepares an unsigned transaction and submits a pre-signed transaction;
                certificate generation and signing stay in the cold environment.
              </p>
              <div className="mt-4">
                <PoolRegistrationWizard poolTicker={pool.ticker} />
              </div>
            </div>
          </section>
        )}
      </section>

      <section className="rounded-xl border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
        <header className="flex items-center justify-between gap-3">
          <h2 className="text-sm font-semibold">Recent Operation Logs</h2>
          <span className="text-xs text-slate-500">latest {Math.min(recentTasks.length, 6)} entries</span>
        </header>
        <div className="mt-3 overflow-x-auto rounded-lg border border-slate-200 bg-white">
          <table className="min-w-full text-left text-xs">
            <thead className="bg-slate-100 text-slate-600">
              <tr>
                <th className="px-3 py-2">Time</th>
                <th className="px-3 py-2">Task</th>
                <th className="px-3 py-2">Status</th>
                <th className="px-3 py-2">Phase</th>
                <th className="px-3 py-2">Detail</th>
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
                  return (
                    <tr key={task.task_id} className="border-t border-slate-200">
                      <td className="px-3 py-2 text-slate-600">{task.created_at}</td>
                      <td className="px-3 py-2 font-medium text-slate-900">{formatTaskLabel(task.task_type)}</td>
                      <td className="px-3 py-2">
                        <span className={statusToneClass(task.status)}>{formatTaskLabel(task.status)}</span>
                      </td>
                      <td className="px-3 py-2 text-slate-600">{task.phase ? formatTaskLabel(task.phase) : "--"}</td>
                      <td className="px-3 py-2 text-slate-600">
                        {taskError ? (
                          <span title={taskError} className="text-rose-700">
                            {taskError}
                          </span>
                        ) : (
                          `${task.machine_count} machine(s)`
                        )}
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
