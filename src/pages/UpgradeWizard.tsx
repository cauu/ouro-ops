import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import TaskLogStream from "../components/TaskLogStream";
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
      setError(String(e));
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
            setError(String(e));
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
      setError(String(e));
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
      setError(String(e));
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
      setError(String(e));
    } finally {
      setRollbackMachineId(null);
    }
  };

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Upgrade Wizard</h1>
        <p className="mt-1 text-sm text-zinc-400">
          Run relay-first rolling upgrades, then confirm the BP cutover when the gate event arrives.
        </p>
      </header>

      {error && (
        <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      )}

      {rollbackNotice && (
        <p className="rounded-md border border-amber-700/60 bg-amber-900/20 px-3 py-2 text-sm text-amber-200">
          {rollbackNotice}
        </p>
      )}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.9fr)]">
        <div className="space-y-6">
          <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h2 className="text-base font-semibold text-zinc-100">Upgrade plan</h2>
                <p className="mt-1 text-sm text-zinc-400">
                  Relay machines are upgraded first. BP waits for an explicit gate confirmation.
                </p>
              </div>
              <button
                type="button"
                onClick={() => void loadMachines()}
                className="rounded-md border border-zinc-700 px-3 py-2 text-sm text-zinc-200 hover:border-zinc-500"
              >
                Refresh Machines
              </button>
            </div>

            <div className="mt-4 grid gap-4 md:grid-cols-2">
              <label className="text-sm">
                <span className="mb-1 block text-zinc-400">Target Version</span>
                <input
                  value={targetVersion}
                  onChange={(event) => setTargetVersion(event.target.value)}
                  className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
                />
              </label>
              <label className="text-sm">
                <span className="mb-1 block text-zinc-400">Image Registry</span>
                <input
                  value={imageRegistry}
                  onChange={(event) => setImageRegistry(event.target.value)}
                  className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
                />
              </label>
              <label className="text-sm md:col-span-2">
                <span className="mb-1 block text-zinc-400">Image Digest (optional)</span>
                <input
                  value={imageDigest}
                  onChange={(event) => setImageDigest(event.target.value)}
                  placeholder="sha256:..."
                  className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
                />
              </label>
            </div>

            <label className="mt-4 flex items-center gap-3 rounded-md border border-zinc-800 bg-zinc-950/60 px-3 py-3 text-sm text-zinc-200">
              <input
                type="checkbox"
                checked={autoContinue}
                onChange={(event) => setAutoContinue(event.target.checked)}
                className="h-4 w-4 rounded border-zinc-600 bg-zinc-900 text-sky-500"
              />
              <span>Auto-continue relay sequence without waiting at each relay gate.</span>
            </label>

            <div className="mt-4 rounded-md border border-amber-700/50 bg-amber-950/20 px-3 py-3 text-sm text-amber-200">
              This flow is disruptive. Relay hosts are upgraded in order; BP cutover requires a
              separate confirmation gate.
            </div>

            <div className="mt-4">
              <h3 className="text-sm font-medium text-zinc-100">Selected machines</h3>
              {loading ? (
                <p className="mt-2 text-sm text-zinc-400">Loading upgrade candidates...</p>
              ) : machines.length === 0 ? (
                <p className="mt-2 text-sm text-zinc-400">No relay or BP machines found.</p>
              ) : (
                <div className="mt-3 grid gap-3">
                  {machines.map((machine) => {
                    const checked = selectedMachineIds.includes(machine.id);
                    return (
                      <label
                        key={machine.id}
                        className="flex items-center justify-between rounded-md border border-zinc-800 bg-zinc-950/60 px-3 py-3 text-sm"
                      >
                        <span className="flex items-center gap-3">
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => toggleMachine(machine.id)}
                            className="h-4 w-4 rounded border-zinc-600 bg-zinc-900 text-sky-500"
                          />
                          <span>
                            <span className="block font-medium text-zinc-100">{machine.name}</span>
                            <span className="text-zinc-400">
                              {machine.role} · {machine.ip} · {machine.cardano_version ?? "--"}
                            </span>
                          </span>
                        </span>
                        <span className="text-xs uppercase tracking-wide text-zinc-500">
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
                className="rounded-md bg-sky-500 px-4 py-2 text-sm font-medium text-zinc-950 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {starting ? "Starting Upgrade..." : "Start Upgrade"}
              </button>
              {taskId && <span className="text-xs text-zinc-500">task_id={taskId}</span>}
            </div>
          </section>

          {taskId && <TaskLogStream taskId={taskId} />}
        </div>

        <aside className="space-y-6">
          <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
            <h2 className="text-base font-semibold text-zinc-100">Task status</h2>
            {taskStatus ? (
              <div className="mt-4 space-y-4 text-sm">
                <dl className="grid gap-3">
                  <div>
                    <dt className="text-zinc-500">Status</dt>
                    <dd className={`mt-1 font-medium uppercase ${taskTone(taskStatus.status)}`}>
                      {formatTaskStatus(taskStatus.status)}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Phase</dt>
                    <dd className="mt-1 font-medium text-zinc-100">{readPhase(taskStatus)}</dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Created</dt>
                    <dd className="mt-1 font-medium text-zinc-100">{taskStatus.created_at}</dd>
                  </div>
                </dl>

                <div>
                  <h3 className="text-sm font-medium text-zinc-100">Machine progress</h3>
                  <div className="mt-3 space-y-2">
                    {selectedMachines.map((machine) => {
                      const status = machineStatusMap[machine.id] ?? "pending";
                      return (
                        <div
                          key={machine.id}
                          className="flex items-center justify-between rounded-md border border-zinc-800 bg-zinc-950/60 px-3 py-2"
                        >
                          <div>
                            <p className="font-medium text-zinc-100">{machine.name}</p>
                            <p className="text-xs uppercase tracking-wide text-zinc-500">
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
                                className="rounded-md border border-zinc-700 px-2 py-1 text-xs text-zinc-200 hover:border-zinc-500 disabled:cursor-not-allowed disabled:opacity-60"
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
              <p className="mt-3 text-sm text-zinc-400">No upgrade task started yet.</p>
            )}
          </section>

          <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
            <h2 className="text-base font-semibold text-zinc-100">Upgrade gate</h2>
            {gateEvent && gateEvent.task_id === taskId ? (
              <div className="mt-4 space-y-4 text-sm">
                <p className="rounded-md border border-amber-700/60 bg-amber-900/20 px-3 py-3 text-amber-200">
                  {gateEvent.message}
                </p>
                <dl className="grid gap-3">
                  <div>
                    <dt className="text-zinc-500">Completed Machine</dt>
                    <dd className="mt-1 font-medium text-zinc-100">{gateEvent.completed_machine}</dd>
                  </div>
                  <div>
                    <dt className="text-zinc-500">Next Machine</dt>
                    <dd className="mt-1 font-medium text-zinc-100">{gateEvent.next_machine}</dd>
                  </div>
                </dl>
                {gateEvent.is_bp && (
                  <div className="rounded-md border border-red-800/60 bg-red-950/20 p-3 text-xs text-red-100">
                    <p className="font-medium">Type pool ticker {normalizedTicker} to unlock BP upgrade.</p>
                    <input
                      value={bpConfirmValue}
                      onChange={(event) => setBpConfirmValue(event.target.value)}
                      autoCapitalize="none"
                      autoCorrect="off"
                      className="mt-3 w-full rounded-md border border-red-800/70 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    />
                  </div>
                )}
                <button
                  type="button"
                  onClick={() => void handleConfirmNext()}
                  disabled={confirming || (gateEvent.is_bp && !bpGateUnlocked)}
                  className="rounded-md bg-amber-400 px-4 py-2 text-sm font-medium text-zinc-950 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {confirming ? "Confirming..." : "Confirm Next Step"}
                </button>
              </div>
            ) : (
              <p className="mt-3 text-sm text-zinc-400">
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
