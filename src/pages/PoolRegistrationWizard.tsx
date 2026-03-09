import { useEffect, useMemo, useState } from "react";
import { formatTaskError, toUserError } from "../lib/errors";
import {
  machineList,
  poolRegistrationPrepare,
  poolRegistrationSubmit,
} from "../lib/ipc";
import type {
  Machine,
  PoolRegistrationPrepareResult,
  PoolRegistrationSubmitResult,
} from "../lib/types";

interface PoolRegistrationWizardProps {
  poolTicker: string;
}

function truncatePath(value: string | null | undefined): string {
  if (!value) {
    return "--";
  }
  return value;
}

function formatLovelace(value: number | null): string {
  if (value == null) {
    return "--";
  }
  return value.toLocaleString();
}

export default function PoolRegistrationWizard({ poolTicker }: PoolRegistrationWizardProps) {
  const [loading, setLoading] = useState(true);
  const [preparing, setPreparing] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [machineId, setMachineId] = useState<number | null>(null);
  const [poolId, setPoolId] = useState("");
  const [certificatePath, setCertificatePath] = useState(
    "/opt/cardano/config/registration/pool-registration.cert",
  );
  const [paymentAddrPath, setPaymentAddrPath] = useState("/opt/cardano/config/payment.addr");
  const [txSignedPath, setTxSignedPath] = useState(
    "/opt/cardano/config/registration/pool-registration.signed",
  );
  const [confirmPoolId, setConfirmPoolId] = useState("");
  const [prepareResult, setPrepareResult] = useState<PoolRegistrationPrepareResult | null>(null);
  const [submitResult, setSubmitResult] = useState<PoolRegistrationSubmitResult | null>(null);

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

  const handlePrepare = async () => {
    if (machineId == null) {
      setError("Select a relay or BP machine first.");
      return;
    }
    if (!poolId.trim()) {
      setError("Pool ID is required.");
      return;
    }
    setPreparing(true);
    setError(null);
    setMessage(null);
    setSubmitResult(null);
    try {
      const next = await poolRegistrationPrepare({
        machine_id: machineId,
        pool_id: poolId.trim(),
        certificate_path: certificatePath.trim(),
        payment_addr_path: paymentAddrPath.trim(),
      });
      setPrepareResult(next);
      setTxSignedPath(
        next.tx_draft.tx_body_path?.replace(/\.raw$/, ".signed") ??
          "/opt/cardano/config/registration/pool-registration.signed",
      );
      setConfirmPoolId("");
      setMessage("Unsigned registration transaction prepared. Complete cold signing before submit.");
    } catch (e) {
      setPrepareResult(null);
      setError(toUserError(e));
    } finally {
      setPreparing(false);
    }
  };

  const handleSubmit = async () => {
    if (machineId == null) {
      setError("Select a relay or BP machine first.");
      return;
    }
    if (!prepareResult?.pool_id) {
      setError("Prepare the registration transaction first.");
      return;
    }
    setSubmitting(true);
    setError(null);
    setMessage(null);
    try {
      const next = await poolRegistrationSubmit({
        machine_id: machineId,
        pool_id: prepareResult.pool_id,
        confirm_pool_id: confirmPoolId.trim(),
        tx_signed_path: txSignedPath.trim(),
      });
      setSubmitResult(next);
      setMessage("Signed registration transaction submitted. Wait for chain inclusion, then bind the pool.");
    } catch (e) {
      setError(toUserError(e));
      setSubmitResult(null);
    } finally {
      setSubmitting(false);
    }
  };

  const prepareBlocked = machineId == null || !poolId.trim();
  const canSubmit =
    !!prepareResult?.pool_id &&
    prepareResult.missing_requirements.length === 0 &&
    !!prepareResult.tx_draft.tx_body_path &&
    confirmPoolId.trim() === prepareResult.pool_id &&
    !!txSignedPath.trim();

  return (
    <section className="space-y-4">
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

      <div className="rounded-md border border-amber-900/50 bg-amber-950/20 px-3 py-2 text-xs text-amber-200">
        High-risk flow. The hot node only builds an unsigned transaction and later submits a pre-signed
        transaction. Registration certificate generation and transaction signing must stay in the cold
        environment.
      </div>

      <div className="rounded-md border border-zinc-800 bg-zinc-950/40 px-3 py-3 text-xs text-zinc-300">
        <p className="font-medium text-zinc-100">Cold / Hot Workflow</p>
        <ol className="mt-2 space-y-2">
          <li>
            1. In the cold environment, generate{" "}
            <span className="font-medium text-zinc-100">pool-registration.cert</span> from the pool cold
            signing material.
          </li>
          <li>
            2. Copy only that certificate to the hot node. Do not copy{" "}
            <span className="font-medium text-zinc-100">cold.skey</span> or{" "}
            <span className="font-medium text-zinc-100">cold.vkey</span> to the running node.
          </li>
          <li>
            3. Run <span className="font-medium text-zinc-100">Prepare Registration</span> on the hot node to
            build an unsigned transaction body.
          </li>
          <li>
            4. Move the unsigned tx body back to the cold environment, sign it offline, then copy only the
            signed tx file back for submission.
          </li>
        </ol>
      </div>

      {loading ? (
        <p className="text-sm text-zinc-300">Loading registration machines...</p>
      ) : machines.length === 0 ? (
        <p className="text-sm text-zinc-400">No relay or BP machines available for registration flow.</p>
      ) : (
        <div className="space-y-4">
          <div className="grid gap-4 md:grid-cols-2">
            <label className="block text-sm">
              <span className="mb-1 block text-zinc-300">Build/submit machine</span>
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
              <span className="mb-1 block text-zinc-300">Pool ID</span>
              <input
                value={poolId}
                onChange={(event) => setPoolId(event.target.value)}
                placeholder="pool1..."
                className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              />
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-zinc-300">Registration certificate path</span>
              <input
                value={certificatePath}
                onChange={(event) => setCertificatePath(event.target.value)}
                placeholder="/opt/cardano/config/registration/pool-registration.cert"
                className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              />
              <span className="mt-1 block text-xs text-zinc-500">
                Expected input from the cold environment. This file is not generated on the hot node.
              </span>
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-zinc-300">Payment address path</span>
              <input
                value={paymentAddrPath}
                onChange={(event) => setPaymentAddrPath(event.target.value)}
                placeholder="/opt/cardano/config/payment.addr"
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
              onClick={() => void handlePrepare()}
              disabled={preparing || prepareBlocked}
              className="rounded-md bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-900 disabled:cursor-not-allowed disabled:bg-zinc-700 disabled:text-zinc-400"
            >
              {preparing ? "Preparing..." : "Prepare Registration"}
            </button>
          </div>
        </div>
      )}

      {prepareResult && (
        <article className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h3 className="text-base font-semibold text-zinc-100">Registration Draft</h3>
              <p className="mt-1 text-xs text-zinc-400">{prepareResult.note}</p>
            </div>
            <span
              className={`rounded-full px-2 py-1 text-xs font-medium ${
                prepareResult.missing_requirements.length === 0
                  ? "bg-emerald-900/40 text-emerald-300"
                  : "bg-amber-900/40 text-amber-300"
              }`}
            >
              {prepareResult.missing_requirements.length === 0 ? "ready for cold sign" : "missing inputs"}
            </span>
          </div>

          <dl className="mt-4 grid gap-3 text-sm md:grid-cols-2">
            <div>
              <dt className="text-zinc-500">Pool ID</dt>
              <dd className="mt-1 break-all font-medium text-zinc-100">{prepareResult.pool_id ?? "--"}</dd>
            </div>
            <div>
              <dt className="text-zinc-500">Required Deposit</dt>
              <dd className="mt-1 font-medium text-zinc-100">
                {formatLovelace(prepareResult.tx_draft.required_deposit)}
              </dd>
            </div>
            <div>
              <dt className="text-zinc-500">Payment Address</dt>
              <dd className="mt-1 break-all font-medium text-zinc-100">
                {prepareResult.tx_draft.payment_address ?? "--"}
              </dd>
            </div>
            <div>
              <dt className="text-zinc-500">Unsigned Tx Body</dt>
              <dd className="mt-1 break-all font-medium text-zinc-100">
                {truncatePath(prepareResult.tx_draft.tx_body_path)}
              </dd>
            </div>
            <div>
              <dt className="text-zinc-500">Certificate Path</dt>
              <dd className="mt-1 break-all font-medium text-zinc-100">
                {truncatePath(prepareResult.certificate_path)}
              </dd>
            </div>
            <div>
              <dt className="text-zinc-500">Offline Signing</dt>
              <dd className="mt-1 font-medium text-zinc-100">
                {prepareResult.tx_draft.offline_signing_required ? "required" : "not required"}
              </dd>
            </div>
            <div className="md:col-span-2">
              <dt className="text-zinc-500">Registration Relays</dt>
              <dd className="mt-1 font-medium text-zinc-100">
                {prepareResult.registration_relays.length > 0
                  ? prepareResult.registration_relays
                      .map((relay) => `${relay.address}:${relay.port}`)
                      .join(", ")
                  : "--"}
              </dd>
            </div>
          </dl>

          {prepareResult.missing_requirements.length > 0 && (
            <div className="mt-4 rounded-md border border-amber-900/40 bg-amber-950/20 px-3 py-2 text-sm text-amber-200">
              <p className="font-medium">Missing requirements</p>
              <ul className="mt-2 space-y-1">
                {prepareResult.missing_requirements.map((item) => (
                  <li key={item}>- {item}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="mt-4 rounded-md border border-zinc-800 bg-black/20 px-3 py-2 text-xs text-zinc-400 break-words">
            <p className="font-medium text-zinc-200">Command preview</p>
            <p className="mt-2">{prepareResult.tx_draft.command_preview}</p>
          </div>

          <div className="mt-4 rounded-md border border-zinc-800 bg-zinc-950/30 px-3 py-2 text-xs text-zinc-400">
            <p className="font-medium text-zinc-200">Cold Environment Inputs</p>
            <ul className="mt-2 space-y-1">
              <li>- keep pool cold signing material offline</li>
              <li>- sign the unsigned tx body shown above</li>
              <li>- copy back only the signed transaction file</li>
            </ul>
          </div>
        </article>
      )}

      {prepareResult && (
        <article className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
          <h3 className="text-base font-semibold text-zinc-100">Submit Pre-signed Transaction</h3>
          <p className="mt-1 text-xs text-zinc-400">
            Sign <span className="font-medium text-zinc-200">{prepareResult.tx_draft.tx_body_path ?? "--"}</span>{" "}
            in the cold environment, copy the signed tx to the selected hot machine, then confirm the pool ID
            before submission.
          </p>

          <div className="mt-4 grid gap-4 md:grid-cols-2">
            <label className="block text-sm">
              <span className="mb-1 block text-zinc-300">Signed transaction path</span>
              <input
                value={txSignedPath}
                onChange={(event) => setTxSignedPath(event.target.value)}
                placeholder="/opt/cardano/config/registration/pool-registration.signed"
                className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              />
            </label>
            <label className="block text-sm">
              <span className="mb-1 block text-zinc-300">Confirm Pool ID</span>
              <input
                value={confirmPoolId}
                onChange={(event) => setConfirmPoolId(event.target.value)}
                placeholder={prepareResult.pool_id ?? "pool1..."}
                className="w-full rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100"
              />
            </label>
          </div>

          <div className="mt-4 flex items-center justify-between gap-3">
            <p className="text-xs text-zinc-500">
              Confirm by typing the exact `pool_id`. This step writes an audit record and submits the signed
              transaction from the selected hot machine.
            </p>
            <button
              type="button"
              onClick={() => void handleSubmit()}
              disabled={submitting || !canSubmit}
              className="rounded-md border border-emerald-700/70 bg-emerald-900/30 px-4 py-2 text-sm font-medium text-emerald-200 disabled:cursor-not-allowed disabled:opacity-70"
            >
              {submitting ? "Submitting..." : "Submit Registration"}
            </button>
          </div>

          {submitResult && (
            <div className="mt-4 rounded-md border border-emerald-900/40 bg-emerald-950/20 px-3 py-3 text-sm text-emerald-200">
              <p className="font-medium">Registration submitted</p>
              <dl className="mt-3 grid gap-2 md:grid-cols-2">
                <div>
                  <dt className="text-emerald-400/80">Transaction Hash</dt>
                  <dd className="mt-1 break-all text-zinc-100">{submitResult.tx_hash ?? "--"}</dd>
                </div>
                <div>
                  <dt className="text-emerald-400/80">Signed Tx Path</dt>
                  <dd className="mt-1 break-all text-zinc-100">{submitResult.tx_signed_path ?? "--"}</dd>
                </div>
              </dl>
              <p className="mt-3 text-xs text-emerald-200/90">{submitResult.note}</p>
            </div>
          )}
        </article>
      )}
    </section>
  );
}
