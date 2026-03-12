import { useEffect, useMemo, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import TaskLogStream from "../components/TaskLogStream";
import { formatTaskError, toUserError } from "../lib/errors";
import {
  deployCancel,
  deployStart,
  deployStatus,
  machineAdd,
  machineRemove,
  machineRuntimeProbe,
  sshAgentAddKey,
  sshAgentListKeys,
} from "../lib/ipc";
import type {
  DeployPayload,
  DeployTaskStatus,
  Machine,
  MachineAddPayload,
  Pool,
  RuntimeProbe,
  SshKeyInfo,
} from "../lib/types";

interface DeployWizardProps {
  pool: Pool;
}

function isTerminal(status: string): boolean {
  return status === "success" || status === "failed" || status === "cancelled";
}

function networkSupportsMithril(network: Pool["network"]): boolean {
  return network === "mainnet" || network === "preprod";
}

interface MachineDraft {
  name: string;
  ip: string;
  port: string;
  sshUser: string;
  sshKeyFingerprint: string;
}

function emptyMachineDraft(role: MachineAddPayload["role"], index: number, keyFingerprint = ""): MachineDraft {
  return {
    name: role === "bp" ? "bp-1" : `relay-${index + 1}`,
    ip: "",
    port: "22",
    sshUser: "root",
    sshKeyFingerprint: keyFingerprint,
  };
}

export default function DeployWizard({ pool }: DeployWizardProps) {
  const defaultRelayRestore = false;
  const defaultBpRestore = false;
  const mithrilInitializationAllowed = false;
  const [machines, setMachines] = useState<Machine[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [step, setStep] = useState(1);
  const [step1Completed, setStep1Completed] = useState(false);
  const [creatingStep1, setCreatingStep1] = useState(false);
  const [selectedMachineIds, setSelectedMachineIds] = useState<number[]>([]);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const [addingKey, setAddingKey] = useState(false);
  const [keyPath, setKeyPath] = useState("~/.ssh/id_ed25519");
  const [bpDraft, setBpDraft] = useState<MachineDraft>(emptyMachineDraft("bp", 0));
  const [relayDrafts, setRelayDrafts] = useState<MachineDraft[]>([emptyMachineDraft("relay", 0)]);

  const [cardanoVersion, setCardanoVersion] = useState("10.5.4-1");
  const [imageRegistry, setImageRegistry] = useState("ghcr.io/blinklabs-io/cardano-node");
  const [network, setNetwork] = useState<Pool["network"]>(pool.network);
  const [enableSwap, setEnableSwap] = useState(true);
  const [swapSizeGb, setSwapSizeGb] = useState(8);
  const [enableChrony, setEnableChrony] = useState(true);
  const [enableHardening, setEnableHardening] = useState(true);
  const [safeValidationMode, setSafeValidationMode] = useState(false);
  const [takeoverExistingNode, setTakeoverExistingNode] = useState(false);
  const [restoreSnapshotRelay, setRestoreSnapshotRelay] = useState(defaultRelayRestore);
  const [restoreSnapshotBp, setRestoreSnapshotBp] = useState(defaultBpRestore);
  const [restoreSnapshotTouched, setRestoreSnapshotTouched] = useState(false);
  const [runtimeProbeMap, setRuntimeProbeMap] = useState<Record<number, RuntimeProbe>>({});
  const [probingRuntime, setProbingRuntime] = useState(false);
  const [runtimeProbeStatus, setRuntimeProbeStatus] = useState<string | null>(null);

  const [showConfirm, setShowConfirm] = useState(false);
  const [starting, setStarting] = useState(false);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [taskStatus, setTaskStatus] = useState<DeployTaskStatus | null>(null);
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    void (async () => {
      setError(null);
      try {
        const loadedKeys = await sshAgentListKeys();
        setKeys(loadedKeys);
        const defaultFingerprint = loadedKeys[0]?.fingerprint ?? "";
        if (defaultFingerprint) {
          setBpDraft((prev) =>
            prev.sshKeyFingerprint ? prev : { ...prev, sshKeyFingerprint: defaultFingerprint },
          );
          setRelayDrafts((prev) =>
            prev.map((draft) =>
              draft.sshKeyFingerprint ? draft : { ...draft, sshKeyFingerprint: defaultFingerprint },
            ),
          );
        }
      } catch (e) {
        setError(toUserError(e));
      }
    })();
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
            setError(toUserError(e));
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
  const selectedRelayCount = useMemo(
    () => selectedMachines.filter((m) => m.role === "relay").length,
    [selectedMachines],
  );
  const selectedBpCount = useMemo(
    () => selectedMachines.filter((m) => m.role === "bp").length,
    [selectedMachines],
  );
  const mithrilSupported = useMemo(() => networkSupportsMithril(network), [network]);
  const selectedWithRuntime = useMemo(
    () => selectedMachines.filter((m) => runtimeProbeMap[m.id]?.container_present),
    [selectedMachines, runtimeProbeMap],
  );

  const canNextFromStep1 =
    step1Completed ||
    (keys.length > 0 &&
      bpDraft.ip.trim().length > 0 &&
      relayDrafts.length > 0 &&
      relayDrafts.every((draft) => draft.ip.trim().length > 0));
  const canNextFromStep2 = cardanoVersion.trim().length > 0 && swapSizeGb >= 8 && swapSizeGb <= 16;

  const updateRelayDraft = (index: number, patch: Partial<MachineDraft>) => {
    setRelayDrafts((prev) =>
      prev.map((draft, current) => (current === index ? { ...draft, ...patch } : draft)),
    );
  };

  const addRelayDraft = () => {
    const fallbackFingerprint = keys[0]?.fingerprint ?? "";
    setRelayDrafts((prev) => [...prev, emptyMachineDraft("relay", prev.length, fallbackFingerprint)]);
  };

  const removeRelayDraft = (index: number) => {
    setRelayDrafts((prev) => {
      if (prev.length <= 1) {
        return prev;
      }
      const next = prev.filter((_, current) => current !== index);
      return next.map((draft, current) => ({
        ...draft,
        name: draft.name.startsWith("relay-") ? `relay-${current + 1}` : draft.name,
      }));
    });
  };

  const handleAddKey = async () => {
    if (!keyPath.trim()) {
      return;
    }
    setAddingKey(true);
    setError(null);
    try {
      const updatedKeys = await sshAgentAddKey(keyPath.trim());
      setKeys(updatedKeys);
      const defaultFingerprint = updatedKeys[0]?.fingerprint ?? "";
      if (defaultFingerprint) {
        setBpDraft((prev) =>
          prev.sshKeyFingerprint ? prev : { ...prev, sshKeyFingerprint: defaultFingerprint },
        );
        setRelayDrafts((prev) =>
          prev.map((draft) =>
            draft.sshKeyFingerprint ? draft : { ...draft, sshKeyFingerprint: defaultFingerprint },
          ),
        );
      }
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setAddingKey(false);
    }
  };

  const validateDraft = (draft: MachineDraft, label: string): string | null => {
    if (!draft.ip.trim()) {
      return `${label}: IP is required.`;
    }
    const port = Number(draft.port);
    if (!Number.isInteger(port) || port <= 0) {
      return `${label}: SSH port must be a positive integer.`;
    }
    if (!draft.sshUser.trim()) {
      return `${label}: SSH user is required.`;
    }
    if (!draft.sshKeyFingerprint.trim()) {
      return `${label}: SSH key fingerprint is required.`;
    }
    return null;
  };

  const handlePersistStep1 = async () => {
    if (step1Completed) {
      setStep(2);
      return;
    }
    setError(null);
    if (keys.length === 0) {
      setError("No ssh-agent key available. Add a key before creating nodes.");
      return;
    }

    const bpValidation = validateDraft(bpDraft, "BP");
    if (bpValidation) {
      setError(bpValidation);
      return;
    }
    for (let index = 0; index < relayDrafts.length; index += 1) {
      const validation = validateDraft(relayDrafts[index], `Relay #${index + 1}`);
      if (validation) {
        setError(validation);
        return;
      }
    }

    const payloads: MachineAddPayload[] = [
      {
        name: bpDraft.name.trim() || "bp-1",
        ip: bpDraft.ip.trim(),
        port: Number(bpDraft.port),
        ssh_user: bpDraft.sshUser.trim(),
        role: "bp",
        network: pool.network,
        ssh_key_fingerprint: bpDraft.sshKeyFingerprint.trim(),
      },
      ...relayDrafts.map((draft, index) => ({
        name: draft.name.trim() || `relay-${index + 1}`,
        ip: draft.ip.trim(),
        port: Number(draft.port),
        ssh_user: draft.sshUser.trim(),
        role: "relay" as const,
        network: pool.network,
        ssh_key_fingerprint: draft.sshKeyFingerprint.trim(),
      })),
    ];

    setCreatingStep1(true);
    const createdIds: number[] = [];
    try {
      const createdMachines: Machine[] = [];
      for (const payload of payloads) {
        const created = await machineAdd(payload);
        createdMachines.push(created);
        createdIds.push(created.id);
      }
      setMachines(createdMachines);
      setSelectedMachineIds(createdMachines.map((machine) => machine.id));
      setStep1Completed(true);
      setStep(2);
    } catch (e) {
      await Promise.all(
        createdIds.map(async (machineId) => {
          try {
            await machineRemove(machineId);
          } catch {
            // best effort cleanup to avoid partial node creation during step-1 failure
          }
        }),
      );
      setError(toUserError(e));
    } finally {
      setCreatingStep1(false);
    }
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
    restore_snapshot_relay: mithrilInitializationAllowed ? restoreSnapshotRelay : false,
    restore_snapshot_bp: mithrilInitializationAllowed ? restoreSnapshotBp : false,
  });

  useEffect(() => {
    if (!mithrilInitializationAllowed) {
      setRestoreSnapshotRelay(false);
      setRestoreSnapshotBp(false);
      setRestoreSnapshotTouched(false);
      return;
    }
    if (!mithrilSupported) {
      setRestoreSnapshotRelay(false);
      setRestoreSnapshotBp(false);
      setRestoreSnapshotTouched(false);
      return;
    }
    if (!restoreSnapshotTouched) {
      setRestoreSnapshotRelay(true);
      setRestoreSnapshotBp(true);
    }
  }, [mithrilInitializationAllowed, mithrilSupported, restoreSnapshotTouched]);

  useEffect(() => {
    let active = true;
    const runProbe = async () => {
      if (selectedMachineIds.length === 0) {
        setRuntimeProbeMap({});
        setTakeoverExistingNode(false);
        setRuntimeProbeStatus(null);
        return;
      }
      setProbingRuntime(true);
      setRuntimeProbeStatus(`Resolving runtime state (0/${selectedMachineIds.length})...`);
      let completed = 0;
      const pairs = await Promise.all(
        selectedMachineIds.map(async (machineId) => {
          try {
            const probe = await machineRuntimeProbe(machineId);
            return [machineId, probe] as const;
          } catch {
            return [machineId, undefined] as const;
          } finally {
            completed += 1;
            if (active) {
              setRuntimeProbeStatus(`Resolving runtime state (${completed}/${selectedMachineIds.length})...`);
            }
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
      setRuntimeProbeStatus(`Resolved runtime state for ${Object.keys(next).length}/${selectedMachineIds.length} machines.`);
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
      setError(toUserError(e));
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
      setError(toUserError(e));
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

      <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
        {step === 1 && (
          <div className="space-y-4">
            <div>
              <h2 className="text-lg font-medium">1. Configure Nodes</h2>
              <p className="mt-1 text-sm text-zinc-400">
                Enter BP and relay nodes manually. Moving to step 2 will create these machines for the current pool.
              </p>
            </div>

            {!step1Completed && (
              <section className="space-y-3 rounded-md border border-zinc-800 bg-zinc-950/40 p-3">
                <div className="flex items-center justify-between gap-3">
                  <h3 className="text-sm font-medium text-zinc-100">SSH Agent Key</h3>
                  <span className="text-xs text-zinc-500">{keys.length} key(s)</span>
                </div>
                {keys.length === 0 ? (
                  <div className="space-y-2">
                    <p className="rounded-md border border-amber-700/60 bg-amber-950/20 px-3 py-2 text-xs text-amber-200">
                      No key loaded in ssh-agent. Add one before creating nodes.
                    </p>
                    <div className="flex flex-col gap-2 md:flex-row">
                      <input
                        value={keyPath}
                        onChange={(event) => setKeyPath(event.target.value)}
                        placeholder="~/.ssh/id_ed25519"
                        autoCapitalize="none"
                        autoCorrect="off"
                        spellCheck={false}
                        className="flex-1 rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                      />
                      <button
                        type="button"
                        onClick={() => void handleAddKey()}
                        disabled={addingKey || !keyPath.trim()}
                        className="rounded-md border border-zinc-700 px-3 py-2 text-sm text-zinc-200 hover:border-zinc-500 disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        {addingKey ? "Adding key..." : "Add key to ssh-agent"}
                      </button>
                    </div>
                  </div>
                ) : (
                  <p className="text-xs text-zinc-400">
                    Choose one fingerprint per node. Keys are resolved from local ssh-agent.
                  </p>
                )}
              </section>
            )}

            <section className="space-y-3 rounded-md border border-zinc-800 bg-zinc-950/40 p-3">
              <h3 className="text-sm font-medium text-zinc-100">BP Node</h3>
              <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                <input
                  value={bpDraft.name}
                  onChange={(event) => setBpDraft((prev) => ({ ...prev, name: event.target.value }))}
                  placeholder="bp-1"
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                />
                <input
                  value={bpDraft.ip}
                  onChange={(event) => setBpDraft((prev) => ({ ...prev, ip: event.target.value }))}
                  placeholder="BP IP"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                />
                <input
                  value={bpDraft.sshUser}
                  onChange={(event) => setBpDraft((prev) => ({ ...prev, sshUser: event.target.value }))}
                  placeholder="SSH user"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                />
                <input
                  value={bpDraft.port}
                  onChange={(event) => setBpDraft((prev) => ({ ...prev, port: event.target.value }))}
                  placeholder="22"
                  inputMode="numeric"
                  className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                />
                <select
                  value={bpDraft.sshKeyFingerprint}
                  onChange={(event) =>
                    setBpDraft((prev) => ({ ...prev, sshKeyFingerprint: event.target.value }))
                  }
                  className="md:col-span-2 rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                >
                  <option value="">Select ssh-agent fingerprint</option>
                  {keys.map((key) => (
                    <option key={key.fingerprint} value={key.fingerprint}>
                      {key.fingerprint}
                    </option>
                  ))}
                </select>
              </div>
            </section>

            <section className="space-y-3 rounded-md border border-zinc-800 bg-zinc-950/40 p-3">
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-medium text-zinc-100">Relay Nodes</h3>
                <button
                  type="button"
                  onClick={addRelayDraft}
                  disabled={step1Completed}
                  className="rounded-md border border-zinc-700 px-2 py-1 text-xs text-zinc-200 hover:border-zinc-500 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  + Add relay
                </button>
              </div>
              {relayDrafts.map((draft, index) => (
                <div key={`relay-draft-${index}`} className="rounded-md border border-zinc-800 bg-black/20 p-3">
                  <div className="mb-3 flex items-center justify-between">
                    <p className="text-xs font-medium uppercase tracking-wide text-zinc-400">Relay #{index + 1}</p>
                    <button
                      type="button"
                      onClick={() => removeRelayDraft(index)}
                      disabled={relayDrafts.length <= 1 || step1Completed}
                      className="rounded-md border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:border-zinc-500 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      Remove
                    </button>
                  </div>
                  <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                    <input
                      value={draft.name}
                      onChange={(event) => updateRelayDraft(index, { name: event.target.value })}
                      placeholder={`relay-${index + 1}`}
                      className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    />
                    <input
                      value={draft.ip}
                      onChange={(event) => updateRelayDraft(index, { ip: event.target.value })}
                      placeholder="Relay IP"
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                      className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    />
                    <input
                      value={draft.sshUser}
                      onChange={(event) => updateRelayDraft(index, { sshUser: event.target.value })}
                      placeholder="SSH user"
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                      className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    />
                    <input
                      value={draft.port}
                      onChange={(event) => updateRelayDraft(index, { port: event.target.value })}
                      placeholder="22"
                      inputMode="numeric"
                      className="rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    />
                    <select
                      value={draft.sshKeyFingerprint}
                      onChange={(event) =>
                        updateRelayDraft(index, { sshKeyFingerprint: event.target.value })
                      }
                      className="md:col-span-2 rounded-md border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
                    >
                      <option value="">Select ssh-agent fingerprint</option>
                      {keys.map((key) => (
                        <option key={`${index}-${key.fingerprint}`} value={key.fingerprint}>
                          {key.fingerprint}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>
              ))}
            </section>

            {step1Completed && machines.length > 0 && (
              <div className="rounded-md border border-emerald-700/40 bg-emerald-950/20 px-3 py-2 text-xs text-emerald-200">
                Step 1 completed. {machines.length} machine(s) created for this deploy draft.
              </div>
            )}
          </div>
        )}

          {step === 2 && (
            <div className="space-y-3">
              <h2 className="text-lg font-medium">2. Configure Parameters</h2>
              {probingRuntime && (
                <p className="rounded-md border border-zinc-700 bg-zinc-900 px-3 py-2 text-xs text-zinc-300">
                  {runtimeProbeStatus ?? "Probing runtime containers on selected machines..."}
                </p>
              )}
              {!probingRuntime && runtimeProbeStatus && (
                <p className="rounded-md border border-zinc-800 bg-zinc-950/50 px-3 py-2 text-xs text-zinc-400">
                  {runtimeProbeStatus}
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
                    checked={restoreSnapshotRelay}
                    disabled={!mithrilInitializationAllowed || !mithrilSupported || selectedRelayCount === 0}
                    onChange={(e) => {
                      setRestoreSnapshotRelay(e.target.checked);
                      setRestoreSnapshotTouched(true);
                    }}
                  />
                  restore_snapshot_relay (Mithril cold-start restore)
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={restoreSnapshotBp}
                    disabled={!mithrilInitializationAllowed || !mithrilSupported || selectedBpCount === 0}
                    onChange={(e) => {
                      setRestoreSnapshotBp(e.target.checked);
                      setRestoreSnapshotTouched(true);
                    }}
                  />
                  restore_snapshot_bp (Mithril cold-start restore)
                </label>
              </div>
              <p className="text-xs text-zinc-400">
                Mithril cold-start restore is disabled in this mac app stage to avoid long initialization time. Keep
                <code> restore_snapshot_relay/bp </code>
                off unless this policy is explicitly changed later.
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
              <p className="text-sm text-zinc-300">restore_snapshot_relay={String(restoreSnapshotRelay)}</p>
              <p className="text-sm text-zinc-300">restore_snapshot_bp={String(restoreSnapshotBp)}</p>
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
            disabled={step === 1 || creatingStep1}
            className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm disabled:opacity-50"
          >
            Back
          </button>
          <button
            type="button"
            onClick={() => {
              if (step === 1) {
                void handlePersistStep1();
                return;
              }
              setStep((v) => Math.min(3, v + 1));
            }}
            disabled={
              creatingStep1 ||
              (step === 1 && !canNextFromStep1) ||
              (step === 2 && !canNextFromStep2) ||
              step === 3
            }
            className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm disabled:opacity-50"
          >
            {step === 1 && creatingStep1 ? "Creating nodes..." : "Next"}
          </button>
        </div>
      </div>

      {taskId && (
        <section className="space-y-3">
          <div className="rounded-lg border border-zinc-800 bg-zinc-900/50 p-3 text-sm">
            <p>TaskId: {taskId}</p>
            <p>Status: {taskStatus?.status ?? "pending"}</p>
            {formatTaskError(taskStatus?.error_msg) && (
              <p className="text-red-300">Error: {formatTaskError(taskStatus?.error_msg)}</p>
            )}
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
