import { useEffect, useMemo, useState } from "react";
import TaskLogStream from "../components/TaskLogStream";
import { formatTaskError, toUserError } from "../lib/errors";
import { useMonitorStore } from "../lib/monitorStore";
import {
  kesGenerate,
  kesImportCert,
  kesPrepareBundle,
  kesPushStart,
  kesRotationStatus,
  kesStatusAll,
} from "../lib/ipc";
import type { DeployTaskStatus, KesBundleResult, KesSignRequest, KesStatus } from "../lib/types";

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
  const { snapshots } = useMonitorStore();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<KesStatus[]>([]);
  const [requests, setRequests] = useState<Record<number, KesSignRequest>>({});
  const [certPaths, setCertPaths] = useState<Record<number, string>>({});
  const [rotationTasks, setRotationTasks] = useState<Record<number, DeployTaskStatus>>({});
  const [busyMachineId, setBusyMachineId] = useState<number | null>(null);
  const [pushConfirmMachineId, setPushConfirmMachineId] = useState<number | null>(null);
  const [pushConfirmValue, setPushConfirmValue] = useState("");
  const [selectedMachineId, setSelectedMachineId] = useState<number | null>(null);
  const [wizardStep, setWizardStep] = useState(1);
  const [bundleIncludeCli, setBundleIncludeCli] = useState(false);
  const [bundlePlatform, setBundlePlatform] = useState<string>("linux-x86_64");
  const [bundleResult, setBundleResult] = useState<KesBundleResult | null>(null);
  const [bundleBusy, setBundleBusy] = useState(false);

  const selectedSnapshot = useMemo(
    () => (selectedMachineId != null ? snapshots.find((s) => s.machine_id === selectedMachineId) ?? null : null),
    [snapshots, selectedMachineId],
  );

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

  useEffect(() => {
    if (sortedStatuses.length === 0) {
      setSelectedMachineId(null);
      return;
    }
    if (selectedMachineId == null || !sortedStatuses.some((row) => row.machine_id === selectedMachineId)) {
      setSelectedMachineId(sortedStatuses[0].machine_id);
    }
  }, [selectedMachineId, sortedStatuses]);

  const selectedStatus = useMemo(
    () => sortedStatuses.find((row) => row.machine_id === selectedMachineId) ?? null,
    [selectedMachineId, sortedStatuses],
  );

  const displaySeverity = useMemo(() => {
    const approxDays =
      selectedSnapshot?.kes_remaining_periods != null
        ? selectedSnapshot.kes_remaining_periods * 1.5
        : null;
    if (approxDays != null) {
      if (approxDays > 10) return "healthy";
      if (approxDays >= 3) return "warning";
      return "critical";
    }
    return selectedStatus?.severity ?? "warning";
  }, [selectedSnapshot?.kes_remaining_periods, selectedStatus?.severity]);

  const selectedRequest = useMemo(
    () => (selectedMachineId == null ? undefined : requests[selectedMachineId]),
    [requests, selectedMachineId],
  );

  const selectedTask = useMemo(
    () => (selectedMachineId == null ? undefined : rotationTasks[selectedMachineId]),
    [rotationTasks, selectedMachineId],
  );

  useEffect(() => {
    if (selectedMachineId == null) {
      setWizardStep(1);
      return;
    }
    if (selectedTask && isTerminal(selectedTask.status)) {
      setWizardStep(4);
      return;
    }
    if (selectedTask) {
      setWizardStep(3);
      return;
    }
    if (selectedRequest) {
      setWizardStep(2);
      return;
    }
    setWizardStep(1);
  }, [selectedMachineId]);

  useEffect(() => {
    if (selectedMachineId == null) {
      return;
    }
    if (selectedTask && isTerminal(selectedTask.status) && wizardStep < 4) {
      setWizardStep(4);
      return;
    }
    if (selectedTask && wizardStep < 3) {
      setWizardStep(3);
      return;
    }
    if (selectedRequest && wizardStep < 2) {
      setWizardStep(2);
    }
  }, [selectedMachineId, selectedRequest, selectedTask, wizardStep]);

  const handlePrepareBundle = async (machineId: number) => {
    setBundleBusy(true);
    setBundleResult(null);
    setError(null);
    try {
      const result = await kesPrepareBundle(
        machineId,
        bundleIncludeCli,
        bundleIncludeCli ? bundlePlatform : null,
      );
      setBundleResult(result);
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setBundleBusy(false);
    }
  };

  const handleGenerate = async (machineId: number) => {
    setBusyMachineId(machineId);
    setBundleResult(null);
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
  const selectedBusy = selectedMachineId != null && busyMachineId === selectedMachineId;
  const canPush = selectedTask?.status === "pending";
  const pushConfirmArmed = selectedMachineId != null && pushConfirmMachineId === selectedMachineId;
  const pushUnlocked = pushConfirmValue.trim() === normalizedTicker;
  const selectedTaskError = formatTaskError(selectedTask?.error_msg);

  return (
    <section className="space-y-5">
      <header className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight text-slate-900">KES Rotate</h1>
        <p className="text-sm text-slate-600">Step 1-4 向导：生成 keypairs、离线签发 cert、执行 rotate、完成校验。</p>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          {[1, 2, 3, 4].map((step) => (
            <span
              key={`kes-step-${step}`}
              className={`rounded-full border px-2.5 py-1 ${
                wizardStep === step
                  ? "border-blue-300 bg-blue-50 font-semibold text-blue-700"
                  : wizardStep > step
                    ? "border-emerald-300 bg-emerald-50 text-emerald-700"
                    : "border-slate-300 bg-slate-50 text-slate-600"
              }`}
            >
              {step === 1
                ? "1 生成 KES keypairs"
                : step === 2
                  ? "2 离线生成 cert"
                  : step === 3
                    ? "3 上传并执行 Rotate"
                    : "4 校验完成"}
            </span>
          ))}
        </div>
      </header>

      {error && (
        <p role="alert" className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
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
        <div className="space-y-4 rounded-xl border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
          <div className="inline-flex flex-wrap items-center gap-2 rounded-lg border border-slate-300 bg-slate-100 p-1">
            {sortedStatuses.map((status) => {
              const active = selectedMachineId === status.machine_id;
              return (
                <button
                  key={status.machine_id}
                  type="button"
                  onClick={() => {
                    setSelectedMachineId(status.machine_id);
                    setPushConfirmMachineId(null);
                    setPushConfirmValue("");
                  }}
                  className={`inline-flex h-9 min-w-28 items-center justify-center rounded-md border px-3 text-xs font-semibold ${
                    active
                      ? "border-blue-300 bg-white text-blue-700"
                      : "border-transparent bg-transparent text-slate-600 hover:text-slate-900"
                  }`}
                >
                  {status.machine_name}
                </button>
              );
            })}
          </div>

          {selectedStatus && (
            <>
              <div className="grid gap-3 text-sm md:grid-cols-4">
                <div className="rounded-md bg-slate-100/80 px-3 py-2">
                  <p className="text-xs text-slate-500">Current Period</p>
                  <p className="font-semibold text-slate-900">
                    {selectedSnapshot?.kes_current_period ?? selectedStatus.kes_period_current ?? "--"}
                  </p>
                </div>
                <div className="rounded-md bg-slate-100/80 px-3 py-2">
                  <p className="text-xs text-slate-500">Max Period</p>
                  <p className="font-semibold text-slate-900">
                    {selectedSnapshot?.kes_expiry_period ?? selectedStatus.kes_period_max ?? "--"}
                  </p>
                </div>
                <div className="rounded-md bg-slate-100/80 px-3 py-2">
                  <p className="text-xs text-slate-500">Remaining Days</p>
                  <p className="font-semibold text-slate-900">
                    {selectedSnapshot?.kes_remaining_periods != null
                      ? `约 ${Number((selectedSnapshot.kes_remaining_periods * 1.5).toFixed(1))}`
                      : selectedStatus.remaining_days != null
                        ? String(selectedStatus.remaining_days)
                        : "--"}
                  </p>
                </div>
                <div className="rounded-md bg-slate-100/80 px-3 py-2">
                  <p className="text-xs text-slate-500">Severity</p>
                  <p className={`font-semibold uppercase ${severityTone(displaySeverity)}`}>
                    {displaySeverity}
                  </p>
                </div>
              </div>

              {wizardStep === 1 && (
                <section className="space-y-3 rounded-lg border border-slate-200 bg-white p-3">
                  <h2 className="text-sm font-semibold">Step 1 · 生成 KES Keypairs</h2>
                  <p className="text-xs text-slate-500">
                    远程连接 BP 节点执行 KES keygen，`kes.skey` 留在 BP，`kes.vkey` 拉回本地。
                  </p>
                  <button
                    type="button"
                    onClick={() => void handleGenerate(selectedStatus.machine_id)}
                    disabled={selectedBusy}
                    className="rounded-md border border-blue-300 bg-blue-50 px-3 py-2 text-sm font-semibold text-blue-700 hover:bg-blue-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1 disabled:opacity-60"
                  >
                    {selectedBusy ? "Connecting to BP..." : "Generate KES"}
                  </button>
                </section>
              )}

              {wizardStep === 2 && (
                <section className="space-y-3 rounded-lg border border-slate-200 bg-white p-3">
                  <h2 className="text-sm font-semibold">Step 2 · 冷环境生成 node.cert</h2>
                  {selectedRequest ? (
                    <div className="space-y-3">
                      <div className="grid gap-3 text-xs md:grid-cols-3">
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <span className="block text-slate-500">KES Period</span>
                          <strong className="text-slate-900">{selectedRequest.kes_period ?? "--"}</strong>
                        </div>
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <span className="block text-slate-500">Counter</span>
                          <strong className="text-slate-900">{selectedRequest.counter_value}</strong>
                        </div>
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <span className="block text-slate-500">Node Version</span>
                          <strong className="text-slate-900">{selectedRequest.cardano_node_version ?? "--"}</strong>
                        </div>
                      </div>

                      <div className="rounded-md border border-slate-200 bg-slate-50 p-3 text-xs">
                        <p className="font-medium text-slate-900">KES verification key</p>
                        <p className="mt-1 break-all text-slate-700">{selectedRequest.kes_vkey_path}</p>
                      </div>

                      <div className="space-y-2 rounded-md border border-slate-200 bg-slate-50 p-3">
                        <p className="text-xs font-medium text-slate-900">Bundle 工具包</p>
                        <p className="text-xs text-slate-500">
                          打包签发脚本 + kes.vkey，拷贝到冷环境执行即可生成 node.cert。
                        </p>
                        <label className="flex items-center gap-2 text-xs text-slate-700">
                          <input
                            type="checkbox"
                            checked={bundleIncludeCli}
                            onChange={(e) => setBundleIncludeCli(e.target.checked)}
                            className="rounded border-slate-300"
                          />
                          冷环境无 cardano-cli，需要附带
                        </label>
                        {bundleIncludeCli && (
                          <div className="flex items-center gap-2">
                            <label htmlFor="bundle-platform" className="text-xs text-slate-600">
                              目标平台
                            </label>
                            <select
                              id="bundle-platform"
                              value={bundlePlatform}
                              onChange={(e) => setBundlePlatform(e.target.value)}
                              className="rounded-md border border-slate-300 bg-white px-2 py-1 text-xs text-slate-900 outline-none focus:border-blue-300 focus:ring-2 focus:ring-blue-100"
                            >
                              <option value="linux-x86_64">Linux x86_64</option>
                              <option value="macos-aarch64">macOS Apple Silicon</option>
                            </select>
                          </div>
                        )}
                        <button
                          type="button"
                          onClick={() => void handlePrepareBundle(selectedStatus.machine_id)}
                          disabled={bundleBusy}
                          className="rounded-md border border-blue-300 bg-blue-50 px-3 py-2 text-sm font-semibold text-blue-700 hover:bg-blue-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1 disabled:opacity-60"
                        >
                          {bundleBusy
                            ? bundleIncludeCli
                              ? "Downloading cardano-cli..."
                              : "Preparing..."
                            : "Prepare Bundle"}
                        </button>
                        {bundleResult && (
                          <div className="rounded-md border border-emerald-200 bg-emerald-50 p-2 text-xs text-emerald-800">
                            <p className="font-medium">Bundle ready</p>
                            <p className="mt-1 break-all text-emerald-700">{bundleResult.bundle_dir}</p>
                            {bundleResult.includes_cli && (
                              <p className="mt-1 text-emerald-600">
                                Includes cardano-cli for {bundleResult.target_platform}
                              </p>
                            )}
                          </div>
                        )}
                      </div>

                      <button
                        type="button"
                        onClick={() => setWizardStep(3)}
                        className="rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
                      >
                        下一步：上传 node.cert
                      </button>
                    </div>
                  ) : (
                    <p className="text-sm text-slate-600">先完成 Step 1，生成 KES keypairs。</p>
                  )}
                </section>
              )}

              {wizardStep === 3 && (
                <section className="space-y-3 rounded-lg border border-slate-200 bg-white p-3">
                  <h2 className="text-sm font-semibold">Step 3 · 上传证书并执行 Rotate</h2>
                  <div className="space-y-2">
                    <p className="text-xs text-slate-500">
                      上传离线签发后的 `node.cert`，通过预检后执行 BP Rotate。
                    </p>
                    <div className="flex flex-col gap-2 md:flex-row">
                      <label htmlFor="kes-cert-path" className="sr-only">
                        证书文件路径
                      </label>
                      <input
                        id="kes-cert-path"
                        value={certPaths[selectedStatus.machine_id] ?? ""}
                        onChange={(event) =>
                          setCertPaths((prev) => ({
                            ...prev,
                            [selectedStatus.machine_id]: event.target.value,
                          }))
                        }
                        placeholder="/absolute/path/to/node.cert"
                        autoCapitalize="none"
                        autoCorrect="off"
                        className="flex-1 min-w-0 rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none transition focus:border-blue-300 focus:ring-2 focus:ring-blue-100"
                      />
                      <button
                        type="button"
                        onClick={() => void handleImport(selectedStatus.machine_id)}
                        disabled={selectedBusy}
                        className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm font-semibold text-amber-700 hover:bg-amber-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-300 focus-visible:ring-offset-1 disabled:opacity-60"
                      >
                        {selectedBusy ? "Importing..." : "Import Cert"}
                      </button>
                    </div>
                  </div>

                  <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <p className="text-sm font-medium text-slate-900">Risk Gate</p>
                        <p className="text-xs text-slate-500">输入 ticker 解锁高风险 Rotate 操作。</p>
                      </div>
                      <button
                        type="button"
                        onClick={() => {
                          setPushConfirmMachineId(selectedStatus.machine_id);
                          setPushConfirmValue("");
                        }}
                        disabled={selectedBusy || !canPush}
                        className="rounded-md border border-emerald-300 bg-emerald-50 px-3 py-1.5 text-xs font-semibold text-emerald-700 hover:bg-emerald-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300 focus-visible:ring-offset-1 disabled:opacity-60"
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
                            className="flex-1 min-w-0 rounded-md border border-red-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none transition focus:border-red-400 focus:ring-2 focus:ring-red-100"
                          />
                          <div className="flex gap-2">
                            <button
                              type="button"
                              onClick={() => void handlePush(selectedStatus.machine_id)}
                              disabled={selectedBusy || !pushUnlocked}
                              className="rounded-md bg-red-500 px-3 py-2 text-sm font-semibold text-white hover:bg-red-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                              Confirm KES Push
                            </button>
                            <button
                              type="button"
                              onClick={() => {
                                setPushConfirmMachineId(null);
                                setPushConfirmValue("");
                              }}
                              className="rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
                            >
                              Cancel
                            </button>
                          </div>
                        </div>
                      </div>
                    )}
                    {selectedTask && (
                      <div className="mt-3 rounded-md border border-slate-200 bg-white p-3 text-xs text-slate-700">
                        <p className={`font-medium uppercase ${taskTone(selectedTask.status)}`}>
                          Task {formatTaskLabel(selectedTask.status)}
                        </p>
                        <p className="mt-1 break-all text-slate-500">Task ID: {selectedTask.task_id}</p>
                        {selectedTaskError && (
                          <p className="mt-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-red-700">
                            {selectedTaskError}
                          </p>
                        )}
                      </div>
                    )}
                  </div>
                </section>
              )}

              {wizardStep === 4 && (
                <section className="space-y-3 rounded-lg border border-slate-200 bg-white p-3">
                  <h2 className="text-sm font-semibold">Step 4 · 校验完成</h2>
                  <div className="grid gap-2 text-xs text-slate-600 md:grid-cols-3">
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                      <span className="block text-slate-500">Operation</span>
                      <strong className="text-slate-900">
                        {selectedTask?.status ? formatTaskLabel(selectedTask.status) : "pending"}
                      </strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                      <span className="block text-slate-500">KES remain</span>
                      <strong className="text-slate-900">
                        {selectedSnapshot?.kes_remaining_periods != null
                          ? `约 ${Number((selectedSnapshot.kes_remaining_periods * 1.5).toFixed(1))}d`
                          : selectedStatus.remaining_days != null
                            ? `${selectedStatus.remaining_days}d`
                            : "--"}
                      </strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                      <span className="block text-slate-500">BP health</span>
                      <strong className="text-slate-900">{displaySeverity === "critical" ? "risk" : "online"}</strong>
                    </div>
                  </div>
                  {selectedTask && <TaskLogStream taskId={selectedTask.task_id} />}
                </section>
              )}
            </>
          )}
        </div>
      )}
    </section>
  );
}
