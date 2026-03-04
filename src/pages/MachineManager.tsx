import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  machineAdd,
  machineList,
  machinePreflight,
  machineRemove,
  sshAgentAddKey,
  sshAgentListKeys,
} from "../lib/ipc";
import type {
  Machine,
  MachineAddPayload,
  Pool,
  PreflightReport,
  SshKeyInfo,
} from "../lib/types";

interface MachineManagerProps {
  pool: Pool;
}

type Role = MachineAddPayload["role"];

const roleOptions: Role[] = ["relay", "bp", "archive"];

export default function MachineManager({ pool }: MachineManagerProps) {
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const [preflightMap, setPreflightMap] = useState<Record<number, PreflightReport>>({});
  const [runningPreflight, setRunningPreflight] = useState<number | null>(null);
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
    setError(null);
    try {
      const [machineRows, keyRows] = await Promise.all([machineList(), sshAgentListKeys()]);
      setMachines(machineRows);
      setKeys(keyRows);
      if (keyRows.length > 0 && !fingerprint) {
        setFingerprint(keyRows[0].fingerprint);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadData();
    // only on mount
  }, []);

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
      setError(String(e));
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
      setError(String(e));
    }
  };

  const handlePreflight = async (machineId: number) => {
    setError(null);
    setRunningPreflight(machineId);
    try {
      const report = await machinePreflight(machineId);
      setPreflightMap((prev) => ({ ...prev, [machineId]: report }));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunningPreflight(null);
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
      setError(String(e));
    } finally {
      setAddingKey(false);
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
          <p className="text-sm text-zinc-400">Loading...</p>
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
              </article>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
