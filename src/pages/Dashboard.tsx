import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { dbVersion, kesStatusAll, ping, runPlaybookTest, taskRecentList } from "../lib/ipc";
import {
  refreshMonitorStore,
  startMonitorStore,
  stopMonitorStore,
  useMonitorStore,
} from "../lib/monitorStore";
import type { DbVersionResult, KesStatus, RecentTaskSummary } from "../lib/types";

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

export default function Dashboard() {
  const [status, setStatus] = useState<string>("loading");
  const [dbInfo, setDbInfo] = useState<DbVersionResult | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [events, setEvents] = useState<string[]>([]);
  const [kesStatuses, setKesStatuses] = useState<KesStatus[]>([]);
  const [recentTasks, setRecentTasks] = useState<RecentTaskSummary[]>([]);
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
        setStatus(`Error: ${String(error)}`);
      }
    })();
    return () => {
      void stopMonitorStore();
    };
  }, [refreshMonitor]);

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
      setEvents((prev) => [...prev, `run_playbook_test error: ${String(error)}`]);
    }
  };

  const healthyMachines = snapshots.filter((row) => row.health_level === "healthy").length;
  const warningMachines = snapshots.filter((row) => row.health_level === "warning").length;
  const criticalMachines = snapshots.filter((row) => row.health_level === "critical").length;

  return (
    <section className="space-y-4">
      <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
      <p className="text-sm text-zinc-300">
        <span className="font-medium text-zinc-100">Sidecar:</span> {status}
      </p>
      {dbInfo && (
        <p className="text-sm text-zinc-300">
          <span className="font-medium text-zinc-100">DB:</span> user_version={dbInfo.user_version}
        </p>
      )}
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
              recentTasks.slice(0, 5).map((task) => (
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
                  {task.error_msg && (
                    <p className="mt-2 rounded-md border border-red-950 bg-red-950/30 px-2 py-1 text-xs text-red-200">
                      {task.error_msg}
                    </p>
                  )}
                </div>
              ))
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
                  <p className="mt-3 rounded-md border border-red-950 bg-red-950/30 px-3 py-2 text-xs text-red-200">
                    {snapshot.note}
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
