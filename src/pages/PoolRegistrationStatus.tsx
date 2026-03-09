import { useEffect, useMemo, useState } from "react";
import { formatTaskError, toUserError } from "../lib/errors";
import { machineList, poolBindOnchain, poolOnchainStatus } from "../lib/ipc";
import type { Machine, Pool, PoolOnchainStatus } from "../lib/types";

interface PoolRegistrationStatusProps {
  poolTicker: string;
  onBound: (pool: Pool) => void;
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

export default function PoolRegistrationStatus({ poolTicker, onBound }: PoolRegistrationStatusProps) {
  const [loading, setLoading] = useState(true);
  const [querying, setQuerying] = useState(false);
  const [binding, setBinding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [machineId, setMachineId] = useState<number | null>(null);
  const [poolId, setPoolId] = useState("");
  const [coldVkeyPath, setColdVkeyPath] = useState("/opt/cardano/keys/cold.vkey");
  const [status, setStatus] = useState<PoolOnchainStatus | null>(null);

  useEffect(() => {
    let active = true;
    const load = async () => {
      setLoading(true);
      setError(null);
      try {
        const rows = await machineList();
        if (!active) {
          return;
        }
        const queryMachines = rows.filter((row) => row.role === "relay" || row.role === "bp");
        setMachines(queryMachines);
        if (queryMachines.length > 0) {
          const preferred = queryMachines.find((row) => row.role === "relay") ?? queryMachines[0];
          setMachineId(preferred.id);
        }
      } catch (e) {
        if (active) {
          setError(toUserError(e));
        }
      } finally {
        if (active) {
          setLoading(false);
        }
      }
    };
    void load();
    return () => {
      active = false;
    };
  }, []);

  const selectedMachine = useMemo(
    () => machines.find((row) => row.id === machineId) ?? null,
    [machineId, machines],
  );

  const handleQuery = async () => {
    if (machineId == null) {
      setError("Select a relay or BP machine first.");
      return;
    }
    setQuerying(true);
    setError(null);
    setMessage(null);
    try {
      const next = await poolOnchainStatus({
        machine_id: machineId,
        pool_id: poolId.trim() || undefined,
        cold_vkey_path: coldVkeyPath.trim() || undefined,
      });
      setStatus(next);
    } catch (e) {
      setError(toUserError(e));
      setStatus(null);
    } finally {
      setQuerying(false);
    }
  };

  const handleBind = async () => {
    if (!status?.registered_onchain || !status.pool_id || machineId == null) {
      setError("Query a registered on-chain pool first.");
      return;
    }
    setBinding(true);
    setError(null);
    setMessage(null);
    try {
      const nextPool = await poolBindOnchain({
        machine_id: machineId,
        pool_id: status.pool_id,
      });
      onBound(nextPool);
      setMessage("On-chain pool bound and persisted to local database.");
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setBinding(false);
    }
  };

  const registration = status?.registration;

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">On-chain Pool Status</h1>
        <p className="mt-1 text-sm text-zinc-400">
          Query the current stake pool registration state from a running relay or BP. This page is
          read-only and is intended to validate on-chain data before the registration wizard lands.
        </p>
      </header>

      {error && (
        <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
          {formatTaskError(error)}
        </p>
      )}
      {message && (
        <p className="rounded-md border border-emerald-700/60 bg-emerald-900/20 px-3 py-2 text-sm text-emerald-300">
          {message}
        </p>
      )}

      <section className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        {loading ? (
          <p className="text-sm text-zinc-300">Loading query machines...</p>
        ) : machines.length === 0 ? (
          <p className="text-sm text-zinc-400">No relay or BP machines available for on-chain query.</p>
        ) : (
          <div className="space-y-4">
            <div className="grid gap-4 md:grid-cols-3">
              <label className="block text-sm">
                <span className="mb-1 block text-zinc-300">Query machine</span>
                <select
                  value={machineId ?? ""}
                  onChange={(event) => setMachineId(Number(event.target.value))}
                  className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
                >
                  {machines.map((machine) => (
                    <option key={machine.id} value={machine.id}>
                      {machine.name} ({machine.role})
                    </option>
                  ))}
                </select>
              </label>

              <label className="block text-sm">
                <span className="mb-1 block text-zinc-300">Pool ID (preferred)</span>
                <input
                  value={poolId}
                  onChange={(event) => setPoolId(event.target.value)}
                  placeholder="pool1..."
                  className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
                />
              </label>

              <label className="block text-sm">
                <span className="mb-1 block text-zinc-300">Cold vkey path (fallback)</span>
                <input
                  value={coldVkeyPath}
                  onChange={(event) => setColdVkeyPath(event.target.value)}
                  placeholder="/opt/cardano/keys/cold.vkey"
                  className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
                />
              </label>
            </div>

            <div className="flex items-center justify-between gap-3">
              <div className="text-xs text-zinc-500">
                <p>Pool label: {poolTicker}</p>
                {selectedMachine && (
                  <p>
                    Network: {selectedMachine.network} | Role: {selectedMachine.role}
                  </p>
                )}
              </div>
              <button
                type="button"
                onClick={() => void handleQuery()}
                disabled={querying || machineId == null}
                className="rounded-md bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-900 disabled:cursor-not-allowed disabled:bg-zinc-700 disabled:text-zinc-400"
              >
                {querying ? "Querying..." : "Query On-chain Status"}
              </button>
            </div>
            {status?.registered_onchain && status.pool_id && (
              <div className="flex justify-end">
                <button
                  type="button"
                  onClick={() => void handleBind()}
                  disabled={binding}
                  className="rounded-md border border-emerald-700/70 bg-emerald-900/30 px-4 py-2 text-sm font-medium text-emerald-200 disabled:cursor-not-allowed disabled:opacity-70"
                >
                  {binding ? "Binding..." : "Bind Pool To Workspace"}
                </button>
              </div>
            )}
          </div>
        )}
      </section>

      {status && (
        <section className="space-y-4">
          <article className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold text-zinc-100">{status.machine_name}</h2>
                <p className="mt-1 text-xs uppercase tracking-wide text-zinc-500">
                  {status.network} · source: {status.query_source}
                </p>
              </div>
              <span
                className={`rounded-full px-2 py-1 text-xs font-medium ${
                  status.registered_onchain
                    ? "bg-emerald-900/40 text-emerald-300"
                    : "bg-amber-900/40 text-amber-300"
                }`}
              >
                {status.registered_onchain ? "registered on-chain" : "not registered"}
              </span>
            </div>

            <dl className="mt-4 grid gap-3 md:grid-cols-3 text-sm">
              <div>
                <dt className="text-zinc-500">Resolved Pool ID</dt>
                <dd className="mt-1 break-all font-medium text-zinc-100">{status.pool_id ?? "--"}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">Cold vkey path</dt>
                <dd className="mt-1 break-all font-medium text-zinc-100">
                  {status.cold_vkey_path ?? "--"}
                </dd>
              </div>
              <div>
                <dt className="text-zinc-500">Missing requirements</dt>
                <dd className="mt-1 font-medium text-zinc-100">
                  {status.missing_requirements.length > 0
                    ? status.missing_requirements.join(", ")
                    : "none"}
                </dd>
              </div>
            </dl>

            <p className="mt-4 rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 text-sm text-zinc-300">
              {status.note}
            </p>
          </article>

          {registration && (
            <article className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
              <h2 className="text-base font-semibold text-zinc-100">Registered Parameters</h2>
              <dl className="mt-4 grid gap-3 md:grid-cols-3 text-sm">
                <div>
                  <dt className="text-zinc-500">Ticker</dt>
                  <dd className="mt-1 font-medium text-zinc-100">{registration.ticker ?? "--"}</dd>
                </div>
                <div>
                  <dt className="text-zinc-500">Margin</dt>
                  <dd className="mt-1 font-medium text-zinc-100">
                    {formatMargin(registration.margin)}
                  </dd>
                </div>
                <div>
                  <dt className="text-zinc-500">Fixed Cost</dt>
                  <dd className="mt-1 font-medium text-zinc-100">
                    {formatLovelace(registration.fixed_cost)}
                  </dd>
                </div>
                <div>
                  <dt className="text-zinc-500">Pledge</dt>
                  <dd className="mt-1 font-medium text-zinc-100">
                    {formatLovelace(registration.pledge)}
                  </dd>
                </div>
                <div>
                  <dt className="text-zinc-500">Reward Account</dt>
                  <dd className="mt-1 break-all font-medium text-zinc-100">
                    {registration.reward_account ?? "--"}
                  </dd>
                </div>
                <div>
                  <dt className="text-zinc-500">Metadata Hash</dt>
                  <dd className="mt-1 break-all font-medium text-zinc-100">
                    {registration.metadata_hash ?? "--"}
                  </dd>
                </div>
              </dl>

              <div className="mt-4 grid gap-4 md:grid-cols-2">
                <div>
                  <h3 className="text-sm font-medium text-zinc-200">Owners</h3>
                  <ul className="mt-2 space-y-2 text-xs text-zinc-300">
                    {registration.owners.length > 0 ? (
                      registration.owners.map((owner) => (
                        <li
                          key={owner}
                          className="rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 break-all"
                        >
                          {owner}
                        </li>
                      ))
                    ) : (
                      <li className="rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 text-zinc-500">
                        No owners returned.
                      </li>
                    )}
                  </ul>
                </div>

                <div>
                  <h3 className="text-sm font-medium text-zinc-200">Relays</h3>
                  <ul className="mt-2 space-y-2 text-xs text-zinc-300">
                    {registration.relays.length > 0 ? (
                      registration.relays.map((relay) => (
                        <li
                          key={`${relay.address}:${relay.port}`}
                          className="rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 break-all"
                        >
                          {relay.address}:{relay.port}
                        </li>
                      ))
                    ) : (
                      <li className="rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 text-zinc-500">
                        No relays returned.
                      </li>
                    )}
                  </ul>
                </div>
              </div>

              <div className="mt-4">
                <h3 className="text-sm font-medium text-zinc-200">Metadata URL</h3>
                <p className="mt-2 rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 text-xs break-all text-zinc-300">
                  {registration.metadata_url ?? "--"}
                </p>
              </div>
            </article>
          )}
        </section>
      )}
    </section>
  );
}
