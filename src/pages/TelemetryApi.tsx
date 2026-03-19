import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import TaskLogStream from "../components/TaskLogStream";
import { formatTaskError } from "../lib/errors";
import {
  observabilityBootstrapStart,
  observabilityBootstrapStatus,
  observabilityRollbackStart,
  observabilityRollbackStatus,
} from "../lib/ipc";
import { useGatewayStatusQuery } from "../lib/queries";
import type { DeployTaskStatus } from "../lib/types";

function isTaskTerminal(status: string): boolean {
  return status === "success" || status === "failed" || status === "cancelled";
}

function relayProbeToneClass(configured: boolean, nginxRunning: boolean): string {
  if (configured && nginxRunning) {
    return "border-emerald-300 bg-emerald-50 text-emerald-700";
  }
  return "border-amber-300 bg-amber-50 text-amber-700";
}

function taskChipClass(status: string): string {
  const base = "inline-flex min-h-6 items-center rounded-full border px-2 text-[11px] font-semibold leading-none";
  if (status === "success") {
    return `${base} border-emerald-300 bg-emerald-50 text-emerald-700`;
  }
  if (status === "running" || status === "pending") {
    return `${base} border-sky-300 bg-sky-50 text-sky-700`;
  }
  if (status === "failed" || status === "cancelled") {
    return `${base} border-rose-300 bg-rose-50 text-rose-700`;
  }
  return `${base} border-slate-300 bg-slate-100 text-slate-700`;
}

export default function TelemetryApi() {
  const queryClient = useQueryClient();
  const { data: gatewayStatus = null } = useGatewayStatusQuery();
  const [gatewayActionError, setGatewayActionError] = useState<string | null>(null);
  const [gatewayActionMessage, setGatewayActionMessage] = useState<string | null>(null);
  const [gatewayTask, setGatewayTask] = useState<{ taskId: string; kind: "bootstrap" | "rollback" } | null>(null);
  const [gatewaySubmittingKind, setGatewaySubmittingKind] = useState<"bootstrap" | "rollback" | null>(null);
  const [gatewayLogTaskId, setGatewayLogTaskId] = useState<string | null>(null);

  const refreshGatewayStatus = () => {
    void queryClient.invalidateQueries({ queryKey: ["telemetry", "gateway"] });
  };

  useEffect(() => {
    if (!gatewayTask) {
      return;
    }
    const timer = window.setInterval(() => {
      void (async () => {
        try {
          const task: DeployTaskStatus =
            gatewayTask.kind === "bootstrap"
              ? await observabilityBootstrapStatus(gatewayTask.taskId)
              : await observabilityRollbackStatus(gatewayTask.taskId);

          if (!isTaskTerminal(task.status)) {
            setGatewayActionMessage(
              `${gatewayTask.kind === "bootstrap" ? "Enable API" : "Rollback"} running · task ${gatewayTask.taskId} · ${task.status}`,
            );
            return;
          }

          window.clearInterval(timer);
          setGatewayTask(null);
          if (task.status === "success") {
            setGatewayActionError(null);
            setGatewayActionMessage(
              gatewayTask.kind === "bootstrap"
                ? "Telemetry API bootstrap completed."
                : "Telemetry API rollback completed.",
            );
          } else {
            setGatewayActionError(formatTaskError(task.error_msg) || `${gatewayTask.kind} task failed`);
          }
          await refreshGatewayStatus();
        } catch (error) {
          window.clearInterval(timer);
          setGatewayTask(null);
          setGatewayActionError(String(error));
        }
      })();
    }, 3_000);

    return () => {
      window.clearInterval(timer);
    };
  }, [gatewayTask]);

  const gatewayTaskRunning = gatewayTask != null;
  const gatewayActionBusy = gatewayTaskRunning || gatewaySubmittingKind != null;

  const configuredSummary = gatewayStatus
    ? `${gatewayStatus.configured_relays}/${gatewayStatus.relay_total}`
    : "--";

  const latestTask = useMemo(() => {
    if (!gatewayStatus) {
      return null;
    }
    if (gatewayStatus.last_bootstrap && gatewayStatus.last_rollback) {
      if (gatewayStatus.last_bootstrap.finished_at && gatewayStatus.last_rollback.finished_at) {
        return gatewayStatus.last_bootstrap.finished_at >= gatewayStatus.last_rollback.finished_at
          ? gatewayStatus.last_bootstrap
          : gatewayStatus.last_rollback;
      }
      return gatewayStatus.last_bootstrap;
    }
    return gatewayStatus.last_bootstrap ?? gatewayStatus.last_rollback;
  }, [gatewayStatus]);

  const handleGatewayBootstrap = async () => {
    if (gatewayActionBusy) {
      return;
    }
    setGatewaySubmittingKind("bootstrap");
    try {
      setGatewayActionError(null);
      setGatewayActionMessage("提交中…");
      const taskId = await observabilityBootstrapStart();
      setGatewayLogTaskId(taskId);
      setGatewayTask({ taskId, kind: "bootstrap" });
      setGatewayActionMessage(`Enable API started · task ${taskId}`);
      await refreshGatewayStatus();
    } catch (error) {
      setGatewayActionError(String(error));
    } finally {
      setGatewaySubmittingKind((current) => (current === "bootstrap" ? null : current));
    }
  };

  const handleGatewayRollback = async () => {
    if (gatewayActionBusy) {
      return;
    }
    setGatewaySubmittingKind("rollback");
    try {
      setGatewayActionError(null);
      setGatewayActionMessage("提交中…");
      const taskId = await observabilityRollbackStart();
      setGatewayLogTaskId(taskId);
      setGatewayTask({ taskId, kind: "rollback" });
      setGatewayActionMessage(`Rollback started · task ${taskId}`);
      await refreshGatewayStatus();
    } catch (error) {
      setGatewayActionError(String(error));
    } finally {
      setGatewaySubmittingKind((current) => (current === "rollback" ? null : current));
    }
  };

  return (
    <section className="space-y-5">
      <section className="rounded-xl border border-slate-200 bg-slate-50 text-slate-900 shadow-sm">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 px-4 py-3">
          <div>
            <h2 className="text-sm font-semibold">Telemetry API Gateway</h2>
            <p className="text-xs text-slate-600">管理 relay 观测网关：Enable API、Rollback、执行日志与 relay 健康探测。</p>
          </div>
          <div className="inline-flex items-center gap-2 rounded-full border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-700">
            <span className="font-semibold text-slate-900">GW {configuredSummary}</span>
            {latestTask && (
              <span className={taskChipClass(latestTask.status)}>
                latest · {latestTask.status}
              </span>
            )}
          </div>
        </header>

        <div className="flex flex-wrap items-center gap-2 px-4 pb-3 pt-3">
          <button
            type="button"
            onClick={() => {
              void handleGatewayBootstrap();
            }}
            disabled={gatewayActionBusy}
            className="inline-flex h-9 items-center rounded border border-slate-300 bg-white px-3 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-50"
            title="执行 observability bootstrap"
          >
            {gatewaySubmittingKind === "bootstrap"
              ? "提交中…"
              : gatewayTaskRunning && gatewayTask?.kind === "bootstrap"
                ? "Enabling..."
                : "Enable API"}
          </button>
          <button
            type="button"
            onClick={() => {
              void handleGatewayRollback();
            }}
            disabled={gatewayActionBusy}
            className="inline-flex h-9 items-center rounded border border-rose-300 bg-rose-50 px-3 text-xs font-semibold text-rose-700 transition hover:bg-rose-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-300 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-50"
            title="执行 observability rollback"
          >
            {gatewaySubmittingKind === "rollback"
              ? "提交中…"
              : gatewayTaskRunning && gatewayTask?.kind === "rollback"
                ? "Rolling..."
                : "Rollback"}
          </button>
        </div>

        {(gatewayActionMessage || gatewayActionError) && (
          <div className="px-4 pb-3">
            <p className={`text-xs font-medium ${gatewayActionError ? "text-rose-700" : "text-slate-700"}`}>
              {gatewayActionError ?? gatewayActionMessage}
            </p>
          </div>
        )}

        {gatewayLogTaskId && (
          <div className="px-4 pb-3">
            <TaskLogStream taskId={gatewayLogTaskId} />
          </div>
        )}
      </section>

      <section className="rounded-xl border border-slate-200 bg-slate-50 p-4 text-slate-900 shadow-sm">
        <header className="flex items-center justify-between gap-3">
          <h2 className="text-sm font-semibold">Relay Probe</h2>
          <span className="text-xs text-slate-500">
            {gatewayStatus ? `${gatewayStatus.relays.length} relay` : "--"}
          </span>
        </header>

        <div className="mt-3 overflow-x-auto rounded-lg border border-slate-200 bg-white">
          <p className="px-3 py-1.5 text-xs text-slate-500" role="note">
            表格较宽，小屏可左右滑动查看。
          </p>
          <table className="w-full min-w-[760px] text-left text-xs">
            <thead className="bg-slate-100 text-slate-600">
              <tr>
                <th className="px-3 py-2">Relay</th>
                <th className="px-3 py-2">IP</th>
                <th className="px-3 py-2">Gateway</th>
                <th className="px-3 py-2">htpasswd</th>
                <th className="px-3 py-2">nginx</th>
                <th className="px-3 py-2">状态</th>
                <th className="px-3 py-2">备注</th>
              </tr>
            </thead>
            <tbody>
              {!gatewayStatus || gatewayStatus.relays.length === 0 ? (
                <tr>
                  <td colSpan={7} className="px-3 py-4 text-center text-slate-500">
                    No relay probe data.
                  </td>
                </tr>
              ) : (
                gatewayStatus.relays.map((relay) => (
                  <tr key={relay.machine_id} className="border-t border-slate-200">
                    <td className="px-3 py-2 font-medium text-slate-900">{relay.machine_name}</td>
                    <td className="px-3 py-2 text-slate-600">{relay.ip}</td>
                    <td className="px-3 py-2 text-slate-600">{relay.gateway_conf_present ? "yes" : "no"}</td>
                    <td className="px-3 py-2 text-slate-600">{relay.htpasswd_present ? "yes" : "no"}</td>
                    <td className="px-3 py-2 text-slate-600">{relay.nginx_running ? "running" : "down"}</td>
                    <td className="px-3 py-2">
                      <span className={`inline-flex min-h-6 items-center rounded-full border px-2 text-[11px] font-semibold ${relayProbeToneClass(relay.configured, relay.nginx_running)}`}>
                        {relay.configured ? "configured" : "partial"}
                      </span>
                    </td>
                    <td className="px-3 py-2 text-slate-600">{relay.note ?? "--"}</td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </section>
    </section>
  );
}
