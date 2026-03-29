import { useEffect, useMemo, useState } from "react";
import { formatTaskError, toUserError } from "../lib/errors";
import { machineList, poolBindOnchain, poolOnchainStatus } from "../lib/ipc";
import type { Machine, Pool, PoolOnchainStatus } from "../lib/types";

interface PoolRegistrationStatusProps {
  poolTicker: string;
  onBound: (pool: Pool) => void;
  embedded?: boolean;
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

export default function PoolRegistrationStatus({
  poolTicker,
  onBound,
  embedded = false,
}: PoolRegistrationStatusProps) {
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
      {!embedded && (
        <header>
          <h1 className="text-2xl font-semibold tracking-tight">On-chain Pool Status</h1>
          <p className="mt-1 text-sm text-slate-500">
            Query the current stake pool registration state from a running relay or BP. This page is
            read-only and is intended to validate on-chain data before the registration wizard lands.
          </p>
        </header>
      )}

      {error && (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
          {formatTaskError(error)}
        </p>
      )}
      {message && (
        <p className="rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-sm text-emerald-700">
          {message}
        </p>
      )}

      <section className="rounded-lg border border-slate-200 bg-slate-50 p-4">
        {loading ? (
          <p className="text-sm text-slate-700">Loading query machines...</p>
        ) : machines.length === 0 ? (
          <p className="text-sm text-slate-500">No relay or BP machines available for on-chain query.</p>
        ) : (
          <div className="space-y-4">
            <div className="grid gap-4 md:grid-cols-3">
              <label className="block text-sm">
                <span className="mb-1 block text-slate-700">Query machine</span>
                <select
                  value={machineId ?? ""}
                  onChange={(event) => setMachineId(Number(event.target.value))}
                  className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900"
                >
                  {machines.map((machine) => (
                    <option key={machine.id} value={machine.id}>
                      {machine.name} ({machine.role})
                    </option>
                  ))}
                </select>
              </label>

              <label className="block text-sm">
                <span className="mb-1 block text-slate-700">Pool ID (preferred)</span>
                <input
                  value={poolId}
                  onChange={(event) => setPoolId(event.target.value)}
                  placeholder="pool1..."
                  className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900"
                />
              </label>

              <label className="block text-sm">
                <span className="mb-1 block text-slate-700">Cold vkey path (fallback)</span>
                <input
                  value={coldVkeyPath}
                  onChange={(event) => setColdVkeyPath(event.target.value)}
                  placeholder="/opt/cardano/keys/cold.vkey"
                  className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900"
                />
              </label>
            </div>

            <div className="flex items-center justify-between gap-3">
              <div className="text-xs text-slate-500">
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
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white disabled:cursor-not-allowed disabled:bg-slate-300 disabled:text-slate-500"
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
                  className="rounded-md border border-emerald-300 bg-emerald-50 px-4 py-2 text-sm font-medium text-emerald-700 disabled:cursor-not-allowed disabled:opacity-70"
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
          <article className="rounded-lg border border-slate-200 bg-slate-50 p-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold text-slate-900">{status.machine_name}</h2>
                <p className="mt-1 text-xs uppercase tracking-wide text-slate-500">
                  {status.network} · source: {status.query_source}
                </p>
              </div>
              <span
                className={`rounded-full px-2 py-1 text-xs font-medium ${
                  status.registered_onchain
                    ? "bg-emerald-50 text-emerald-700"
                    : "bg-amber-50 text-amber-700"
                }`}
              >
                {status.registered_onchain ? "registered on-chain" : "not registered"}
              </span>
            </div>

            <dl className="mt-4 grid gap-3 md:grid-cols-3 text-sm">
              <div>
                <dt className="text-slate-500">Resolved Pool ID</dt>
                <dd className="mt-1 break-all font-medium text-slate-900">{status.pool_id ?? "--"}</dd>
              </div>
              <div>
                <dt className="text-slate-500">Cold vkey path</dt>
                <dd className="mt-1 break-all font-medium text-slate-900">
                  {status.cold_vkey_path ?? "--"}
                </dd>
              </div>
              <div>
                <dt className="text-slate-500">Missing requirements</dt>
                <dd className="mt-1 font-medium text-slate-900">
                  {status.missing_requirements.length > 0
                    ? status.missing_requirements.join(", ")
                    : "none"}
                </dd>
              </div>
            </dl>

            <p className="mt-4 rounded-md border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700">
              {status.note}
            </p>
          </article>

          {registration && (
            <article className="rounded-lg border border-slate-200 bg-slate-50 p-4">
              <h2 className="text-base font-semibold text-slate-900">Registered Parameters</h2>
              <dl className="mt-4 grid gap-3 md:grid-cols-3 text-sm">
                <div>
                  <dt className="text-slate-500">Ticker</dt>
                  <dd className="mt-1 font-medium text-slate-900">{registration.ticker ?? "--"}</dd>
                </div>
                <div>
                  <dt className="text-slate-500">Margin</dt>
                  <dd className="mt-1 font-medium text-slate-900">
                    {formatMargin(registration.margin)}
                  </dd>
                </div>
                <div>
                  <dt className="text-slate-500">Fixed Cost</dt>
                  <dd className="mt-1 font-medium text-slate-900">
                    {formatLovelace(registration.fixed_cost)}
                  </dd>
                </div>
                <div>
                  <dt className="text-slate-500">Pledge</dt>
                  <dd className="mt-1 font-medium text-slate-900">
                    {formatLovelace(registration.pledge)}
                  </dd>
                </div>
                <div>
                  <dt className="text-slate-500">Reward Account</dt>
                  <dd className="mt-1 break-all font-medium text-slate-900">
                    {registration.reward_account ?? "--"}
                  </dd>
                </div>
                <div>
                  <dt className="text-slate-500">Metadata Hash</dt>
                  <dd className="mt-1 break-all font-medium text-slate-900">
                    {registration.metadata_hash ?? "--"}
                  </dd>
                </div>
              </dl>

              <div className="mt-4 grid gap-4 md:grid-cols-2">
                <div>
                  <h3 className="text-sm font-medium text-slate-800">Owners</h3>
                  <ul className="mt-2 space-y-2 text-xs text-slate-700">
                    {registration.owners.length > 0 ? (
                      registration.owners.map((owner) => (
                        <li
                          key={owner}
                          className="rounded-md border border-slate-200 bg-white px-3 py-2 break-all"
                        >
                          {owner}
                        </li>
                      ))
                    ) : (
                      <li className="rounded-md border border-slate-200 bg-white px-3 py-2 text-slate-500">
                        No owners returned.
                      </li>
                    )}
                  </ul>
                </div>

                <div>
                  <h3 className="text-sm font-medium text-slate-800">Relays</h3>
                  <ul className="mt-2 space-y-2 text-xs text-slate-700">
                    {registration.relays.length > 0 ? (
                      registration.relays.map((relay) => (
                        <li
                          key={`${relay.address}:${relay.port}`}
                          className="rounded-md border border-slate-200 bg-white px-3 py-2 break-all"
                        >
                          {relay.address}:{relay.port}
                        </li>
                      ))
                    ) : (
                      <li className="rounded-md border border-slate-200 bg-white px-3 py-2 text-slate-500">
                        No relays returned.
                      </li>
                    )}
                  </ul>
                </div>
              </div>

              <div className="mt-4">
                <h3 className="text-sm font-medium text-slate-800">Metadata URL</h3>
                <p className="mt-2 rounded-md border border-slate-200 bg-white px-3 py-2 text-xs break-all text-slate-700">
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
