import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import TaskLogStream from "../components/TaskLogStream";
import { toUserError } from "../lib/errors";
import {
  machineList,
  upgradeConfirmNext,
  upgradeRollback,
  upgradeStart,
  upgradeStatus,
} from "../lib/ipc";
import type { DeployTaskStatus, Machine, UpgradeGateEvent, UpgradePayload } from "../lib/types";

function isTerminal(status: string): boolean {
  return status === "success" || status === "failed" || status === "cancelled";
}

function formatTaskStatus(status: string): string {
  return status.split("_").join(" ");
}

function taskTone(status: string): string {
  switch (status) {
    case "success":
      return "text-emerald-700";
    case "failed":
    case "cancelled":
      return "text-red-700";
    case "paused":
      return "text-amber-700";
    case "running":
      return "text-sky-700";
    default:
      return "text-slate-600";
  }
}

function roleRank(role: Machine["role"]): number {
  switch (role) {
    case "relay":
      return 0;
    case "bp":
      return 1;
    default:
      return 2;
  }
}

function readPhase(task: DeployTaskStatus | null): string {
  const phase = task?.payload?.phase;
  return typeof phase === "string" ? phase : "--";
}

interface UpgradeWizardProps {
  poolTicker: string;
}

export default function UpgradeWizard({ poolTicker }: UpgradeWizardProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [selectedMachineIds, setSelectedMachineIds] = useState<number[]>([]);
  const [targetVersion, setTargetVersion] = useState("10.5.4-1");
  const [imageRegistry, setImageRegistry] = useState("ghcr.io/blinklabs-io/cardano-node");
  const [imageDigest, setImageDigest] = useState("");
  const [autoContinue, setAutoContinue] = useState(false);
  const [starting, setStarting] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [rollbackMachineId, setRollbackMachineId] = useState<number | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [taskStatus, setTaskStatus] = useState<DeployTaskStatus | null>(null);
  const [gateEvent, setGateEvent] = useState<UpgradeGateEvent | null>(null);
  const [rollbackNotice, setRollbackNotice] = useState<string | null>(null);
  const [bpConfirmValue, setBpConfirmValue] = useState("");

  const loadMachines = async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await machineList();
      const upgradeMachines = rows
        .filter((machine) => machine.role === "relay" || machine.role === "bp")
        .sort((left, right) => {
          const roleDiff = roleRank(left.role) - roleRank(right.role);
          if (roleDiff !== 0) {
            return roleDiff;
          }
          return left.name.localeCompare(right.name);
        });
      setMachines(upgradeMachines);
      setSelectedMachineIds((prev) => {
        if (prev.length > 0) {
          return prev.filter((id) => upgradeMachines.some((machine) => machine.id === id));
        }
        return upgradeMachines.map((machine) => machine.id);
      });
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadMachines();
  }, []);

  useEffect(() => {
    if (!taskId || !taskStatus || isTerminal(taskStatus.status)) {
      return;
    }
    let active = true;
    const timer = window.setInterval(() => {
      void upgradeStatus(taskId)
        .then((next) => {
          if (!active) {
            return;
          }
          setTaskStatus(next);
          if (next.status !== "paused") {
            setGateEvent((current) => (current?.task_id === next.task_id ? null : current));
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
  }, [taskId, taskStatus]);

  useEffect(() => {
    let active = true;
    const unlistenPromise = listen<UpgradeGateEvent>("upgrade:gate", (event) => {
      if (!active) {
        return;
      }
      setGateEvent(event.payload);
      setBpConfirmValue("");
    });
    return () => {
      active = false;
      void unlistenPromise.then((unlisten) => {
        unlisten();
      });
    };
  }, []);

  const selectedMachines = useMemo(
    () => machines.filter((machine) => selectedMachineIds.includes(machine.id)),
    [machines, selectedMachineIds],
  );
  const normalizedTicker = poolTicker.trim();
  const bpGateUnlocked = bpConfirmValue.trim() === normalizedTicker;
  const wizardStep = useMemo(() => {
    if (!taskId) {
      return 1;
    }
    if (taskStatus && isTerminal(taskStatus.status)) {
      return 3;
    }
    return 2;
  }, [taskId, taskStatus]);

  const machineStatusMap = useMemo(() => {
    const entries =
      taskStatus?.machine_statuses.map((row) => [row.machine_id, row.status] as const) ?? [];
    return Object.fromEntries(entries);
  }, [taskStatus]);

  const toggleMachine = (machineId: number) => {
    setSelectedMachineIds((prev) =>
      prev.includes(machineId) ? prev.filter((id) => id !== machineId) : [...prev, machineId],
    );
  };

  const handleStart = async () => {
    if (selectedMachineIds.length === 0) {
      setError("Select at least one relay or BP machine.");
      return;
    }
    setStarting(true);
    setError(null);
    setRollbackNotice(null);
    setGateEvent(null);
    try {
      const payload: UpgradePayload = {
        target_version: targetVersion.trim(),
        image_registry: imageRegistry.trim(),
        image_digest: imageDigest.trim() || undefined,
        machine_ids: selectedMachineIds,
        auto_continue: autoContinue,
      };
      const nextTaskId = await upgradeStart(payload);
      const status = await upgradeStatus(nextTaskId);
      setTaskId(nextTaskId);
      setTaskStatus(status);
      setBpConfirmValue("");
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setStarting(false);
    }
  };

  const handleConfirmNext = async () => {
    if (!taskId) {
      return;
    }
    setConfirming(true);
    setError(null);
    try {
      await upgradeConfirmNext(taskId);
      const next = await upgradeStatus(taskId);
      setTaskStatus(next);
      setGateEvent(null);
      setBpConfirmValue("");
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setConfirming(false);
    }
  };

  const handleRollback = async (machineId: number) => {
    if (!taskId) {
      return;
    }
    setRollbackMachineId(machineId);
    setError(null);
    setRollbackNotice(null);
    try {
      const rollbackTaskId = await upgradeRollback(taskId, machineId);
      const machineName =
        machines.find((machine) => machine.id === machineId)?.name ?? `machine-${machineId}`;
      setRollbackNotice(`Rollback started for ${machineName}. task_id=${rollbackTaskId}`);
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setRollbackMachineId(null);
    }
  };

  return (
    <section className="space-y-5">
      <header className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">Upgrade Wizard</h1>
        <p className="text-sm text-zinc-400">
          Run relay-first rolling upgrades, then confirm the BP cutover when the gate event arrives.
        </p>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span
            className={`rounded-full border px-2.5 py-1 ${
              wizardStep === 1
                ? "border-blue-300 bg-blue-50 font-semibold text-blue-700"
                : "border-slate-300 bg-slate-50 text-slate-600"
            }`}
          >
            1 Version Confirm
          </span>
          <span
            className={`rounded-full border px-2.5 py-1 ${
              wizardStep === 2
                ? "border-blue-300 bg-blue-50 font-semibold text-blue-700"
                : "border-slate-300 bg-slate-50 text-slate-600"
            }`}
          >
            2 Rolling Upgrade
          </span>
          <span
            className={`rounded-full border px-2.5 py-1 ${
              wizardStep === 3
                ? "border-blue-300 bg-blue-50 font-semibold text-blue-700"
                : "border-slate-300 bg-slate-50 text-slate-600"
            }`}
          >
            3 Health Check
          </span>
        </div>
      </header>

      {error && (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
          {error}
        </p>
      )}

      {rollbackNotice && (
        <p className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-700">
          {rollbackNotice}
        </p>
      )}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.9fr)]">
        <div className="space-y-6">
          <section className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h2 className="text-base font-semibold">Step 1: Upgrade plan</h2>
                <p className="mt-1 text-sm text-slate-600">
                  Relay machines are upgraded first. BP waits for an explicit gate confirmation.
                </p>
              </div>
              <button
                type="button"
                onClick={() => void loadMachines()}
                className="rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-100"
              >
                Refresh Machines
              </button>
            </div>

            <div className="mt-4 grid gap-4 md:grid-cols-2">
              <label className="text-sm">
                <span className="mb-1 block text-slate-600">Target Version</span>
                <input
                  value={targetVersion}
                  onChange={(event) => setTargetVersion(event.target.value)}
                  className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900"
                />
              </label>
              <label className="text-sm">
                <span className="mb-1 block text-slate-600">Image Registry</span>
                <input
                  value={imageRegistry}
                  onChange={(event) => setImageRegistry(event.target.value)}
                  className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900"
                />
              </label>
              <label className="text-sm md:col-span-2">
                <span className="mb-1 block text-slate-600">Image Digest (optional)</span>
                <input
                  value={imageDigest}
                  onChange={(event) => setImageDigest(event.target.value)}
                  placeholder="sha256:..."
                  className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900"
                />
              </label>
            </div>

            <label className="mt-4 flex items-center gap-3 rounded-md border border-slate-200 bg-white px-3 py-3 text-sm text-slate-700">
              <input
                type="checkbox"
                checked={autoContinue}
                onChange={(event) => setAutoContinue(event.target.checked)}
                className="h-4 w-4 rounded border-slate-300 bg-white text-sky-600"
              />
              <span>Auto-continue relay sequence without waiting at each relay gate.</span>
            </label>

            <div className="mt-4 rounded-md border border-amber-200 bg-amber-50 px-3 py-3 text-sm text-amber-700">
              This flow is disruptive. Relay hosts are upgraded in order; BP cutover requires a
              separate confirmation gate.
            </div>

            <div className="mt-4">
              <h3 className="text-sm font-medium">Selected machines</h3>
              {loading ? (
                <p className="mt-2 text-sm text-slate-500">Loading upgrade candidates...</p>
              ) : machines.length === 0 ? (
                <p className="mt-2 text-sm text-slate-500">No relay or BP machines found.</p>
              ) : (
                <div className="mt-3 grid gap-3">
                  {machines.map((machine) => {
                    const checked = selectedMachineIds.includes(machine.id);
                    return (
                      <label
                        key={machine.id}
                        className="flex items-center justify-between rounded-md border border-slate-200 bg-white px-3 py-3 text-sm"
                      >
                        <span className="flex items-center gap-3">
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => toggleMachine(machine.id)}
                            className="h-4 w-4 rounded border-slate-300 bg-white text-sky-600"
                          />
                          <span>
                            <span className="block font-medium text-slate-900">{machine.name}</span>
                            <span className="text-slate-500">
                              {machine.role} · {machine.ip} · {machine.cardano_version ?? "--"}
                            </span>
                          </span>
                        </span>
                        <span className="text-xs uppercase tracking-wide text-slate-500">
                          {machine.role}
                        </span>
                      </label>
                    );
                  })}
                </div>
              )}
            </div>

            <div className="mt-4 flex items-center gap-3">
              <button
                type="button"
                onClick={() => void handleStart()}
                disabled={starting || loading || selectedMachineIds.length === 0}
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
              >
                {starting ? "Starting Upgrade..." : "Start Upgrade"}
              </button>
              {taskId && <span className="text-xs text-slate-500">task_id={taskId}</span>}
            </div>
          </section>

          {taskId && <TaskLogStream taskId={taskId} />}
        </div>

        <aside className="space-y-6">
          <section className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
            <h2 className="text-base font-semibold">Step 2: Task status</h2>
            {taskStatus ? (
              <div className="mt-4 space-y-4 text-sm">
                <dl className="grid gap-3">
                  <div>
                    <dt className="text-slate-500">Status</dt>
                    <dd className={`mt-1 font-medium uppercase ${taskTone(taskStatus.status)}`}>
                      {formatTaskStatus(taskStatus.status)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-slate-500">Phase</dt>
                    <dd className="mt-1 font-medium text-slate-900">{readPhase(taskStatus)}</dd>
                  </div>
                  <div>
                    <dt className="text-slate-500">Created</dt>
                    <dd className="mt-1 font-medium text-slate-900">{taskStatus.created_at}</dd>
                  </div>
                </dl>

                <div>
                  <h3 className="text-sm font-medium">Machine progress</h3>
                  <div className="mt-3 space-y-2">
                    {selectedMachines.map((machine) => {
                      const status = machineStatusMap[machine.id] ?? "pending";
                      return (
                        <div
                          key={machine.id}
                          className="flex items-center justify-between rounded-md border border-slate-200 bg-white px-3 py-2"
                        >
                          <div>
                            <p className="font-medium text-slate-900">{machine.name}</p>
                            <p className="text-xs uppercase tracking-wide text-slate-500">
                              {machine.role}
                            </p>
                          </div>
                          <div className="flex items-center gap-3">
                            <span className={`text-xs uppercase ${taskTone(status)}`}>
                              {formatTaskStatus(status)}
                            </span>
                            {(status === "running" || status === "success" || status === "failed") && (
                              <button
                                type="button"
                                onClick={() => void handleRollback(machine.id)}
                                disabled={rollbackMachineId === machine.id}
                                className="rounded-md border border-slate-300 bg-white px-2 py-1 text-xs text-slate-700 hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
                              >
                                {rollbackMachineId === machine.id ? "Rolling Back..." : "Rollback"}
                              </button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              </div>
            ) : (
              <p className="mt-3 text-sm text-slate-500">No upgrade task started yet.</p>
            )}
          </section>

          <section className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
            <h2 className="text-base font-semibold">Step 2: BP gate & rollback</h2>
            {gateEvent && gateEvent.task_id === taskId ? (
              <div className="mt-4 space-y-4 text-sm">
                <p className="rounded-md border border-amber-200 bg-amber-50 px-3 py-3 text-amber-700">
                  {gateEvent.message}
                </p>
                <dl className="grid gap-3">
                  <div>
                    <dt className="text-slate-500">Completed Machine</dt>
                    <dd className="mt-1 font-medium text-slate-900">{gateEvent.completed_machine}</dd>
                  </div>
                  <div>
                    <dt className="text-slate-500">Next Machine</dt>
                    <dd className="mt-1 font-medium text-slate-900">{gateEvent.next_machine}</dd>
                  </div>
                </dl>
                {gateEvent.is_bp && (
                  <div className="rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-700">
                    <p className="font-medium">Type pool ticker {normalizedTicker} to unlock BP upgrade.</p>
                    <input
                      value={bpConfirmValue}
                      onChange={(event) => setBpConfirmValue(event.target.value)}
                      autoCapitalize="none"
                      autoCorrect="off"
                      className="mt-3 w-full rounded-md border border-red-300 bg-white px-3 py-2 text-sm text-slate-900"
                    />
                  </div>
                )}
                <button
                  type="button"
                  onClick={() => void handleConfirmNext()}
                  disabled={confirming || (gateEvent.is_bp && !bpGateUnlocked)}
                  className="rounded-md bg-amber-500 px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {confirming ? "Confirming..." : "Confirm Next Step"}
                </button>
              </div>
            ) : (
              <p className="mt-3 text-sm text-slate-500">
                Waiting for an `upgrade:gate` event. Relay auto-continue can skip intermediate relay
                gates, but BP still pauses for manual confirmation.
              </p>
            )}
          </section>
        </aside>
      </div>
    </section>
  );
}
