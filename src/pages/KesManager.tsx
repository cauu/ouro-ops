import { useEffect, useMemo, useState } from "react";
import { formatTaskError, toUserError } from "../lib/errors";
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
      return "text-emerald-700";
    case "critical":
      return "text-red-700";
    default:
      return "text-amber-700";
  }
}

function taskTone(status: string): string {
  switch (status) {
    case "success":
      return "text-emerald-700";
    case "failed":
    case "cancelled":
      return "text-red-700";
    case "running":
      return "text-sky-700";
    default:
      return "text-amber-700";
  }
}

function formatTaskLabel(status: string): string {
  return status.split("_").join(" ");
}

interface KesManagerProps {
  poolTicker: string;
}

export default function KesManager({ poolTicker }: KesManagerProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<KesStatus[]>([]);
  const [requests, setRequests] = useState<Record<number, KesSignRequest>>({});
  const [certPaths, setCertPaths] = useState<Record<number, string>>({});
  const [rotationTasks, setRotationTasks] = useState<Record<number, DeployTaskStatus>>({});
  const [busyMachineId, setBusyMachineId] = useState<number | null>(null);
  const [pushConfirmMachineId, setPushConfirmMachineId] = useState<number | null>(null);
  const [pushConfirmValue, setPushConfirmValue] = useState("");

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
      setError(toUserError(e));
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
            setError(toUserError(e));
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
      setError(toUserError(e));
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
      setError(toUserError(e));
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
      setError(toUserError(e));
    } finally {
      setBusyMachineId(null);
    }
  };

  const normalizedTicker = poolTicker.trim();

  return (
    <section className="space-y-5">
      <header className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">KES Rotate</h1>
        <p className="text-sm text-zinc-400">
          Step 1-4 向导：生成 keypairs、离线签发 cert、执行 rotate、完成校验。
        </p>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="rounded-full border border-sky-300 bg-sky-50 px-2.5 py-1 font-semibold text-sky-700">
            1 Generate KES
          </span>
          <span className="rounded-full border border-slate-300 bg-slate-50 px-2.5 py-1 text-slate-600">
            2 Offline Cert
          </span>
          <span className="rounded-full border border-slate-300 bg-slate-50 px-2.5 py-1 text-slate-600">
            3 Rotate Gate
          </span>
          <span className="rounded-full border border-slate-300 bg-slate-50 px-2.5 py-1 text-slate-600">
            4 Validate
          </span>
        </div>
      </header>

      {error && (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
          {error}
        </p>
      )}

      {loading ? (
        <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-sm text-slate-600">
          <p className="font-medium text-slate-900">Loading KES status...</p>
        </div>
      ) : sortedStatuses.length === 0 ? (
        <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-sm text-slate-500">
          No BP machines found.
        </div>
      ) : (
        <div className="grid gap-4">
          {sortedStatuses.map((status) => {
            const request = requests[status.machine_id];
            const task = rotationTasks[status.machine_id];
            const busy = busyMachineId === status.machine_id;
            const canPush = task?.status === "pending";
            const pushConfirmArmed = pushConfirmMachineId === status.machine_id;
            const pushUnlocked = pushConfirmValue.trim() === normalizedTicker;
            const taskError = formatTaskError(task?.error_msg);
            const step4Ready = task?.status === "success";
            return (
              <article
                key={status.machine_id}
                className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm"
              >
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h2 className="text-base font-semibold">{status.machine_name}</h2>
                    <p className="mt-1 text-xs uppercase tracking-wide text-slate-500">
                      block producer
                    </p>
                  </div>
                  <span className={`text-xs font-medium uppercase ${severityTone(status.severity)}`}>
                    {status.severity}
                  </span>
                </div>

                <dl className="mt-4 grid gap-3 text-sm md:grid-cols-4">
                  <div>
                    <dt className="text-slate-500">Current Period</dt>
                    <dd className="mt-1 font-semibold">
                      {status.kes_period_current ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-slate-500">Max Period</dt>
                    <dd className="mt-1 font-semibold">
                      {status.kes_period_max ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-slate-500">Remaining Days</dt>
                    <dd className="mt-1 font-semibold">
                      {status.remaining_days ?? "--"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-slate-500">Op Cert Counter</dt>
                    <dd className="mt-1 font-semibold">
                      {status.op_cert_counter ?? "--"}
                    </dd>
                  </div>
                </dl>

                {status.expiry_date && (
                  <p className="mt-3 text-xs text-slate-500">Expiry date: {status.expiry_date}</p>
                )}

                <div className="mt-4 rounded-md border border-slate-200 bg-white p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium">Step 1: Generate KES request</p>
                      <p className="mt-1 text-xs text-slate-500">
                        Create a new local KES keypair and copy the generated verification key to the cold environment.
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => void handleGenerate(status.machine_id)}
                      disabled={busy}
                      className="rounded-md border border-blue-300 bg-blue-50 px-3 py-1.5 text-xs font-semibold text-blue-700 hover:bg-blue-100 disabled:opacity-60"
                    >
                      Generate KES
                    </button>
                  </div>
                  {request && (
                    <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-3 text-xs text-slate-700">
                      <p className="font-medium text-slate-900">KES verification key</p>
                      <p className="mt-1 break-all">{request.kes_vkey_path}</p>
                      <p className="mt-3 font-medium text-slate-900">Step 2: Cold signing instructions</p>
                      <pre className="mt-1 whitespace-pre-wrap text-slate-700">{request.instructions}</pre>
                    </div>
                  )}
                </div>

                <div className="mt-4 rounded-md border border-slate-200 bg-white p-3">
                  <p className="text-sm font-medium">Step 3: Import signed certificate</p>
                  <p className="mt-1 text-xs text-slate-500">
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
                      className="flex-1 rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900"
                    />
                    <button
                      type="button"
                      onClick={() => void handleImport(status.machine_id)}
                      disabled={busy}
                      className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm font-semibold text-amber-700 hover:bg-amber-100 disabled:opacity-60"
                    >
                      Import Cert
                    </button>
                  </div>
                </div>

                <div className="mt-4 rounded-md border border-slate-200 bg-white p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium">Step 3: Push to BP (Risk Gate)</p>
                      <p className="mt-1 text-xs text-slate-500">
                        Install the staged certificate on the BP and restart the runtime with readiness checks.
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        setPushConfirmMachineId(status.machine_id);
                        setPushConfirmValue("");
                      }}
                      disabled={busy || !canPush}
                      className="rounded-md border border-emerald-300 bg-emerald-50 px-3 py-1.5 text-xs font-semibold text-emerald-700 hover:bg-emerald-100 disabled:opacity-60"
                    >
                      Push to BP
                    </button>
                  </div>
                  {canPush && pushConfirmArmed && (
                    <div className="mt-3 rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-700">
                      <p className="font-medium">Type pool ticker {normalizedTicker} to unlock KES push.</p>
                      <div className="mt-3 flex flex-col gap-2 md:flex-row">
                        <input
                          value={pushConfirmValue}
                          onChange={(event) => setPushConfirmValue(event.target.value)}
                          autoCapitalize="none"
                          autoCorrect="off"
                          className="flex-1 rounded-md border border-red-300 bg-white px-3 py-2 text-sm text-slate-900"
                        />
                        <div className="flex gap-2">
                          <button
                            type="button"
                            onClick={() => void handlePush(status.machine_id)}
                            disabled={busy || !pushUnlocked}
                            className="rounded-md bg-red-500 px-3 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
                          >
                            Confirm KES Push
                          </button>
                          <button
                            type="button"
                            onClick={() => {
                              setPushConfirmMachineId(null);
                              setPushConfirmValue("");
                            }}
                            className="rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-100"
                          >
                            Cancel
                          </button>
                        </div>
                      </div>
                    </div>
                  )}
                  {task && (
                    <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-3 text-xs text-slate-700">
                      <p className={`font-medium uppercase ${taskTone(task.status)}`}>
                        Task {formatTaskLabel(task.status)}
                      </p>
                      <p className="mt-1 break-all text-slate-500">Task ID: {task.task_id}</p>
                      {taskError && (
                        <p className="mt-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-red-700">
                          {taskError}
                        </p>
                      )}
                    </div>
                  )}
                </div>

                <div className="mt-4 rounded-md border border-slate-200 bg-white p-3">
                  <p className="text-sm font-medium">Step 4: Validation</p>
                  <div className="mt-2 grid gap-2 text-xs text-slate-600 md:grid-cols-3">
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                      <span className="block text-slate-500">Operation</span>
                      <strong className="text-slate-900">
                        {step4Ready ? "success" : task ? formatTaskLabel(task.status) : "pending"}
                      </strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                      <span className="block text-slate-500">KES remain</span>
                      <strong className="text-slate-900">{status.remaining_days ?? "--"}d</strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                      <span className="block text-slate-500">BP health</span>
                      <strong className="text-slate-900">{status.severity === "critical" ? "risk" : "online"}</strong>
                    </div>
                  </div>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
