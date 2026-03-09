import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { formatTaskError, toUserError } from "../lib/errors";
import {
  dbVersion,
  kesStatusAll,
  ping,
  poolUnbindOnchain,
  poolRefreshBoundOnchain,
  runPlaybookTest,
  taskRecentList,
} from "../lib/ipc";
import {
  refreshMonitorStore,
  startMonitorStore,
  stopMonitorStore,
  useMonitorStore,
} from "../lib/monitorStore";
import type { DbVersionResult, KesStatus, Pool, RecentTaskSummary } from "../lib/types";
import PoolRegistrationStatus from "./PoolRegistrationStatus";
import PoolRegistrationWizard from "./PoolRegistrationWizard";

function formatProgress(value: number | null): string {
  if (value === null) {
    return "--";
  }
  return `${value.toFixed(2)}%`;
}

function formatBlocksPerMinute(value: number | null): string {
  if (value === null) {
    return "--";
  }
  return value.toFixed(2);
}

function statusTone(status: string): string {
  switch (status) {
    case "synced":
      return "text-emerald-300";
    case "syncing":
      return "text-amber-300";
    case "stalled":
      return "text-red-300";
    case "unreachable":
      return "text-red-400";
    default:
      return "text-zinc-300";
  }
}

function stageTone(stage: string): string {
  switch (stage) {
    case "snapshot_restoring":
      return "text-sky-300";
    case "restore_failed":
    case "restore_timeout":
      return "text-red-300";
    case "fallback_syncing":
      return "text-orange-300";
    case "synced":
      return "text-emerald-300";
    case "syncing":
      return "text-amber-300";
    default:
      return "text-zinc-300";
  }
}

function healthTone(level: string): string {
  switch (level) {
    case "healthy":
      return "text-emerald-300";
    case "critical":
      return "text-red-300";
    default:
      return "text-amber-300";
  }
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

function severityTone(severity: string): string {
  switch (severity) {
    case "healthy":
      return "text-emerald-300";
    case "critical":
      return "text-red-300";
    default:
      return "text-amber-300";
  }
}

function taskTone(status: string): string {
  switch (status) {
    case "success":
      return "text-emerald-300";
    case "failed":
    case "cancelled":
      return "text-red-300";
    case "paused":
      return "text-amber-300";
    case "running":
      return "text-sky-300";
    default:
      return "text-zinc-300";
  }
}

function formatTaskLabel(value: string): string {
  return value.split("_").join(" ");
}

function truncatePreview(value: string, maxLength = 180): string {
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, maxLength).trimEnd()}...`;
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

interface DashboardProps {
  pool: Pool;
  onPoolRefreshed: (pool: Pool) => void;
}

export default function Dashboard({ pool, onPoolRefreshed }: DashboardProps) {
  const [status, setStatus] = useState<string>("loading");
  const [dbInfo, setDbInfo] = useState<DbVersionResult | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [events, setEvents] = useState<string[]>([]);
  const [kesStatuses, setKesStatuses] = useState<KesStatus[]>([]);
  const [recentTasks, setRecentTasks] = useState<RecentTaskSummary[]>([]);
  const [unbindError, setUnbindError] = useState<string | null>(null);
  const [unbinding, setUnbinding] = useState(false);
  const { snapshots, status: monitorStatus } = useMonitorStore();

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
        const version = await dbVersion();
        setDbInfo(version);
        await startMonitorStore(30);
        const [nextKes, nextTasks] = await Promise.all([kesStatusAll(), taskRecentList(8)]);
        setKesStatuses(nextKes);
        setRecentTasks(nextTasks);
      } catch (error) {
        setStatus(`Error: ${toUserError(error)}`);
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

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const eventNames = ["task:progress", "task:log", "task:completed", "task:failed"];
    eventNames.forEach((eventName) => {
      void listen(eventName, (event) => {
        setEvents((prev) => [...prev.slice(-59), `${eventName}: ${JSON.stringify(event.payload)}`]);
      }).then((unlisten) => {
        unlisteners.push(unlisten);
      });
    });
    return () => {
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);
  const handleRunTest = async () => {
    try {
      const id = await runPlaybookTest();
      setTaskId(id);
    } catch (error) {
      setEvents((prev) => [...prev, `run_playbook_test error: ${toUserError(error)}`]);
    }
  };

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

  const healthyMachines = snapshots.filter((row) => row.health_level === "healthy").length;
  const warningMachines = snapshots.filter((row) => row.health_level === "warning").length;
  const criticalMachines = snapshots.filter((row) => row.health_level === "critical").length;

  return (
    <section className="space-y-4">
      <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
      <p title={status} className="text-sm text-zinc-300 break-words">
        <span className="font-medium text-zinc-100">Sidecar:</span> {truncatePreview(status, 160)}
      </p>
      {dbInfo && (
        <p className="text-sm text-zinc-300">
          <span className="font-medium text-zinc-100">DB:</span> user_version={dbInfo.user_version}
        </p>
      )}
      <div className="grid gap-4 xl:grid-cols-[1.4fr_1fr]">
        {pool.onchain_registered && pool.onchain_pool_id ? (
          <section className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
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
              <div>
                <dt className="text-zinc-500">Reward Account</dt>
                <dd className="mt-1 break-all font-medium text-zinc-100">{pool.reward_account ?? "--"}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Metadata URL</dt>
                <dd className="mt-1 break-all font-medium text-zinc-100">{pool.metadata_url ?? "--"}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Metadata Hash</dt>
                <dd className="mt-1 break-all font-medium text-zinc-100">{pool.metadata_hash ?? "--"}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Owners</dt>
                <dd className="mt-1 font-medium text-zinc-100">
                  {pool.owners.length > 0 ? pool.owners.join(", ") : "--"}
                </dd>
              </div>
              <div className="md:col-span-2 xl:col-span-2">
                <dt className="text-zinc-500">Relays</dt>
                <dd className="mt-1 font-medium text-zinc-100">
                  {pool.relays.length > 0
                    ? pool.relays
                        .map((relay) => `${relay.address}:${relay.port}`)
                        .join(", ")
                    : "--"}
                </dd>
              </div>
            </dl>
          </section>
        ) : (
          <section className="grid gap-4 lg:grid-cols-[1.4fr_1fr]">
            <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
              <div className="mb-4">
                <h2 className="text-sm font-semibold text-zinc-100">Bind Existing Pool</h2>
                <p className="mt-1 text-sm text-zinc-400">
                  This workspace has no on-chain pool binding yet. If the pool is already registered on-chain,
                  query it by `pool_id` and bind it here.
                </p>
              </div>
              <PoolRegistrationStatus
                poolTicker={pool.ticker}
                onBound={onPoolRefreshed}
                embedded
              />
            </div>
            <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
              <h2 className="text-sm font-semibold text-zinc-100">Register New Pool</h2>
              <p className="mt-2 text-sm text-zinc-400">
                If this workspace does not correspond to an existing on-chain `pool_id`, use the
                registration flow below. The hot node only prepares an unsigned transaction and submits a
                pre-signed transaction; certificate generation and signing stay in the cold environment.
              </p>
              <div className="mt-4">
                <PoolRegistrationWizard poolTicker={pool.ticker} />
              </div>
            </div>
          </section>
        )}
      </div>
      <div className="grid gap-4 lg:grid-cols-3">
        <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
          <h2 className="text-sm font-semibold text-zinc-100">Fleet Health</h2>
          <dl className="mt-3 grid grid-cols-3 gap-3 text-sm">
            <div>
              <dt className="text-zinc-500">Healthy</dt>
              <dd className="mt-1 font-medium text-emerald-300">{healthyMachines}</dd>
            </div>
            <div>
              <dt className="text-zinc-500">Warning</dt>
              <dd className="mt-1 font-medium text-amber-300">{warningMachines}</dd>
            </div>
            <div>
              <dt className="text-zinc-500">Critical</dt>
              <dd className="mt-1 font-medium text-red-300">{criticalMachines}</dd>
            </div>
          </dl>
        </div>

        <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
          <h2 className="text-sm font-semibold text-zinc-100">KES Rotation Watch</h2>
          <div className="mt-3 space-y-2">
            {kesStatuses.length === 0 ? (
              <p className="text-sm text-zinc-400">No BP KES status loaded.</p>
            ) : (
              kesStatuses.slice(0, 4).map((row) => (
                <div
                  key={row.machine_id}
                  className="flex items-center justify-between rounded-md border border-zinc-800 bg-black/20 px-3 py-2 text-sm"
                >
                  <div>
                    <p className="font-medium text-zinc-100">{row.machine_name}</p>
                    <p className="text-xs text-zinc-500">
                      {row.remaining_days ?? "--"} day(s) remaining
                    </p>
                  </div>
                  <span className={`text-xs uppercase ${severityTone(row.severity)}`}>
                    {row.severity}
                  </span>
                </div>
              ))
            )}
          </div>
        </div>

        <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
          <h2 className="text-sm font-semibold text-zinc-100">Recent Tasks</h2>
          <div className="mt-3 space-y-2">
            {recentTasks.length === 0 ? (
              <p className="text-sm text-zinc-400">No tasks recorded yet.</p>
            ) : (
              recentTasks.slice(0, 5).map((task) => {
                const taskError = formatTaskError(task.error_msg);
                return (
                  <div
                    key={task.task_id}
                    className="rounded-md border border-zinc-800 bg-black/20 px-3 py-2 text-sm"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <p className="font-medium text-zinc-100">{formatTaskLabel(task.task_type)}</p>
                      <span className={`text-xs uppercase ${taskTone(task.status)}`}>
                        {formatTaskLabel(task.status)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-zinc-500">
                      {task.phase ? `${formatTaskLabel(task.phase)} · ` : ""}
                      {task.machine_count} machine(s) · {task.created_at}
                    </p>
                    {taskError && (
                      <p
                        title={taskError}
                        className="mt-2 max-h-20 overflow-hidden rounded-md border border-red-950 bg-red-950/30 px-2 py-1 text-xs text-red-200 break-words"
                      >
                        {truncatePreview(taskError)}
                      </p>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
      <div className="rounded-md border border-zinc-800 bg-zinc-900/70 p-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-zinc-100">Sync Monitor</h2>
            <p className="text-xs text-zinc-400">{monitorStatus}</p>
          </div>
          <button
            type="button"
            onClick={() => void refreshMonitor()}
            className="rounded-md border border-zinc-700 px-3 py-2 text-xs font-medium text-zinc-100 hover:bg-zinc-800"
          >
            Refresh
          </button>
        </div>
        <div className="mt-4 grid gap-3 md:grid-cols-2">
          {snapshots.length === 0 ? (
            <div className="rounded-md border border-dashed border-zinc-700 p-4 text-sm text-zinc-400">
              No sync samples yet.
            </div>
          ) : (
            snapshots.map((snapshot) => (
              <article
                key={snapshot.machine_id}
                className="rounded-md border border-zinc-800 bg-black/20 p-4"
              >
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h3 className="text-sm font-semibold text-zinc-100">{snapshot.machine_name}</h3>
                    <p className="text-xs uppercase tracking-wide text-zinc-500">
                      {snapshot.role} · {snapshot.network}
                    </p>
                  </div>
                  <div className="flex flex-col items-end gap-1">
                    <span className={`text-[11px] font-medium uppercase ${healthTone(snapshot.health_level)}`}>
                      {snapshot.health_level}
                    </span>
                    <span className={`text-xs font-medium uppercase ${statusTone(snapshot.status)}`}>
                      {snapshot.status}
                    </span>
                    <span className={`text-[11px] font-medium uppercase ${stageTone(snapshot.sync_stage)}`}>
                      {formatStage(snapshot.sync_stage)}
                    </span>
                  </div>
                </div>
                <dl className="mt-4 grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <dt className="text-zinc-500">Sync Progress</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {formatProgress(snapshot.sync_progress)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Blocks/min</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {formatBlocksPerMinute(snapshot.blocks_per_minute)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Block Height</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {snapshot.block_height ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Collected At</dt>
                    <dd className="mt-1 font-medium text-zinc-100">{snapshot.collected_at}</dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Snapshot Restore</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {snapshot.restore_snapshot_requested ? "requested" : "not requested"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Sync Stage</dt>
                    <dd className={`mt-1 font-medium ${stageTone(snapshot.sync_stage)}`}>
                      {formatStage(snapshot.sync_stage)}
                    </dd>
                  </div>
                </dl>
                {snapshot.note && (
                  <p
                    title={snapshot.note}
                    className="mt-3 max-h-20 overflow-hidden rounded-md border border-red-950 bg-red-950/30 px-3 py-2 text-xs text-red-200 break-words"
                  >
                    {truncatePreview(snapshot.note, 220)}
                  </p>
                )}
                {snapshot.sync_stage === "snapshot_restoring" && (
                  <p className="mt-3 rounded-md border border-sky-900 bg-sky-950/30 px-3 py-2 text-xs text-sky-200">
                    Mithril snapshot restore is still initializing the database. Full chain sync has not started yet.
                  </p>
                )}
                {snapshot.sync_stage === "restore_timeout" && (
                  <p className="mt-3 rounded-md border border-red-900 bg-red-950/30 px-3 py-2 text-xs text-red-200">
                    Mithril restore has been stuck for too long. Inspect logs or allow the node to continue with regular sync.
                  </p>
                )}
                {snapshot.sync_stage === "fallback_syncing" && (
                  <p className="mt-3 rounded-md border border-orange-900 bg-orange-950/30 px-3 py-2 text-xs text-orange-200">
                    Mithril restore failed earlier, but the node is now continuing with regular sync.
                  </p>
                )}
                {snapshot.stalled && !snapshot.note && (
                  <p className="mt-3 rounded-md border border-amber-900 bg-amber-950/30 px-3 py-2 text-xs text-amber-200">
                    Sync progress is moving too slowly or has stopped for at least 5 minutes.
                  </p>
                )}
              </article>
            ))
          )}
        </div>
      </div>
      <button
        type="button"
        onClick={handleRunTest}
        className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
      >
        Run playbook test (mock)
      </button>
      {taskId && <p className="text-xs text-zinc-400">Task ID: {taskId}</p>}
      <div className="max-h-72 overflow-y-auto rounded-md border border-zinc-800 bg-black/30 p-3 font-mono text-xs">
        {events.length === 0
          ? "Events will appear here."
          : events.map((line, index) => (
              <div key={`${index}-${line.slice(0, 16)}`} className="break-all py-0.5 text-zinc-300">
                {line}
              </div>
            ))}
      </div>
    </section>
  );
}
