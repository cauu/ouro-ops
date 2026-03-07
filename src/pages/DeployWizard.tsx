import { useEffect, useMemo, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import TaskLogStream from "../components/TaskLogStream";
import { deployCancel, deployStart, deployStatus, machineList, machineRuntimeProbe } from "../lib/ipc";
import type { DeployPayload, DeployTaskStatus, Machine, Pool, RuntimeProbe } from "../lib/types";

interface DeployWizardProps {
  pool: Pool;
}

function isTerminal(status: string): boolean {
  return status === "success" || status === "failed" || status === "cancelled";
}

function networkSupportsMithril(network: Pool["network"]): boolean {
  return network === "mainnet" || network === "preprod";
}

export default function DeployWizard({ pool }: DeployWizardProps) {
  const [loading, setLoading] = useState(true);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [step, setStep] = useState(1);
  const [selectedMachineIds, setSelectedMachineIds] = useState<number[]>([]);

  const [cardanoVersion, setCardanoVersion] = useState("10.5.4-1");
  const [imageRegistry, setImageRegistry] = useState("ghcr.io/blinklabs-io/cardano-node");
  const [network, setNetwork] = useState<Pool["network"]>(pool.network);
  const [enableSwap, setEnableSwap] = useState(true);
  const [swapSizeGb, setSwapSizeGb] = useState(8);
  const [enableChrony, setEnableChrony] = useState(true);
  const [enableHardening, setEnableHardening] = useState(true);
  const [safeValidationMode, setSafeValidationMode] = useState(false);
  const [takeoverExistingNode, setTakeoverExistingNode] = useState(false);
  const [restoreSnapshot, setRestoreSnapshot] = useState(networkSupportsMithril(pool.network));
  const [restoreSnapshotTouched, setRestoreSnapshotTouched] = useState(false);
  const [runtimeProbeMap, setRuntimeProbeMap] = useState<Record<number, RuntimeProbe>>({});
  const [probingRuntime, setProbingRuntime] = useState(false);

  const [showConfirm, setShowConfirm] = useState(false);
  const [starting, setStarting] = useState(false);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [taskStatus, setTaskStatus] = useState<DeployTaskStatus | null>(null);
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    const load = async () => {
      setLoading(true);
      setError(null);
      try {
        const rows = await machineList();
        setMachines(rows);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    };
    void load();
  }, []);

  useEffect(() => {
    if (!taskId) {
      return;
    }
    let active = true;
    const timer = setInterval(() => {
      void deployStatus(taskId)
        .then((status) => {
          if (!active) {
            return;
          }
          setTaskStatus(status);
          if (isTerminal(status.status)) {
            clearInterval(timer);
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
      clearInterval(timer);
    };
  }, [taskId]);

  const selectedMachines = useMemo(
    () => machines.filter((m) => selectedMachineIds.includes(m.id)),
    [machines, selectedMachineIds],
  );
  const mithrilSupported = useMemo(() => networkSupportsMithril(network), [network]);
  const selectedWithRuntime = useMemo(
    () => selectedMachines.filter((m) => runtimeProbeMap[m.id]?.container_present),
    [selectedMachines, runtimeProbeMap],
  );

  const canNextFromStep1 = selectedMachineIds.length > 0;
  const canNextFromStep2 = cardanoVersion.trim().length > 0 && swapSizeGb >= 8 && swapSizeGb <= 16;

  const toggleMachine = (machineId: number) => {
    setSelectedMachineIds((prev) =>
      prev.includes(machineId) ? prev.filter((id) => id !== machineId) : [...prev, machineId],
    );
  };

  const buildPayload = (): DeployPayload => ({
    machine_ids: selectedMachineIds,
    cardano_version: cardanoVersion.trim(),
    image_registry: imageRegistry.trim(),
    network,
    enable_swap: enableSwap,
    swap_size_gb: swapSizeGb,
    enable_chrony: enableChrony,
    enable_hardening: enableHardening,
    safe_validation_mode: safeValidationMode,
    takeover_existing_node: takeoverExistingNode,
    restore_snapshot: restoreSnapshot,
  });

  useEffect(() => {
    if (!mithrilSupported) {
      setRestoreSnapshot(false);
      setRestoreSnapshotTouched(false);
      return;
    }
    if (!restoreSnapshotTouched) {
      setRestoreSnapshot(true);
    }
  }, [mithrilSupported, restoreSnapshotTouched]);

  useEffect(() => {
    let active = true;
    const runProbe = async () => {
      if (selectedMachineIds.length === 0) {
        setRuntimeProbeMap({});
        setTakeoverExistingNode(false);
        return;
      }
      setProbingRuntime(true);
      const pairs = await Promise.all(
        selectedMachineIds.map(async (machineId) => {
          try {
            const probe = await machineRuntimeProbe(machineId);
            return [machineId, probe] as const;
          } catch {
            return [machineId, undefined] as const;
          }
        }),
      );
      if (!active) {
        return;
      }
      const next: Record<number, RuntimeProbe> = {};
      pairs.forEach(([machineId, probe]) => {
        if (probe) {
          next[machineId] = probe;
        }
      });
      setRuntimeProbeMap(next);
      if (!Object.values(next).some((probe) => probe.container_present)) {
        setTakeoverExistingNode(false);
      }
      setProbingRuntime(false);
    };
    void runProbe();
    return () => {
      active = false;
    };
  }, [selectedMachineIds]);

  const handleStart = async () => {
    setStarting(true);
    setError(null);
    try {
      const createdTaskId = await deployStart(buildPayload());
      setTaskId(createdTaskId);
      const status = await deployStatus(createdTaskId);
      setTaskStatus(status);
      setShowConfirm(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  };

  const handleCancel = async () => {
    if (!taskId) {
      return;
    }
    setCancelling(true);
    setError(null);
    try {
      await deployCancel(taskId);
      const status = await deployStatus(taskId);
      setTaskStatus(status);
    } catch (e) {
      setError(String(e));
    } finally {
      setCancelling(false);
    }
  };

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Deploy Wizard</h1>
        <p className="mt-1 text-sm text-zinc-400">step === {step} · Pool network: {pool.network}</p>
      </header>

      {error && (
        <p className="rounded-md border border-red-700/60 bg-red-900/20 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      )}

      {loading ? (
        <p className="text-sm text-zinc-400">Loading machines...</p>
      ) : (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
          {step === 1 && (
            <div className="space-y-3">
              <h2 className="text-lg font-medium">1. Select Machines</h2>
              {machines.length === 0 ? (
                <p className="text-sm text-zinc-400">No machines available.</p>
              ) : (
                <div className="space-y-2">
                  {machines.map((machine) => (
                    <label key={machine.id} className="flex items-center gap-2 rounded-md border border-zinc-800 p-2">
                      <input
                        type="checkbox"
                        checked={selectedMachineIds.includes(machine.id)}
                        onChange={() => toggleMachine(machine.id)}
                      />
                      <span className="text-sm">
                        {machine.name} ({machine.role}) · {machine.ip}:{machine.port}
                      </span>
                    </label>
                  ))}
                </div>
              )}
            </div>
          )}

          {step === 2 && (
            <div className="space-y-3">
              <h2 className="text-lg font-medium">2. Configure Parameters</h2>
              {probingRuntime && (
                <p className="rounded-md border border-zinc-700 bg-zinc-900 px-3 py-2 text-xs text-zinc-300">
                  Probing runtime containers on selected machines...
                </p>
              )}
              {selectedWithRuntime.length > 0 && (
                <div className="rounded-md border border-amber-700/60 bg-amber-950/20 p-3 text-xs text-amber-200">
                  <p>
                    Detected running <code>cardano-node</code> on:{" "}
                    {selectedWithRuntime.map((m) => m.name).join(", ")}
                  </p>
                  <p className="mt-1">
                    Enabling takeover will migrate DB/keys to <code>/opt/cardano/*</code> and switch to app-managed
                    runtime with automatic rollback on health-check failure.
                  </p>
                </div>
              )}
              <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                <input
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
                  value={cardanoVersion}
                  onChange={(e) => setCardanoVersion(e.target.value)}
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  placeholder="cardano version"
                />
                <input
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
                  value={imageRegistry}
                  onChange={(e) => setImageRegistry(e.target.value)}
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  placeholder="image registry"
                />
                <select
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
                  value={network}
                  onChange={(e) => setNetwork(e.target.value as Pool["network"])}
                >
                  <option value="mainnet">mainnet</option>
                  <option value="preprod">preprod</option>
                  <option value="preview">preview</option>
                </select>
                <input
                  type="number"
                  min={8}
                  max={16}
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm"
                  value={swapSizeGb}
                  onChange={(e) => setSwapSizeGb(Number(e.target.value))}
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                />
                <label className="flex items-center gap-2 text-sm">
                  <input type="checkbox" checked={enableSwap} onChange={(e) => setEnableSwap(e.target.checked)} />
                  enable_swap
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={enableChrony}
                    onChange={(e) => setEnableChrony(e.target.checked)}
                  />
                  enable_chrony
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={enableHardening}
                    onChange={(e) => setEnableHardening(e.target.checked)}
                  />
                  enable_hardening
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={safeValidationMode}
                    onChange={(e) => setSafeValidationMode(e.target.checked)}
                  />
                  safe_validation_mode (read-only)
                </label>
                {selectedWithRuntime.length > 0 && (
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={takeoverExistingNode}
                      onChange={(e) => setTakeoverExistingNode(e.target.checked)}
                    />
                    takeover_existing_node
                  </label>
                )}
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={restoreSnapshot}
                    disabled={!mithrilSupported}
                    onChange={(e) => {
                      setRestoreSnapshot(e.target.checked);
                      setRestoreSnapshotTouched(true);
                    }}
                  />
                  restore_snapshot (Mithril cold-start restore)
                </label>
              </div>
              <p className="text-xs text-zinc-400">
                Default is enabled for <code>mainnet</code>/<code>preprod</code> cold-starts and disabled for{" "}
                <code>preview</code>. Existing DB will be handled by deploy-side checks.
              </p>
            </div>
          )}

          {step === 3 && (
            <div className="space-y-3">
              <h2 className="text-lg font-medium">3. Confirm</h2>
              <p className="text-sm text-zinc-300">
                Machines: {selectedMachines.map((m) => m.name).join(", ") || "-"}
              </p>
              <p className="text-sm text-zinc-300">Version: {cardanoVersion}</p>
              <p className="text-sm text-zinc-300">Network: {network}</p>
              <p className="text-sm text-zinc-300">Image: {imageRegistry} (default tag: 10.5.4-1, overridable)</p>
              <p className="text-sm text-zinc-300">
                swap={String(enableSwap)} ({swapSizeGb}G) · chrony={String(enableChrony)} · hardening=
                {String(enableHardening)}
              </p>
              <p className="text-sm text-zinc-300">safe_validation_mode={String(safeValidationMode)}</p>
              <p className="text-sm text-zinc-300">takeover_existing_node={String(takeoverExistingNode)}</p>
              <p className="text-sm text-zinc-300">restore_snapshot={String(restoreSnapshot)}</p>
              <button
                type="button"
                onClick={() => setShowConfirm(true)}
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
              >
                Execute Deploy
              </button>
              {safeValidationMode && (
                <p className="text-xs text-yellow-300">
                  Safe validation mode only runs read-only checks and will not change host config or restart cardano-node.
                </p>
              )}
            </div>
          )}

          <div className="mt-4 flex gap-2">
            <button
              type="button"
              onClick={() => setStep((v) => Math.max(1, v - 1))}
              disabled={step === 1}
              className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm disabled:opacity-50"
            >
              Back
            </button>
            <button
              type="button"
              onClick={() => setStep((v) => Math.min(3, v + 1))}
              disabled={(step === 1 && !canNextFromStep1) || (step === 2 && !canNextFromStep2) || step === 3}
              className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm disabled:opacity-50"
            >
              Next
            </button>
          </div>
        </div>
      )}

      {taskId && (
        <section className="space-y-3">
          <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-3 text-sm">
            <p>TaskId: {taskId}</p>
            <p>Status: {taskStatus?.status ?? "pending"}</p>
            {taskStatus?.error_msg && <p className="text-red-300">Error: {taskStatus.error_msg}</p>}
            {(taskStatus?.status === "running" || taskStatus?.status === "pending") && (
              <button
                type="button"
                onClick={() => void handleCancel()}
                disabled={cancelling}
                className="mt-2 rounded-md border border-red-700/70 px-3 py-1 text-xs text-red-300 hover:bg-red-950/40 disabled:opacity-60"
              >
                {cancelling ? "Cancelling..." : "Cancel Deploy"}
              </button>
            )}
          </div>
          <TaskLogStream taskId={taskId} />
        </section>
      )}

      <ConfirmModal
        open={showConfirm}
        level="standard"
        title="Confirm Deployment"
        description={
          safeValidationMode
            ? "This action will run read-only safe validation without modifying target hosts."
            : "This action will start deploy_start(payload) and execute playbook on selected machines."
        }
        confirmLabel={starting ? "Starting..." : "Start Deploy"}
        onCancel={() => setShowConfirm(false)}
        onConfirm={() => {
          if (!starting) {
            void handleStart();
          }
        }}
      />
    </section>
  );
}
