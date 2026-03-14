import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { formatTaskError } from "../lib/errors";
import { kesStatusAll, taskRecentList } from "../lib/ipc";
import {
  refreshMonitorStore,
  resolveTelemetryBehavior,
  setMonitorStorePollingInterval,
  startMonitorStore,
  stopMonitorStore,
  useMonitorStore,
} from "../lib/monitorStore";
import type { KesStatus, MonitorSnapshot, RecentTaskSummary } from "../lib/types";

const EPOCH_SLOTS_BY_NETWORK: Record<string, number> = {
  mainnet: 432000,
  preprod: 432000,
  preview: 432000,
};

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

function parseCollectedAt(value: string | null): number | null {
  if (!value) {
    return null;
  }
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)
    ? `${value.replace(" ", "T")}Z`
    : value;
  const parsed = Date.parse(normalized);
  if (Number.isNaN(parsed)) {
    return null;
  }
  return parsed;
}

function formatRelativeCollectedAt(value: string | null): string | null {
  const parsed = parseCollectedAt(value);
  if (parsed == null) {
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

function formatAbsoluteCollectedAt(value: string | null): string | null {
  const parsed = parseCollectedAt(value);
  if (parsed == null) {
    return null;
  }
  return new Date(parsed).toLocaleString();
}

function epochSlotsForNetwork(network: string): number {
  return EPOCH_SLOTS_BY_NETWORK[network] ?? 432000;
}

function monitorSyncPercent(snapshot: MonitorSnapshot): number | null {
  if (snapshot.sync_percent != null) {
    return snapshot.sync_percent;
  }
  if (snapshot.sync_progress != null) {
    return snapshot.sync_progress;
  }
  if (snapshot.slot_in_epoch == null) {
    return null;
  }
  const slotsPerEpoch = epochSlotsForNetwork(snapshot.network);
  if (slotsPerEpoch <= 0) {
    return null;
  }
  const raw = (snapshot.slot_in_epoch / slotsPerEpoch) * 100;
  return Number(Math.max(0, Math.min(100, raw)).toFixed(2));
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

function lateBlocksTone(value: number | null): "critical" | "warn" | "muted" {
  if (value == null) {
    return "muted";
  }
  if (value >= 200) {
    return "critical";
  }
  if (value >= 20) {
    return "warn";
  }
  return "muted";
}

function statusDotClass(status: string): string {
  switch (status) {
    case "telemetry_live":
    case "synced":
      return "bg-emerald-500";
    case "telemetry_stale":
    case "syncing":
      return "bg-amber-500";
    case "telemetry_unavailable":
    case "stalled":
    case "unreachable":
      return "bg-rose-500";
    default:
      return "bg-slate-400";
  }
}

function telemetryStatusLabel(status: string): string {
  switch (status) {
    case "telemetry_live":
      return "live";
    case "telemetry_stale":
      return "stale";
    case "telemetry_unavailable":
      return "unavailable";
    default:
      return status || "unknown";
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
        className="pointer-events-none absolute right-0 top-[calc(100%+8px)] z-20 hidden w-72 max-w-[min(28rem,90vw)] rounded-md border border-slate-700 bg-slate-900 px-2.5 py-2 text-xs leading-5 text-white shadow-xl group-hover:block group-focus-visible:block"
      >
        {tip}
      </span>
    </span>
  );
}

function InlineInfoTip({ tip }: { tip: string }) {
  return (
    <span className="group relative inline-flex" tabIndex={0}>
      <span className="inline-flex h-4 w-4 items-center justify-center rounded-full border border-slate-300 text-[10px] font-semibold text-slate-600">
        i
      </span>
      <span
        role="tooltip"
        className="pointer-events-none absolute right-0 top-[calc(100%+8px)] z-20 hidden w-64 max-w-[min(24rem,90vw)] rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5 text-xs leading-5 text-white shadow-xl group-hover:block group-focus-visible:block"
      >
        {tip}
      </span>
    </span>
  );
}

function MetaIconTip({ tip, icon }: { tip: string; icon: ReactNode }) {
  return (
    <span className="group relative inline-flex" tabIndex={0}>
      <span className="inline-flex h-5 w-5 items-center justify-center rounded-full border border-slate-300 bg-white text-slate-500">
        {icon}
      </span>
      <span
        role="tooltip"
        className="pointer-events-none absolute left-1/2 top-[calc(100%+8px)] z-20 hidden w-64 max-w-[min(22rem,90vw)] -translate-x-1/2 rounded-md border border-slate-700 bg-slate-900 px-2 py-1.5 text-xs leading-5 text-white shadow-xl group-hover:block group-focus-visible:block"
      >
        {tip}
      </span>
    </span>
  );
}

export default function Dashboard() {
  const foregroundIntervalSeconds = 15;
  const backgroundIntervalSeconds = 60;
  const [kesStatuses, setKesStatuses] = useState<KesStatus[]>([]);
  const [recentTasks, setRecentTasks] = useState<RecentTaskSummary[]>([]);
  const [copiedTaskId, setCopiedTaskId] = useState<string | null>(null);
  const [auxRefreshError, setAuxRefreshError] = useState<string | null>(null);
  const refreshInFlightRef = useRef(false);
  const {
    snapshots,
    status: monitorStatus,
    telemetryPhase,
    usingCachedData,
    lastCollectedAt,
    lastError,
  } = useMonitorStore();

  useEffect(() => {
    let active = true;
    let refreshTimer: number | null = null;

    const clearRefreshTimer = () => {
      if (refreshTimer != null) {
        window.clearInterval(refreshTimer);
        refreshTimer = null;
      }
    };

    const currentIntervalSeconds = () =>
      typeof document === "undefined" || document.visibilityState !== "hidden"
        ? foregroundIntervalSeconds
        : backgroundIntervalSeconds;

    const refreshDashboardData = async () => {
      if (!active || refreshInFlightRef.current) {
        return;
      }
      refreshInFlightRef.current = true;
      try {
        const [kesResult, taskResult] = await Promise.allSettled([kesStatusAll(), taskRecentList(8)]);
        if (!active) {
          return;
        }

        const failures: string[] = [];

        if (kesResult.status === "fulfilled") {
          setKesStatuses(kesResult.value);
        } else {
          failures.push("KES");
        }

        if (taskResult.status === "fulfilled") {
          setRecentTasks(taskResult.value);
        } else {
          failures.push("日志");
        }

        setAuxRefreshError(
          failures.length > 0 ? `部分数据刷新失败（${failures.join(" / ")}），将自动重试。` : null,
        );
      } finally {
        refreshInFlightRef.current = false;
      }
    };

    const scheduleDashboardRefresh = (intervalSeconds: number) => {
      clearRefreshTimer();
      refreshTimer = window.setInterval(() => {
        void refreshDashboardData();
      }, intervalSeconds * 1000);
    };

    const applyPollingMode = async (intervalSeconds: number) => {
      await setMonitorStorePollingInterval(intervalSeconds);
      scheduleDashboardRefresh(intervalSeconds);
    };

    const onVisibilityChange = () => {
      const visible = typeof document === "undefined" || document.visibilityState !== "hidden";
      if (visible) {
        void (async () => {
          await applyPollingMode(foregroundIntervalSeconds);
          await Promise.all([refreshDashboardData(), refreshMonitorStore()]);
        })();
        return;
      }
      void applyPollingMode(backgroundIntervalSeconds);
    };

    const onWindowFocus = () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
      void (async () => {
        await applyPollingMode(foregroundIntervalSeconds);
        await Promise.all([refreshDashboardData(), refreshMonitorStore()]);
      })();
    };

    void (async () => {
      try {
        const initialInterval = currentIntervalSeconds();
        await startMonitorStore(initialInterval);
        scheduleDashboardRefresh(initialInterval);
        await refreshDashboardData();
      } catch (error) {
        if (active) {
          setAuxRefreshError(`Dashboard 初始化失败：${String(error)}`);
        }
      }
    })();

    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }
    if (typeof window !== "undefined") {
      window.addEventListener("focus", onWindowFocus);
    }

    return () => {
      active = false;
      clearRefreshTimer();
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
      if (typeof window !== "undefined") {
        window.removeEventListener("focus", onWindowFocus);
      }
      void stopMonitorStore();
    };
  }, []);

  const nodes = useMemo(() => {
    const bp = snapshots.filter((row) => row.role === "bp");
    const relay = snapshots.filter((row) => row.role === "relay");
    const rest = snapshots.filter((row) => row.role !== "bp" && row.role !== "relay");
    return [...bp, ...relay, ...rest];
  }, [snapshots]);

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

  const clusterEpoch = useMemo(() => {
    const epochs = snapshots.map((row) => row.epoch).filter((value): value is number => value != null);
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

  const gatewayRuntimeSummary = useMemo(() => {
    const total = snapshots.length;
    if (total === 0) {
      return "--";
    }
    const relayApiReady = snapshots.filter((snapshot) => {
      const source = snapshot.prometheus_source ?? "";
      return source.startsWith("relay-api:");
    }).length;
    return `${relayApiReady}/${total}`;
  }, [snapshots]);
  const useHorizontalCardRail = cardNodes.length >= 3;

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
    <section className="space-y-5 overflow-x-hidden">
      <section className="overflow-hidden rounded-xl border border-slate-200 bg-slate-50 text-slate-900 shadow-sm">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 px-4 py-3">
          <div>
            <h2 className="text-sm font-semibold">集群概览（BP + Relays）</h2>
            <p className="text-xs text-slate-600">首屏展示链路风险与资源压力，节点卡即监控主入口。</p>
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
                className="pointer-events-none absolute right-0 top-[calc(100%+8px)] z-20 hidden w-72 max-w-[min(28rem,90vw)] rounded-md border border-slate-700 bg-slate-900 px-2.5 py-2 text-xs leading-5 text-white shadow-xl group-hover:block group-focus-visible:block"
              >
                {monitorPhaseText}
                {monitorCollectedAge ? ` · ${monitorCollectedAge}` : ""}。
                {lastError ? ` 最近错误: ${lastError}。` : ""}
                {` Gateway runtime: ${gatewayRuntimeSummary} nodes via relay-api。`}
                若刷新超时则继续展示缓存指标，并在下一轮轮询自动重试。
              </span>
            </span>
            <span className="text-[11px] text-slate-500">GW {gatewayRuntimeSummary}</span>
            <Link
              to="/telemetry"
              className="inline-flex h-6 items-center rounded border border-slate-300 bg-white px-2 text-[11px] font-semibold text-slate-700 transition hover:bg-slate-100"
              title="进入 Telemetry API 管理页"
            >
              管理 API
            </Link>
          </div>
        </header>

        {auxRefreshError && (
          <div className="px-4 pb-3">
            <p className="text-xs font-medium text-amber-700">{auxRefreshError}</p>
          </div>
        )}

        {cardNodes.length === 0 ? (
          <div className="p-4">
            <div className="rounded-lg border border-dashed border-slate-300 bg-white p-4 text-sm text-slate-500">
              No node snapshot yet. Telemetry will populate cards after monitor polling starts.
            </div>
          </div>
        ) : useHorizontalCardRail ? (
          <div className="px-4 py-3 pb-4">
            <div className="overflow-x-auto overflow-y-hidden [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden">
              <div className="flex snap-x snap-mandatory gap-3">
                {cardNodes.map((snapshot) => {
                  const syncPercent = monitorSyncPercent(snapshot);
                  const lateBlocks = snapshot.late_blocks;
                  const lateTone = lateBlocksTone(lateBlocks);
                  const isCritical =
                    snapshot.health_level === "critical" || snapshot.status === "telemetry_unavailable";
                  const isSlowest = slowestNode?.machine_id === snapshot.machine_id;
                  const isBp = snapshot.role === "bp";
                  const bpEpochDrift =
                    isBp && snapshot.epoch != null && clusterEpoch != null
                      ? snapshot.epoch - clusterEpoch
                      : null;
                  const progressWidth = Math.max(0, Math.min(100, syncPercent ?? 0));
                  const kesRemainDays = isBp ? bpKes?.remaining_days ?? null : null;
                  const kesRemainWindows =
                    isBp && bpKes?.kes_period_current != null && bpKes?.kes_period_max != null
                      ? Math.max(bpKes.kes_period_max - bpKes.kes_period_current, 0)
                      : null;
                  const kesLabel =
                    kesRemainDays != null ? `KES remain ${kesRemainDays}d` : "KES remain --";
                  const kesTipParts: string[] = [];
                  if (kesRemainWindows != null) {
                    kesTipParts.push(`窗口剩余 ${kesRemainWindows}`);
                  }
                  if (bpKes?.expiry_date) {
                    kesTipParts.push(`到期 ${bpKes.expiry_date}`);
                  }
                  if (kesTipParts.length === 0) {
                    kesTipParts.push("KES 剩余天数，建议提前完成 Rotate。");
                  }
                  const kesStatusClass =
                    bpKes?.severity === "critical"
                      ? "text-rose-700"
                      : bpKes?.severity === "warning"
                        ? "text-amber-700"
                        : "text-slate-600";

                  return (
                    <article
                      key={snapshot.machine_id}
                      className="relative w-[min(82vw,560px)] min-w-[360px] max-w-[560px] flex-none snap-start rounded-lg border border-slate-300 bg-white p-3 shadow-[0_4px_18px_rgba(15,23,42,0.06)] sm:min-w-[400px]"
                    >
                      <div className="absolute right-3 top-3 inline-flex flex-col items-end gap-1.5">
                        <TooltipBadge
                          label={`late ${formatCounter(lateBlocks)}`}
                          tone={lateTone}
                          tip="同步风险指标。值越高表示区块延迟累计越多。"
                        />
                      </div>

                      <header className="pr-28">
                        <div className="flex items-center gap-1.5">
                          <strong className="text-sm font-semibold">{snapshot.machine_name}</strong>
                          {isCritical && <span className={severityChipClass("critical")}>critical</span>}
                          {isSlowest && (
                            <TooltipBadge
                              label="slowest"
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
                        <p className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-slate-600">
                          <span className={`mr-1 inline-block h-1.5 w-1.5 rounded-full ${statusDotClass(snapshot.status)}`} />
                          <span>{snapshot.status === "telemetry_unavailable" ? "offline" : "online"} · {telemetryStatusLabel(snapshot.status)}</span>
                          <span>slot {formatCounter(snapshot.slot_num)}</span>
                          {isBp && (
                            <span className={kesStatusClass} title={kesTipParts.join(" · ")}>
                              {kesLabel}
                            </span>
                          )}
                        </p>
                      </header>

                      <div className="mt-3 grid gap-2 min-[480px]:grid-cols-[minmax(0,1.85fr)_minmax(0,1fr)]">
                        <section className="rounded-lg border border-slate-200 bg-white px-3 py-2">
                          <div className="flex items-end justify-between gap-2">
                            <div>
                              <p className="text-[11px] text-slate-500">Block</p>
                              <strong className="text-[22px] font-semibold leading-none text-slate-900">
                                {formatCounter(snapshot.block_height)}
                              </strong>
                            </div>
                            <div className="text-right">
                              <p className="text-[11px] text-slate-500">Epoch</p>
                              <div className="inline-flex items-center gap-1.5">
                                <strong className="text-base font-semibold leading-none text-slate-900">
                                  {snapshot.epoch ?? "--"}
                                </strong>
                              </div>
                            </div>
                          </div>

                          <div className="mt-2 flex items-center justify-between text-xs text-slate-600">
                            <span>Sync</span>
                            <div className="inline-flex items-center gap-1.5">
                              <strong className="text-sm text-slate-900">{formatProgress(syncPercent)}</strong>
                              {isBp && (
                                <TooltipBadge
                                  label={bpEpochDrift == null ? "Δ--" : `Δ${bpEpochDrift >= 0 ? "+" : ""}${bpEpochDrift}e`}
                                  tone={bpEpochDrift != null && bpEpochDrift < 0 ? "warn" : "muted"}
                                  tip="BP 与集群最新 epoch 的差值。负值表示仍落后。"
                                />
                              )}
                            </div>
                          </div>

                          <div className="mt-1.5 h-1.5 rounded-full bg-slate-200">
                            <span
                              className="block h-full rounded-full bg-gradient-to-r from-blue-500 to-blue-700"
                              style={{ width: `${progressWidth}%` }}
                            />
                          </div>
                        </section>

                        <section className="rounded-lg border border-sky-100 bg-sky-50/70 px-2.5 py-2">
                          <p className="text-[11px] font-semibold text-slate-600">Runtime</p>
                          <div className="mt-2 flex flex-col gap-2">
                            <div className="inline-flex items-center justify-between rounded-full border border-sky-100 bg-white px-2.5 py-1 text-[11px] text-slate-600 whitespace-nowrap">
                              <span>CPU (sys)</span>
                              <strong className="text-xs font-semibold text-slate-900">
                                {formatProgress(snapshot.cpu_sys_percent)}
                              </strong>
                            </div>
                            <div className="inline-flex items-center justify-between gap-1 rounded-full border border-sky-100 bg-white px-2.5 py-1 text-[11px] text-slate-600 whitespace-nowrap">
                              <span>Mem RSS</span>
                              <strong className="text-xs font-semibold text-slate-900">
                                {formatMemoryGigabytes(snapshot.mem_rss_bytes)}
                              </strong>
                              <InlineInfoTip tip={`Mem (Live): ${formatMemoryGigabytes(snapshot.mem_live_bytes)}`} />
                            </div>
                          </div>
                        </section>
                      </div>

                      <div className="mt-2 flex items-center justify-end gap-1.5 text-xs text-slate-500">
                        {snapshot.prometheus_source && (
                          <MetaIconTip
                            tip={`source: ${snapshot.prometheus_source}`}
                            icon={
                              <svg viewBox="0 0 20 20" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                                <path d="M8.2 11.8 11.8 8.2m-5 6.4-2 2a2.6 2.6 0 0 1-3.6-3.6l2-2m8.8-2.8 2-2a2.6 2.6 0 1 1 3.6 3.6l-2 2m-6.4-5 2-2a2.6 2.6 0 0 1 3.6 3.6l-2 2m-5 6.4-2 2a2.6 2.6 0 0 1-3.6-3.6l2-2" strokeLinecap="round" />
                              </svg>
                            }
                          />
                        )}
                        {snapshot.collected_at && (
                          <MetaIconTip
                            tip={`sample: ${formatRelativeCollectedAt(snapshot.collected_at) ?? "--"} · ${formatAbsoluteCollectedAt(snapshot.collected_at) ?? snapshot.collected_at}`}
                            icon={
                              <svg viewBox="0 0 20 20" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                                <circle cx="10" cy="10" r="6.8" />
                                <path d="M10 6.8v3.6l2.6 1.6" strokeLinecap="round" />
                              </svg>
                            }
                          />
                        )}
                        {snapshot.prometheus_note && (
                          <MetaIconTip
                            tip={`note: ${snapshot.prometheus_note}`}
                            icon={
                              <svg viewBox="0 0 20 20" className="h-3.5 w-3.5 text-amber-600" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                                <path d="M10 3.2 17 16.4H3L10 3.2Z" />
                                <path d="M10 7.6v4.3m0 2.1h.01" strokeLinecap="round" />
                              </svg>
                            }
                          />
                        )}
                      </div>
                    </article>
                  );
                })}
              </div>
            </div>
          </div>
        ) : (
          <div className="grid gap-3 p-4 md:grid-cols-2">
            {cardNodes.map((snapshot) => {
              const syncPercent = monitorSyncPercent(snapshot);
              const lateBlocks = snapshot.late_blocks;
              const lateTone = lateBlocksTone(lateBlocks);
              const isCritical =
                snapshot.health_level === "critical" || snapshot.status === "telemetry_unavailable";
              const isSlowest = slowestNode?.machine_id === snapshot.machine_id;
              const isBp = snapshot.role === "bp";
              const bpEpochDrift =
                isBp && snapshot.epoch != null && clusterEpoch != null
                  ? snapshot.epoch - clusterEpoch
                  : null;
              const progressWidth = Math.max(0, Math.min(100, syncPercent ?? 0));
              const kesRemainDays = isBp ? bpKes?.remaining_days ?? null : null;
              const kesRemainWindows =
                isBp && bpKes?.kes_period_current != null && bpKes?.kes_period_max != null
                  ? Math.max(bpKes.kes_period_max - bpKes.kes_period_current, 0)
                  : null;
              const kesLabel =
                kesRemainDays != null ? `KES remain ${kesRemainDays}d` : "KES remain --";
              const kesTipParts: string[] = [];
              if (kesRemainWindows != null) {
                kesTipParts.push(`窗口剩余 ${kesRemainWindows}`);
              }
              if (bpKes?.expiry_date) {
                kesTipParts.push(`到期 ${bpKes.expiry_date}`);
              }
              if (kesTipParts.length === 0) {
                kesTipParts.push("KES 剩余天数，建议提前完成 Rotate。");
              }
              const kesStatusClass =
                bpKes?.severity === "critical"
                  ? "text-rose-700"
                  : bpKes?.severity === "warning"
                    ? "text-amber-700"
                    : "text-slate-600";

              return (
                <article
                  key={snapshot.machine_id}
                  className="relative w-full rounded-lg border border-slate-300 bg-white p-3 shadow-[0_4px_18px_rgba(15,23,42,0.06)]"
                >
                  <div className="absolute right-3 top-3 inline-flex flex-col items-end gap-1.5">
                    <TooltipBadge
                      label={`late ${formatCounter(lateBlocks)}`}
                      tone={lateTone}
                      tip="同步风险指标。值越高表示区块延迟累计越多。"
                    />
                  </div>

                  <header className="pr-28">
                    <div className="flex items-center gap-1.5">
                      <strong className="text-sm font-semibold">{snapshot.machine_name}</strong>
                      {isCritical && <span className={severityChipClass("critical")}>critical</span>}
                      {isSlowest && (
                        <TooltipBadge
                          label="slowest"
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
                    <p className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-slate-600">
                      <span className={`mr-1 inline-block h-1.5 w-1.5 rounded-full ${statusDotClass(snapshot.status)}`} />
                      <span>{snapshot.status === "telemetry_unavailable" ? "offline" : "online"} · {telemetryStatusLabel(snapshot.status)}</span>
                      <span>slot {formatCounter(snapshot.slot_num)}</span>
                      {isBp && (
                        <span className={kesStatusClass} title={kesTipParts.join(" · ")}>
                          {kesLabel}
                        </span>
                      )}
                    </p>
                  </header>

                  <div className="mt-3 grid gap-2 min-[480px]:grid-cols-[minmax(0,1.85fr)_minmax(0,1fr)]">
                    <section className="rounded-lg border border-slate-200 bg-white px-3 py-2">
                      <div className="flex items-end justify-between gap-2">
                        <div>
                          <p className="text-[11px] text-slate-500">Block</p>
                          <strong className="text-[22px] font-semibold leading-none text-slate-900">
                            {formatCounter(snapshot.block_height)}
                          </strong>
                        </div>
                        <div className="text-right">
                          <p className="text-[11px] text-slate-500">Epoch</p>
                        <div className="inline-flex items-center gap-1.5">
                          <strong className="text-base font-semibold leading-none text-slate-900">
                            {snapshot.epoch ?? "--"}
                          </strong>
                        </div>
                      </div>
                    </div>

                      <div className="mt-2 flex items-center justify-between text-xs text-slate-600">
                        <span>Sync</span>
                        <div className="inline-flex items-center gap-1.5">
                          <strong className="text-sm text-slate-900">{formatProgress(syncPercent)}</strong>
                          {isBp && (
                            <TooltipBadge
                              label={bpEpochDrift == null ? "Δ--" : `Δ${bpEpochDrift >= 0 ? "+" : ""}${bpEpochDrift}e`}
                              tone={bpEpochDrift != null && bpEpochDrift < 0 ? "warn" : "muted"}
                              tip="BP 与集群最新 epoch 的差值。负值表示仍落后。"
                            />
                          )}
                        </div>
                      </div>

                      <div className="mt-1.5 h-1.5 rounded-full bg-slate-200">
                        <span
                          className="block h-full rounded-full bg-gradient-to-r from-blue-500 to-blue-700"
                          style={{ width: `${progressWidth}%` }}
                        />
                      </div>
                    </section>

                    <section className="rounded-lg border border-sky-100 bg-sky-50/70 px-2.5 py-2">
                      <p className="text-[11px] font-semibold text-slate-600">Runtime</p>
                      <div className="mt-2 flex flex-col gap-2">
                        <div className="inline-flex items-center justify-between rounded-full border border-sky-100 bg-white px-2.5 py-1 text-[11px] text-slate-600 whitespace-nowrap">
                          <span>CPU (sys)</span>
                          <strong className="text-xs font-semibold text-slate-900">
                            {formatProgress(snapshot.cpu_sys_percent)}
                          </strong>
                        </div>
                        <div className="inline-flex items-center justify-between gap-1 rounded-full border border-sky-100 bg-white px-2.5 py-1 text-[11px] text-slate-600 whitespace-nowrap">
                          <span>Mem RSS</span>
                          <strong className="text-xs font-semibold text-slate-900">
                            {formatMemoryGigabytes(snapshot.mem_rss_bytes)}
                          </strong>
                          <InlineInfoTip tip={`Mem (Live): ${formatMemoryGigabytes(snapshot.mem_live_bytes)}`} />
                        </div>
                      </div>
                    </section>
                  </div>

                  <div className="mt-2 flex items-center justify-end gap-1.5 text-xs text-slate-500">
                    {snapshot.prometheus_source && (
                      <MetaIconTip
                        tip={`source: ${snapshot.prometheus_source}`}
                        icon={
                          <svg viewBox="0 0 20 20" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                            <path d="M8.2 11.8 11.8 8.2m-5 6.4-2 2a2.6 2.6 0 0 1-3.6-3.6l2-2m8.8-2.8 2-2a2.6 2.6 0 1 1 3.6 3.6l-2 2m-6.4-5 2-2a2.6 2.6 0 0 1 3.6 3.6l-2 2m-5 6.4-2 2a2.6 2.6 0 0 1-3.6-3.6l2-2" strokeLinecap="round" />
                          </svg>
                        }
                      />
                    )}
                    {snapshot.collected_at && (
                      <MetaIconTip
                        tip={`sample: ${formatRelativeCollectedAt(snapshot.collected_at) ?? "--"} · ${formatAbsoluteCollectedAt(snapshot.collected_at) ?? snapshot.collected_at}`}
                        icon={
                          <svg viewBox="0 0 20 20" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                            <circle cx="10" cy="10" r="6.8" />
                            <path d="M10 6.8v3.6l2.6 1.6" strokeLinecap="round" />
                          </svg>
                        }
                      />
                    )}
                    {snapshot.prometheus_note && (
                      <MetaIconTip
                        tip={`note: ${snapshot.prometheus_note}`}
                        icon={
                          <svg viewBox="0 0 20 20" className="h-3.5 w-3.5 text-amber-600" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                            <path d="M10 3.2 17 16.4H3L10 3.2Z" />
                            <path d="M10 7.6v4.3m0 2.1h.01" strokeLinecap="round" />
                          </svg>
                        }
                      />
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        )}
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
                            className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded border border-slate-300 bg-white text-slate-600 transition hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300"
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
      </section>
    </section>
  );
}
