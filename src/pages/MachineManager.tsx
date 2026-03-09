import { useEffect, useMemo, useState, type FormEvent } from "react";
import { formatTaskError, toUserError } from "../lib/errors";
import {
  machineAdd,
  machineList,
  machinePreflight,
  machineRemove,
  machineRuntimeProbe,
  runtimeApplyConfig,
  runtimeConfigStatus,
  runtimeRestart,
  runtimeRestartStatus,
  sshAgentAddKey,
  sshAgentListKeys,
} from "../lib/ipc";
import type {
  DeployTaskStatus,
  Machine,
  MachineAddPayload,
  Pool,
  PreflightReport,
  RuntimeProbe,
  SshKeyInfo,
} from "../lib/types";

interface MachineManagerProps {
  pool: Pool;
}

type Role = MachineAddPayload["role"];

const roleOptions: Role[] = ["relay", "bp", "archive"];

function isTerminal(status: string): boolean {
  return status === "success" || status === "failed" || status === "cancelled";
}

function formatMachineLoadStatus(elapsedSeconds: number): string {
  if (elapsedSeconds < 3) {
    return "Requesting machine list from local app...";
  }
  if (elapsedSeconds < 10) {
    return `Still waiting for machine_list response (${elapsedSeconds}s)...`;
  }
  return `Still waiting for machine_list response (${elapsedSeconds}s). Local DB or Tauri command may be blocked.`;
}

export default function MachineManager({ pool }: MachineManagerProps) {
  const [loading, setLoading] = useState(true);
  const [loadingElapsedSeconds, setLoadingElapsedSeconds] = useState(0);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const [preflightMap, setPreflightMap] = useState<Record<number, PreflightReport>>({});
  const [runtimeProbeMap, setRuntimeProbeMap] = useState<Record<number, RuntimeProbe>>({});
  const [runtimeConfigTasks, setRuntimeConfigTasks] = useState<Record<number, DeployTaskStatus>>({});
  const [runtimeRestartTasks, setRuntimeRestartTasks] = useState<Record<number, DeployTaskStatus>>({});
  const [runningPreflight, setRunningPreflight] = useState<number | null>(null);
  const [runningProbe, setRunningProbe] = useState<number | null>(null);
  const [runningRuntimeConfig, setRunningRuntimeConfig] = useState<number | null>(null);
  const [runningRuntimeRestart, setRunningRuntimeRestart] = useState<number | null>(null);
  const [addingKey, setAddingKey] = useState(false);

  const [name, setName] = useState("");
  const [ip, setIp] = useState("");
  const [port, setPort] = useState("22");
  const [sshUser, setSshUser] = useState("root");
  const [role, setRole] = useState<Role>("relay");
  const [fingerprint, setFingerprint] = useState("");
  const [keyPath, setKeyPath] = useState("~/.ssh/id_ed25519");

  const keyOptions = useMemo(() => keys.map((k) => k.fingerprint), [keys]);

  const loadData = async () => {
    setLoading(true);
    setLoadingElapsedSeconds(0);
    setError(null);
    try {
      const [machineRows, keyRows] = await Promise.all([machineList(), sshAgentListKeys()]);
      setMachines(machineRows);
      setKeys(keyRows);
      if (keyRows.length > 0 && !fingerprint) {
        setFingerprint(keyRows[0].fingerprint);
      }
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadData();
    // only on mount
  }, []);

  useEffect(() => {
    if (!loading) {
      return;
    }
    const startedAt = Date.now();
    const timer = window.setInterval(() => {
      setLoadingElapsedSeconds(Math.floor((Date.now() - startedAt) / 1000));
    }, 1000);
    return () => {
      window.clearInterval(timer);
    };
  }, [loading]);

  useEffect(() => {
    const activeTaskEntries = Object.entries(runtimeConfigTasks).filter(
      ([, task]) => task && !isTerminal(task.status),
    );
    if (activeTaskEntries.length === 0) {
      return;
    }
    let active = true;
    const timer = window.setInterval(() => {
      void Promise.all(
        activeTaskEntries.map(async ([machineId, task]) => {
          const next = await runtimeConfigStatus(task.task_id);
          return [Number(machineId), next] as const;
        }),
      )
        .then((rows) => {
          if (!active) {
            return;
          }
          setRuntimeConfigTasks((prev) => {
            const next = { ...prev };
            rows.forEach(([machineId, task]) => {
              next[machineId] = task;
            });
            return next;
          });
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
  }, [runtimeConfigTasks]);

  useEffect(() => {
    const activeTaskEntries = Object.entries(runtimeRestartTasks).filter(
      ([, task]) => task && !isTerminal(task.status),
    );
    if (activeTaskEntries.length === 0) {
      return;
    }
    let active = true;
    const timer = window.setInterval(() => {
      void Promise.all(
        activeTaskEntries.map(async ([machineId, task]) => {
          const next = await runtimeRestartStatus(task.task_id);
          return [Number(machineId), next] as const;
        }),
      )
        .then((rows) => {
          if (!active) {
            return;
          }
          setRuntimeRestartTasks((prev) => {
            const next = { ...prev };
            rows.forEach(([machineId, task]) => {
              next[machineId] = task;
            });
            return next;
          });
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
  }, [runtimeRestartTasks]);

  const handleAdd = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const payload: MachineAddPayload = {
        name: name.trim(),
        ip: ip.trim(),
        port: Number(port),
        ssh_user: sshUser.trim(),
        role,
        network: pool.network,
        ssh_key_fingerprint: fingerprint,
      };
      await machineAdd(payload);
      setName("");
      setIp("");
      setPort("22");
      setSshUser("root");
      setRole("relay");
      await loadData();
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setSubmitting(false);
    }
  };

  const handleRemove = async (machineId: number) => {
    setError(null);
    try {
      await machineRemove(machineId);
      setMachines((prev) => prev.filter((m) => m.id !== machineId));
      setPreflightMap((prev) => {
        const copy = { ...prev };
        delete copy[machineId];
        return copy;
      });
    } catch (e) {
      setError(toUserError(e));
    }
  };

  const handlePreflight = async (machineId: number) => {
    setError(null);
    setRunningPreflight(machineId);
    try {
      const report = await machinePreflight(machineId);
      setPreflightMap((prev) => ({ ...prev, [machineId]: report }));
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setRunningPreflight(null);
    }
  };

  const handleRuntimeProbe = async (machineId: number) => {
    setError(null);
    setRunningProbe(machineId);
    try {
      const probe = await machineRuntimeProbe(machineId);
      setRuntimeProbeMap((prev) => ({ ...prev, [machineId]: probe }));
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setRunningProbe(null);
    }
  };

  const handleAddKey = async () => {
    setAddingKey(true);
    setError(null);
    try {
      const updatedKeys = await sshAgentAddKey(keyPath.trim());
      setKeys(updatedKeys);
      if (updatedKeys.length > 0) {
        setFingerprint(updatedKeys[0].fingerprint);
      }
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setAddingKey(false);
    }
  };

  const handleRuntimeApplyConfig = async (machineId: number) => {
    setRunningRuntimeConfig(machineId);
    setError(null);
    try {
      const taskId = await runtimeApplyConfig(machineId);
      const task = await runtimeConfigStatus(taskId);
      setRuntimeConfigTasks((prev) => ({ ...prev, [machineId]: task }));
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setRunningRuntimeConfig(null);
    }
  };

  const handleRuntimeRestart = async (machineId: number) => {
    setRunningRuntimeRestart(machineId);
    setError(null);
    try {
      const taskId = await runtimeRestart(machineId);
      const task = await runtimeRestartStatus(taskId);
      setRuntimeRestartTasks((prev) => ({ ...prev, [machineId]: task }));
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setRunningRuntimeRestart(null);
    }
  };

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Machine Manager</h1>
        <p className="mt-1 text-sm text-zinc-400">Pool network: {pool.network}</p>
      </header>

      <form onSubmit={handleAdd} className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
        <h2 className="mb-4 text-lg font-medium">Add Machine</h2>
        {keyOptions.length === 0 && (
          <div className="mb-4 rounded-md border border-yellow-700/60 bg-yellow-900/20 p-3 text-sm text-yellow-200">
            <p>No keys in ssh-agent. Add a private key path to continue.</p>
            <div className="mt-2 flex flex-col gap-2 md:flex-row">
              <input
                className="flex-1 rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
                placeholder="~/.ssh/id_ed25519"
                value={keyPath}
                onChange={(e) => setKeyPath(e.target.value)}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
              />
              <button
                type="button"
                onClick={() => void handleAddKey()}
                disabled={addingKey || keyPath.trim().length === 0}
                className="rounded-md border border-yellow-600/70 px-3 py-2 text-sm hover:bg-yellow-900/30 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {addingKey ? "Adding key..." : "Add Key to ssh-agent"}
              </button>
            </div>
          </div>
        )}
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <input
            className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
            placeholder="name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            required
          />
          <input
            className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
            placeholder="ip"
            value={ip}
            onChange={(e) => setIp(e.target.value)}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            required
          />
          <input
            className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
            placeholder="ssh port"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            inputMode="numeric"
            required
          />
          <input
            className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
            placeholder="ssh user"
            value={sshUser}
            onChange={(e) => setSshUser(e.target.value)}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            required
          />
          <select
            className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
            value={role}
            onChange={(e) => setRole(e.target.value as Role)}
          >
            {roleOptions.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
          <select
            className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
            value={fingerprint}
            onChange={(e) => setFingerprint(e.target.value)}
            required
            disabled={keyOptions.length === 0}
          >
            {keyOptions.length === 0 ? (
              <option value="">No keys in ssh-agent</option>
            ) : (
              keyOptions.map((fp) => (
                <option key={fp} value={fp}>
                  {fp}
                </option>
              ))
            )}
          </select>
        </div>
        <button
          type="submit"
          disabled={submitting || keyOptions.length === 0}
          className="mt-4 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-70"
        >
          {submitting ? "Adding..." : "Add Machine"}
        </button>
      </form>

      {error && (
        <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      )}

      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
        <h2 className="mb-4 text-lg font-medium">Machines</h2>
        {loading ? (
          <div className="rounded-md border border-zinc-800 bg-zinc-950/50 p-4 text-sm text-zinc-300">
            <p className="font-medium text-zinc-100">Loading machines...</p>
            <p className="mt-2 text-zinc-400">{formatMachineLoadStatus(loadingElapsedSeconds)}</p>
            <p className="mt-1 text-xs text-zinc-500">Elapsed: {loadingElapsedSeconds}s</p>
          </div>
        ) : machines.length === 0 ? (
          <p className="text-sm text-zinc-400">No machines added.</p>
        ) : (
          <div className="space-y-3">
            {machines.map((machine) => (
              <article key={machine.id} className="rounded-md border border-zinc-800 bg-zinc-950 p-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <p className="text-sm font-medium text-zinc-100">
                      {machine.name} ({machine.role})
                    </p>
                    <p className="text-xs text-zinc-400">
                      {machine.ip}:{machine.port} · {machine.ssh_user}
                    </p>
                  </div>
                  <div className="flex gap-2">
                    <button
                      type="button"
                      onClick={() => void handlePreflight(machine.id)}
                      className="rounded-md border border-zinc-700 px-3 py-1 text-xs hover:bg-zinc-800"
                      disabled={runningPreflight === machine.id}
                    >
                      {runningPreflight === machine.id ? "Preflighting..." : "Preflight"}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleRuntimeProbe(machine.id)}
                      className="rounded-md border border-zinc-700 px-3 py-1 text-xs hover:bg-zinc-800"
                      disabled={runningProbe === machine.id}
                    >
                      {runningProbe === machine.id ? "Probing..." : "Runtime Probe"}
                    </button>
                    {machine.role !== "archive" && (
                      <>
                        <button
                          type="button"
                          onClick={() => void handleRuntimeApplyConfig(machine.id)}
                          className="rounded-md border border-blue-700/70 px-3 py-1 text-xs text-blue-200 hover:bg-blue-950/30"
                          disabled={runningRuntimeConfig === machine.id}
                        >
                          {runningRuntimeConfig === machine.id
                            ? "Applying Config..."
                            : "Apply Runtime Config"}
                        </button>
                        <button
                          type="button"
                          onClick={() => void handleRuntimeRestart(machine.id)}
                          className="rounded-md border border-amber-700/70 px-3 py-1 text-xs text-amber-200 hover:bg-amber-950/30"
                          disabled={runningRuntimeRestart === machine.id}
                        >
                          {runningRuntimeRestart === machine.id
                            ? "Restarting..."
                            : "Restart Runtime"}
                        </button>
                      </>
                    )}
                    <button
                      type="button"
                      onClick={() => void handleRemove(machine.id)}
                      className="rounded-md border border-red-700/70 px-3 py-1 text-xs text-red-300 hover:bg-red-950/40"
                    >
                      Remove
                    </button>
                  </div>
                </div>
                {preflightMap[machine.id] && (
                  <div className="mt-3 rounded-md border border-zinc-800 bg-black/20 p-2 text-xs text-zinc-300">
                    <p>ssh_ok: {String(preflightMap[machine.id].ssh_ok)}</p>
                    <p>os: {preflightMap[machine.id].os_version}</p>
                    <p>disk_available_gb: {preflightMap[machine.id].disk_available_gb}</p>
                    <p>memory_total_gb: {preflightMap[machine.id].memory_total_gb}</p>
                    <p>disk_iops: {preflightMap[machine.id].disk_iops}</p>
                    {preflightMap[machine.id].warnings.length > 0 && (
                      <ul className="mt-2 list-disc pl-5 text-yellow-300">
                        {preflightMap[machine.id].warnings.map((warning) => (
                          <li key={warning}>{warning}</li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
                {runtimeProbeMap[machine.id] && (
                  <div className="mt-3 rounded-md border border-zinc-800 bg-black/20 p-2 text-xs text-zinc-300">
                    <p>container_present: {String(runtimeProbeMap[machine.id].container_present)}</p>
                    <p>image_ref: {runtimeProbeMap[machine.id].image_ref ?? "-"}</p>
                    <p>managed_by_compose: {String(runtimeProbeMap[machine.id].managed_by_compose)}</p>
                    <p>db_mount_source: {runtimeProbeMap[machine.id].db_mount_source ?? "-"}</p>
                    <p>keys_mount_source: {runtimeProbeMap[machine.id].keys_mount_source ?? "-"}</p>
                    <p>bp_key_files_present: {String(runtimeProbeMap[machine.id].bp_key_files_present)}</p>
                  </div>
                )}
                {machine.role !== "archive" && (
                  <div className="mt-3 rounded-md border border-zinc-800 bg-black/20 p-2 text-xs text-zinc-300">
                    <p className="font-medium text-zinc-100">Runtime Operations</p>
                    <p className="mt-1 text-zinc-400">
                      Apply Runtime Config will re-render role-aware config and restart the
                      container if files changed. Restart Runtime only restarts the existing
                      cardano-node container and does not run deploy or Mithril flows.
                    </p>
                    {runtimeConfigTasks[machine.id] && (
                      <div className="mt-2">
                        <p>
                          config task: {runtimeConfigTasks[machine.id].status} (
                          {runtimeConfigTasks[machine.id].task_id})
                        </p>
                        {formatTaskError(runtimeConfigTasks[machine.id].error_msg) && (
                          <p className="text-red-300">
                            {formatTaskError(runtimeConfigTasks[machine.id].error_msg)}
                          </p>
                        )}
                      </div>
                    )}
                    {runtimeRestartTasks[machine.id] && (
                      <div className="mt-2">
                        <p>
                          restart task: {runtimeRestartTasks[machine.id].status} (
                          {runtimeRestartTasks[machine.id].task_id})
                        </p>
                        {formatTaskError(runtimeRestartTasks[machine.id].error_msg) && (
                          <p className="text-red-300">
                            {formatTaskError(runtimeRestartTasks[machine.id].error_msg)}
                          </p>
                        )}
                      </div>
                    )}
                  </div>
                )}
              </article>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
