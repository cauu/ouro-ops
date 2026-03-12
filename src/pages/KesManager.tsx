import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import TaskLogStream from "../components/TaskLogStream";
import { formatTaskError, toUserError } from "../lib/errors";
import {
  kesGenerate,
  kesImportCert,
  kesPushStart,
  kesRotationStatus,
  kesStatusAll,
} from "../lib/ipc";
import type { DeployTaskStatus, KesSignRequest, KesStatus } from "../lib/types";

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

async function copyPlainText(value: string): Promise<boolean> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return true;
    }
  } catch {
    // fallback below
  }

  try {
    if (typeof document === "undefined") {
      return false;
    }
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.setAttribute("readonly", "true");
    textarea.style.position = "absolute";
    textarea.style.left = "-9999px";
    document.body.appendChild(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    document.body.removeChild(textarea);
    return copied;
  } catch {
    return false;
  }
}

function outputDirFromPath(path: string | null | undefined): string {
  if (!path) {
    return "/opt/cardano/keys";
  }
  const idx = path.lastIndexOf("/");
  if (idx <= 0) {
    return "/opt/cardano/keys";
  }
  return path.slice(0, idx);
}

const STEP_LABELS = [
  "1 生成 KES keypairs",
  "2 离线生成 node.cert",
  "3 上传证书并执行 Rotate",
  "4 校验完成",
] as const;

interface KesManagerProps {
  poolTicker: string;
}

export default function KesManager({ poolTicker }: KesManagerProps) {
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
  const [searchValue, setSearchValue] = useState("");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

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

  const handleGenerate = async (machineId: number) => {
    setBusyMachineId(machineId);
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

  const copyAndFlash = async (key: string, value: string) => {
    const copied = await copyPlainText(value);
    if (!copied) {
      return;
    }
    setCopiedKey(key);
    window.setTimeout(() => {
      setCopiedKey((current) => (current === key ? null : current));
    }, 1200);
  };

  const handleStep3Next = async () => {
    if (selectedMachineId == null) {
      return;
    }
    if (!selectedTask) {
      setError("请先导入 node.cert 并完成预检。");
      return;
    }
    if (selectedTask.status === "pending") {
      if (!pushConfirmArmed) {
        setPushConfirmMachineId(selectedMachineId);
        setPushConfirmValue("");
        setError("请先输入 Pool Ticker 解锁 Rotate 执行。");
        return;
      }
      if (!pushUnlocked) {
        setError(`请输入正确的 Pool Ticker：${normalizedTicker}`);
        return;
      }
      await handlePush(selectedMachineId);
      return;
    }
    if (selectedTask.status === "running") {
      setError("Rotate 正在执行，请等待任务完成后进入 Step 4。");
      return;
    }
    if (isTerminal(selectedTask.status)) {
      setWizardStep(4);
    }
  };

  const handlePrimaryAction = async () => {
    if (selectedMachineId == null) {
      return;
    }
    if (wizardStep === 1) {
      if (!selectedRequest) {
        await handleGenerate(selectedMachineId);
      } else {
        setWizardStep(2);
      }
      return;
    }
    if (wizardStep === 2) {
      setWizardStep(3);
      return;
    }
    if (wizardStep === 3) {
      await handleStep3Next();
      return;
    }
    setWizardStep(4);
  };

  const normalizedTicker = poolTicker.trim();
  const selectedBusy = selectedMachineId != null && busyMachineId === selectedMachineId;
  const canPush = selectedTask?.status === "pending";
  const pushConfirmArmed = selectedMachineId != null && pushConfirmMachineId === selectedMachineId;
  const pushUnlocked = pushConfirmValue.trim() === normalizedTicker;
  const selectedTaskError = formatTaskError(selectedTask?.error_msg);

  const outputDir = outputDirFromPath(selectedRequest?.kes_vkey_path);
  const step1Command = `cardano-cli node key-gen-KES \\
  --verification-key-file ${outputDir}/kes.vkey \\
  --signing-key-file ${outputDir}/kes.skey`;
  const step1CommandWithNotes = `${step1Command}

# Run in bp hot environment
# Output files: kes.vkey / kes.skey
# Next input: node.cert signing`;
  const step2Command =
    selectedRequest?.instructions?.trim() ||
    `cardano-cli node issue-op-cert \\
  --kes-verification-key-file kes.vkey \\
  --cold-signing-key-file cold.skey \\
  --operational-certificate-issue-counter-file opcert.counter \\
  --kes-period ${selectedStatus?.kes_period_current ?? "--"} \\
  --out-file node.cert`;

  const filteredSteps = STEP_LABELS.filter((label) => {
    if (!searchValue.trim()) {
      return true;
    }
    return label.toLowerCase().includes(searchValue.trim().toLowerCase());
  });

  return (
    <section className="space-y-4">
      <header className="drag-region rounded-xl border border-slate-200 bg-white px-4 py-3 shadow-sm" data-tauri-drag-region>
        <div className="drag-region flex flex-wrap items-center justify-between gap-3" data-tauri-drag-region>
          <div className="drag-region flex min-w-0 items-center gap-3" data-tauri-drag-region>
            <div className="flex items-center gap-1.5" aria-hidden="true">
              <span className="h-2.5 w-2.5 rounded-full bg-red-400" />
              <span className="h-2.5 w-2.5 rounded-full bg-amber-400" />
              <span className="h-2.5 w-2.5 rounded-full bg-emerald-400" />
            </div>
            <button type="button" className="no-drag rounded-md border border-slate-300 bg-slate-50 px-2.5 py-1 text-xs text-slate-600">
              Sidebar
            </button>
            <h1 className="no-drag truncate text-sm font-semibold text-slate-900">Ouro Ops · KES Rotate</h1>
          </div>
          <div className="no-drag flex items-center gap-2">
            <label className="no-drag inline-flex min-h-8 items-center gap-2 rounded-md border border-slate-300 bg-slate-50 px-2.5 text-xs text-slate-600">
              <span aria-hidden="true">⌕</span>
              <input
                value={searchValue}
                onChange={(event) => setSearchValue(event.target.value)}
                type="search"
                placeholder="搜索 KES 操作"
                className="w-40 border-none bg-transparent text-xs text-slate-700 outline-none"
              />
            </label>
            <span className="no-drag inline-flex min-h-8 items-center rounded-full border border-amber-300 bg-amber-50 px-2.5 text-xs font-semibold text-amber-700">
              Step {wizardStep} / 4
            </span>
          </div>
        </div>
      </header>

      {error && (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
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
      ) : !selectedStatus ? (
        <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 text-sm text-slate-500">
          Resolving selected BP context...
        </div>
      ) : (
        <section className="rounded-xl border border-slate-200 bg-slate-50 p-4 shadow-sm">
          <section className="mx-auto grid min-h-[calc(100vh-240px)] w-full max-w-[980px] grid-rows-[auto_1fr_auto] gap-3">
            <header className="space-y-2">
              <h2 className="text-base font-semibold text-slate-900">
                {wizardStep === 1
                  ? "Step 1 · 生成 KES Keypairs"
                  : wizardStep === 2
                    ? "Step 2 · 冷环境生成 node.cert"
                    : wizardStep === 3
                      ? "Step 3 · 上传 node.cert 并执行 Rotate"
                      : "Step 4 · 校验完成"}
              </h2>
              <p className="text-sm text-slate-600">
                {wizardStep === 1
                  ? "先在热环境生成新的 `kes.skey` 与 `kes.vkey`，作为后续离线签发 `node.cert` 的输入。"
                  : wizardStep === 2
                    ? "带上 Step 1 生成的 `kes.vkey`，在 cold environment 生成新的运营证书。"
                    : wizardStep === 3
                      ? "上传离线生成的证书，完成预检后触发 Rotate。"
                      : "展示 Rotate 执行日志与最终验收结果。"}
              </p>
              <div className="inline-flex flex-wrap items-center gap-2" aria-label="wizard progress">
                {STEP_LABELS.map((label, index) => {
                  const step = index + 1;
                  const state = wizardStep === step ? "current" : wizardStep > step ? "done" : "todo";
                  return (
                    <div key={label} className="inline-flex items-center gap-2">
                      <span
                        className={`inline-flex min-h-6 items-center rounded-full border px-2.5 text-xs ${
                          state === "current"
                            ? "border-blue-300 bg-blue-50 font-semibold text-blue-700"
                            : state === "done"
                              ? "border-emerald-300 bg-emerald-50 text-emerald-700"
                              : "border-slate-300 bg-slate-100 text-slate-600"
                        }`}
                      >
                        {label}
                      </span>
                      {index < STEP_LABELS.length - 1 && <span className="h-1.5 w-1.5 rounded-full bg-slate-300" aria-hidden="true" />}
                    </div>
                  );
                })}
              </div>
            </header>

            <div className="min-h-0 overflow-y-auto pr-1">
              <article className="rounded-lg border border-slate-200 bg-white shadow-sm">
                <header className="border-b border-slate-200 px-4 py-3">
                  <h3 className="text-sm font-semibold text-slate-900">执行上下文</h3>
                  <p className="mt-1 text-xs text-slate-500">确认 BP 环境和即将执行的命令上下文。</p>
                </header>
                <div className="p-4">
                  <div className="grid gap-3 text-sm md:grid-cols-2">
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                      <span className="block text-xs text-slate-500">Target Node</span>
                      <strong className="text-slate-900">{selectedStatus?.machine_name ?? "--"}</strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                      <span className="block text-xs text-slate-500">KES Period (current)</span>
                      <strong className="text-slate-900">{selectedStatus?.kes_period_current ?? "--"}</strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                      <span className="block text-xs text-slate-500">Output Key Dir</span>
                      <strong className="break-all text-slate-900">{outputDir}</strong>
                    </div>
                    <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                      <span className="block text-xs text-slate-500">Window Remain</span>
                      <strong className="text-slate-900">
                        {selectedStatus?.remaining_days == null ? "--" : `${selectedStatus.remaining_days}d`}
                      </strong>
                    </div>
                  </div>
                </div>
              </article>

              {wizardStep === 1 && (
                <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                  <header className="border-b border-slate-200 px-4 py-3">
                    <h3 className="text-sm font-semibold text-slate-900">Step 1 命令</h3>
                    <p className="mt-1 text-xs text-slate-500">在热环境执行，生成新的 KES 密钥对。</p>
                  </header>
                  <div className="space-y-3 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="text-xs text-slate-500">该命令执行后会写入 `kes.skey` / `kes.vkey`。</span>
                      <div className="inline-flex items-center gap-2">
                        <button
                          type="button"
                          onClick={() => void copyAndFlash("s1-cmd", step1Command)}
                          className="rounded-md border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-700 hover:bg-slate-100"
                        >
                          {copiedKey === "s1-cmd" ? "Copied" : "Copy Command"}
                        </button>
                        <button
                          type="button"
                          onClick={() => void copyAndFlash("s1-note", step1CommandWithNotes)}
                          className="rounded-md border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-700 hover:bg-slate-100"
                        >
                          {copiedKey === "s1-note" ? "Copied" : "Copy + 参数说明"}
                        </button>
                        <button
                          type="button"
                          onClick={() => selectedMachineId != null && void handleGenerate(selectedMachineId)}
                          disabled={selectedBusy}
                          className="rounded-md border border-blue-300 bg-blue-50 px-2.5 py-1 text-xs font-semibold text-blue-700 hover:bg-blue-100 disabled:opacity-60"
                        >
                          {selectedBusy ? "Generating..." : "Generate KES"}
                        </button>
                      </div>
                    </div>

                    <div className="overflow-hidden rounded-md border border-slate-200 bg-slate-950">
                      <div className="flex items-center gap-1.5 border-b border-slate-800 px-3 py-2">
                        <span className="h-2.5 w-2.5 rounded-full bg-red-400" />
                        <span className="h-2.5 w-2.5 rounded-full bg-amber-400" />
                        <span className="h-2.5 w-2.5 rounded-full bg-emerald-400" />
                      </div>
                      <pre className="overflow-x-auto p-3 text-xs text-slate-100">{step1Command}</pre>
                    </div>

                    <div className="flex flex-wrap gap-2" aria-label="command context">
                      <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Run In: bp hot environment</span>
                      <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Output: kes.vkey</span>
                      <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Output: kes.skey</span>
                      <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Next Input: node.cert signing</span>
                    </div>
                  </div>
                </article>
              )}

              {wizardStep === 2 && (
                <>
                  <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                    <header className="border-b border-slate-200 px-4 py-3">
                      <h3 className="text-sm font-semibold text-slate-900">离线签发命令</h3>
                      <p className="mt-1 text-xs text-slate-500">在 cold environment 执行，不在热机保存 cold.skey。</p>
                    </header>
                    <div className="space-y-3 p-4">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <span className="text-xs text-slate-500">执行后生成 `node.cert`，供下一步上传。</span>
                        <button
                          type="button"
                          onClick={() => void copyAndFlash("s2-cmd", step2Command)}
                          className="rounded-md border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-700 hover:bg-slate-100"
                        >
                          {copiedKey === "s2-cmd" ? "Copied" : "Copy Command"}
                        </button>
                      </div>
                      <div className="overflow-hidden rounded-md border border-slate-200 bg-slate-950">
                        <div className="flex items-center gap-1.5 border-b border-slate-800 px-3 py-2">
                          <span className="h-2.5 w-2.5 rounded-full bg-red-400" />
                          <span className="h-2.5 w-2.5 rounded-full bg-amber-400" />
                          <span className="h-2.5 w-2.5 rounded-full bg-emerald-400" />
                        </div>
                        <pre className="overflow-x-auto whitespace-pre-wrap p-3 text-xs text-slate-100">{step2Command}</pre>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Input: kes.vkey</span>
                        <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Input: cold.skey</span>
                        <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Input: opcert.counter</span>
                        <span className="rounded-full border border-slate-300 bg-slate-100 px-2.5 py-1 text-xs text-slate-600">Output: node.cert</span>
                      </div>
                    </div>
                  </article>

                  <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                    <header className="border-b border-slate-200 px-4 py-3">
                      <h3 className="text-sm font-semibold text-slate-900">产物检查</h3>
                      <p className="mt-1 text-xs text-slate-500">完成离线执行后，确认文件完整再回到热环境。</p>
                    </header>
                    <div className="space-y-2 p-4 text-sm">
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">node.cert exists</strong>
                        <span className="text-slate-500">required</span>
                      </div>
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">opcert.counter updated</strong>
                        <span className="text-slate-500">recommended</span>
                      </div>
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">cold keys kept offline</strong>
                        <span className="text-slate-500">mandatory</span>
                      </div>
                    </div>
                  </article>
                </>
              )}

              {wizardStep === 3 && (
                <>
                  <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                    <header className="border-b border-slate-200 px-4 py-3">
                      <h3 className="text-sm font-semibold text-slate-900">上传 node.cert</h3>
                      <p className="mt-1 text-xs text-slate-500">上传后执行格式、网络与 KES 预检。</p>
                    </header>
                    <div className="space-y-3 p-4">
                      <div className="rounded-md border border-dashed border-blue-300 bg-blue-50/60 p-3 text-sm">
                        <strong className="text-slate-900">将 `node.cert` 拖拽到这里</strong>
                        <p className="mt-1 text-slate-600">也可直接粘贴文件路径，校验通过后可执行 Rotate。</p>
                        <div className="mt-3 flex flex-col gap-2 md:flex-row">
                          <input
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
                            className="flex-1 rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900"
                          />
                          <button
                            type="button"
                            onClick={() => void handleImport(selectedStatus.machine_id)}
                            disabled={selectedBusy}
                            className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm font-semibold text-amber-700 hover:bg-amber-100 disabled:opacity-60"
                          >
                            {selectedBusy ? "Importing..." : "Import Cert"}
                          </button>
                        </div>
                      </div>

                      <div className="space-y-2 text-sm">
                        <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <strong className="text-slate-900">1. Certificate parsed</strong>
                          <span className="rounded-full border border-emerald-300 bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700">
                            {selectedTask ? "pass" : "pending"}
                          </span>
                        </div>
                        <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <strong className="text-slate-900">2. KES period matches</strong>
                          <span className="rounded-full border border-slate-300 bg-white px-2 py-0.5 text-xs text-slate-600">
                            {selectedTask ? "pass" : "pending"}
                          </span>
                        </div>
                        <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <strong className="text-slate-900">3. Network checks</strong>
                          <span className="rounded-full border border-slate-300 bg-white px-2 py-0.5 text-xs text-slate-600">
                            {selectedTask ? "pass" : "pending"}
                          </span>
                        </div>
                      </div>
                    </div>
                  </article>

                  <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                    <header className="border-b border-slate-200 px-4 py-3">
                      <h3 className="text-sm font-semibold text-slate-900">执行闸门（高风险）</h3>
                      <p className="mt-1 text-xs text-slate-500">执行 Rotate 前必须完成人工确认，失败时中止并保留回滚入口。</p>
                    </header>
                    <div className="space-y-3 p-4">
                      <div className="flex items-center justify-between gap-2 rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-600">
                        <span>通过人工确认后才能执行 BP Rotate。</span>
                        <button
                          type="button"
                          onClick={() => {
                            if (selectedMachineId == null) {
                              return;
                            }
                            setPushConfirmMachineId(selectedMachineId);
                            setPushConfirmValue("");
                          }}
                          className="rounded-md border border-emerald-300 bg-emerald-50 px-2.5 py-1 text-xs font-semibold text-emerald-700 hover:bg-emerald-100"
                        >
                          Push to BP
                        </button>
                      </div>

                      <div className="rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-700">
                        <p className="font-medium">Type pool ticker {normalizedTicker} to unlock KES push.</p>
                        <div className="mt-2 flex flex-col gap-2 md:flex-row">
                          <input
                            value={pushConfirmValue}
                            onChange={(event) => setPushConfirmValue(event.target.value)}
                            placeholder="输入 Pool Ticker"
                            autoCapitalize="none"
                            autoCorrect="off"
                            className="flex-1 rounded-md border border-red-300 bg-white px-3 py-2 text-sm text-slate-900"
                          />
                          <button
                            type="button"
                            onClick={() => selectedMachineId != null && void handlePush(selectedMachineId)}
                            disabled={selectedBusy || !canPush || !pushUnlocked}
                            className="rounded-md bg-red-500 px-3 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
                          >
                            {selectedBusy ? "Pushing..." : "Confirm KES Push"}
                          </button>
                        </div>
                      </div>

                      <div className="grid gap-3 text-sm md:grid-cols-2">
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <span className="block text-xs text-slate-500">Target Node</span>
                          <strong className="text-slate-900">{selectedStatus?.machine_name ?? "--"}</strong>
                        </div>
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                          <span className="block text-xs text-slate-500">Certificate</span>
                          <strong className="break-all text-slate-900">{certPaths[selectedStatus.machine_id] || "--"}</strong>
                        </div>
                      </div>

                      {selectedTask && (
                        <div className="rounded-md border border-slate-200 bg-slate-50 p-3 text-xs text-slate-700">
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
                  </article>
                </>
              )}

              {wizardStep === 4 && (
                <>
                  <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                    <header className="border-b border-slate-200 px-4 py-3">
                      <h3 className="text-sm font-semibold text-slate-900">Rotate 执行状态</h3>
                      <p className="mt-1 text-xs text-slate-500">展示当前执行阶段与最终校验结果。</p>
                    </header>
                    <div className="space-y-2 p-4 text-sm">
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">upload cert</strong>
                        <span className="rounded-full border border-emerald-300 bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700">done</span>
                      </div>
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">apply cert to bp</strong>
                        <span className="rounded-full border border-emerald-300 bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700">done</span>
                      </div>
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">bp restart</strong>
                        <span className="rounded-full border border-amber-300 bg-amber-50 px-2 py-0.5 text-xs text-amber-700">
                          {selectedTask?.status === "running" ? "running" : "done"}
                        </span>
                      </div>
                      <div className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 px-3 py-2">
                        <strong className="text-slate-900">kes window verify</strong>
                        <span className="rounded-full border border-slate-300 bg-white px-2 py-0.5 text-xs text-slate-600">
                          {selectedStatus?.remaining_days == null ? "pending" : "ok"}
                        </span>
                      </div>
                    </div>
                  </article>

                  <article className="mt-3 rounded-lg border border-slate-200 bg-white shadow-sm">
                    <header className="border-b border-slate-200 px-4 py-3">
                      <h3 className="text-sm font-semibold text-slate-900">执行日志</h3>
                      <p className="mt-1 text-xs text-slate-500">Rotate 过程与最终 KES 指标。</p>
                    </header>
                    <div className="space-y-3 p-4">
                      {selectedTask && <TaskLogStream taskId={selectedTask.task_id} />}
                      <div className="grid gap-2 text-xs md:grid-cols-4">
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                          <span className="block text-slate-500">KES Period</span>
                          <strong className="text-slate-900">{selectedStatus?.kes_period_current ?? "--"}</strong>
                        </div>
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                          <span className="block text-slate-500">KES Remain</span>
                          <strong className="text-slate-900">{selectedStatus?.remaining_days ?? "--"}</strong>
                        </div>
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                          <span className="block text-slate-500">BP Health</span>
                          <strong className={severityTone(selectedStatus?.severity ?? "warning")}>
                            {selectedStatus?.severity ?? "--"}
                          </strong>
                        </div>
                        <div className="rounded-md border border-slate-200 bg-slate-50 px-2.5 py-2">
                          <span className="block text-slate-500">Operation</span>
                          <strong className={taskTone(selectedTask?.status ?? "pending")}>
                            {selectedTask?.status ? formatTaskLabel(selectedTask.status) : "pending"}
                          </strong>
                        </div>
                      </div>
                    </div>
                  </article>
                </>
              )}

              {filteredSteps.length === 0 && (
                <div className="mt-3 rounded-md border border-slate-200 bg-white px-3 py-2 text-xs text-slate-500">
                  当前搜索无匹配步骤标签。
                </div>
              )}
            </div>

            <footer className="sticky bottom-0 z-10 flex min-h-14 items-center justify-between gap-2 rounded-lg border border-slate-200 bg-white/95 px-3 py-2 shadow-sm backdrop-blur-sm">
              <Link to="/" className="rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-100">
                取消
              </Link>
              <div className="inline-flex items-center gap-2">
                <button
                  type="button"
                  disabled={wizardStep <= 1}
                  onClick={() => setWizardStep((step) => Math.max(1, step - 1))}
                  className="rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 hover:bg-slate-100 disabled:opacity-60"
                >
                  上一步
                </button>
                {wizardStep < 4 ? (
                  <button
                    type="button"
                    onClick={() => {
                      void handlePrimaryAction();
                    }}
                    disabled={selectedBusy}
                    className="rounded-md border border-blue-600 bg-blue-600 px-3 py-2 text-sm font-semibold text-white hover:bg-blue-700 disabled:opacity-60"
                  >
                    {wizardStep === 1
                      ? selectedRequest ? "下一步" : "Generate KES"
                      : wizardStep === 3
                        ? "执行 Rotate"
                        : "下一步"}
                  </button>
                ) : (
                  <Link
                    to="/"
                    className="rounded-md border border-blue-600 bg-blue-600 px-3 py-2 text-sm font-semibold text-white hover:bg-blue-700"
                  >
                    返回 Dashboard
                  </Link>
                )}
              </div>
            </footer>
          </section>
        </section>
      )}
    </section>
  );
}
