import { useEffect, useMemo, useState } from "react";
import {
  kesGenerate,
  kesImportCert,
  kesPushStart,
  kesRotationStatus,
  kesStatusAll,
} from "../lib/ipc";
import type { DeployTaskStatus, KesSignRequest, KesStatus } from "../lib/types";

function isTerminal(status: string): boolean {
  return status === "success" || status === "failed" || status === "cancelled";
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
    case "running":
      return "text-sky-300";
    default:
      return "text-amber-300";
  }
}

function formatTaskLabel(status: string): string {
  return status.split("_").join(" ");
}

export default function KesManager() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<KesStatus[]>([]);
  const [requests, setRequests] = useState<Record<number, KesSignRequest>>({});
  const [certPaths, setCertPaths] = useState<Record<number, string>>({});
  const [rotationTasks, setRotationTasks] = useState<Record<number, DeployTaskStatus>>({});
  const [busyMachineId, setBusyMachineId] = useState<number | null>(null);

  const loadKes = async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await kesStatusAll();
      setStatuses(rows);
      setCertPaths((prev) => {
        const next = { ...prev };
        rows.forEach((row) => {
          if (!(row.machine_id in next)) {
            next[row.machine_id] = "";
          }
        });
        return next;
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadKes();
  }, []);

  useEffect(() => {
    const activeTasks = Object.entries(rotationTasks).filter(([, task]) => !isTerminal(task.status));
    if (activeTasks.length === 0) {
      return;
    }
    let active = true;
    const timer = window.setInterval(() => {
      void Promise.all(
        activeTasks.map(async ([machineId, task]) => {
          const next = await kesRotationStatus(task.task_id);
          return [Number(machineId), next] as const;
        }),
      )
        .then((rows) => {
          if (!active) {
            return;
          }
          setRotationTasks((prev) => {
            const next = { ...prev };
            rows.forEach(([machineId, task]) => {
              next[machineId] = task;
            });
            return next;
          });
          if (rows.some(([, task]) => task.status === "success")) {
            void loadKes();
          }
        })
        .catch((e) => {
          if (active) {
            setError(String(e));
          }
        });
    }, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [rotationTasks]);

  const sortedStatuses = useMemo(
    () => [...statuses].sort((a, b) => a.machine_name.localeCompare(b.machine_name)),
    [statuses],
  );

  const handleGenerate = async (machineId: number) => {
    setBusyMachineId(machineId);
    setError(null);
    try {
      const request = await kesGenerate(machineId);
      setRequests((prev) => ({ ...prev, [machineId]: request }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMachineId(null);
    }
  };

  const handleImport = async (machineId: number) => {
    setBusyMachineId(machineId);
    setError(null);
    try {
      const certPath = certPaths[machineId]?.trim();
      if (!certPath) {
        throw new Error("Certificate path is required.");
      }
      const taskId = await kesImportCert(machineId, certPath);
      const task = await kesRotationStatus(taskId);
      setRotationTasks((prev) => ({ ...prev, [machineId]: task }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMachineId(null);
    }
  };

  const handlePush = async (machineId: number) => {
    const task = rotationTasks[machineId];
    if (!task) {
      return;
    }
    setBusyMachineId(machineId);
    setError(null);
    try {
      await kesPushStart(task.task_id);
      const next = await kesRotationStatus(task.task_id);
      setRotationTasks((prev) => ({ ...prev, [machineId]: next }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMachineId(null);
    }
  };

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">KES Manager</h1>
        <p className="mt-1 text-sm text-zinc-400">
          Generate a new KES key, import a signed operational certificate, then push it to the BP.
        </p>
      </header>

      {error && (
        <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      )}

      {loading ? (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4 text-sm text-zinc-300">
          <p className="font-medium text-zinc-100">Loading KES status...</p>
        </div>
      ) : sortedStatuses.length === 0 ? (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4 text-sm text-zinc-400">
          No BP machines found.
        </div>
      ) : (
        <div className="grid gap-4">
          {sortedStatuses.map((status) => {
            const request = requests[status.machine_id];
            const task = rotationTasks[status.machine_id];
            const busy = busyMachineId === status.machine_id;
            const canPush = task?.status === "pending";
            return (
              <article
                key={status.machine_id}
                className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4"
              >
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h2 className="text-base font-semibold text-zinc-100">{status.machine_name}</h2>
                    <p className="mt-1 text-xs uppercase tracking-wide text-zinc-500">
                      block producer
                    </p>
                  </div>
                  <span className={`text-xs font-medium uppercase ${severityTone(status.severity)}`}>
                    {status.severity}
                  </span>
                </div>

                <dl className="mt-4 grid gap-3 md:grid-cols-4 text-sm">
                  <div>
                    <dt className="text-zinc-500">Current Period</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {status.kes_period_current ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Max Period</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {status.kes_period_max ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Remaining Days</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {status.remaining_days ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Op Cert Counter</dt>
                    <dd className="mt-1 font-medium text-zinc-100">
                      {status.op_cert_counter ?? "--"}
                    </dd>
                  </div>
                </dl>

                {status.expiry_date && (
                  <p className="mt-3 text-xs text-zinc-400">Expiry date: {status.expiry_date}</p>
                )}

                <div className="mt-4 rounded-md border border-zinc-800 bg-black/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium text-zinc-100">Step 1: Generate KES request</p>
                      <p className="mt-1 text-xs text-zinc-400">
                        Create a new local KES keypair and copy the generated verification key to the cold environment.
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => void handleGenerate(status.machine_id)}
                      disabled={busy}
                      className="rounded-md border border-sky-700/70 px-3 py-1.5 text-xs text-sky-200 hover:bg-sky-950/30 disabled:opacity-60"
                    >
                      Generate KES
                    </button>
                  </div>
                  {request && (
                    <div className="mt-3 rounded-md border border-zinc-700 bg-zinc-950/60 p-3 text-xs text-zinc-300">
                      <p className="font-medium text-zinc-100">KES verification key</p>
                      <p className="mt-1 break-all">{request.kes_vkey_path}</p>
                      <p className="mt-3 font-medium text-zinc-100">Cold signing instructions</p>
                      <pre className="mt-1 whitespace-pre-wrap text-zinc-300">{request.instructions}</pre>
                    </div>
                  )}
                </div>

                <div className="mt-4 rounded-md border border-zinc-800 bg-black/20 p-3">
                  <p className="text-sm font-medium text-zinc-100">Step 2: Import signed certificate</p>
                  <p className="mt-1 text-xs text-zinc-400">
                    Paste the absolute path of the signed `node.cert` returned by the cold environment.
                  </p>
                  <div className="mt-3 flex flex-col gap-2 md:flex-row">
                    <input
                      value={certPaths[status.machine_id] ?? ""}
                      onChange={(event) =>
                        setCertPaths((prev) => ({
                          ...prev,
                          [status.machine_id]: event.target.value,
                        }))
                      }
                      placeholder="/absolute/path/to/node.cert"
                      autoCapitalize="none"
                      autoCorrect="off"
                      className="flex-1 rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    />
                    <button
                      type="button"
                      onClick={() => void handleImport(status.machine_id)}
                      disabled={busy}
                      className="rounded-md border border-amber-700/70 px-3 py-2 text-sm text-amber-200 hover:bg-amber-950/30 disabled:opacity-60"
                    >
                      Import Cert
                    </button>
                  </div>
                </div>

                <div className="mt-4 rounded-md border border-zinc-800 bg-black/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium text-zinc-100">Step 3: Push to BP</p>
                      <p className="mt-1 text-xs text-zinc-400">
                        Install the staged certificate on the BP and restart the runtime with readiness checks.
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => void handlePush(status.machine_id)}
                      disabled={busy || !canPush}
                      className="rounded-md border border-emerald-700/70 px-3 py-1.5 text-xs text-emerald-200 hover:bg-emerald-950/30 disabled:opacity-60"
                    >
                      Push to BP
                    </button>
                  </div>
                  {task && (
                    <div className="mt-3 rounded-md border border-zinc-700 bg-zinc-950/60 p-3 text-xs text-zinc-300">
                      <p className={`font-medium uppercase ${taskTone(task.status)}`}>
                        Task {formatTaskLabel(task.status)}
                      </p>
                      <p className="mt-1 break-all text-zinc-400">Task ID: {task.task_id}</p>
                      {task.error_msg && (
                        <p className="mt-2 rounded-md border border-red-900 bg-red-950/30 px-3 py-2 text-red-200">
                          {task.error_msg}
                        </p>
                      )}
                    </div>
                  )}
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
