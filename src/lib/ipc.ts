import { invoke } from "@tauri-apps/api/core";
import type {
  DbVersionResult,
  DeployPayload,
  DeployTaskStatus,
  UpgradePayload,
  Machine,
  MachineAddPayload,
  MachineFilter,
  MonitorSnapshot,
  ObservabilityGatewayStatus,
  ObservabilityTaskPayload,
  KesBundleResult,
  KesSignRequest,
  KesStatus,
  Pool,
  PoolBindOnchainPayload,
  PoolOnchainQueryPayload,
  PoolRegistrationPreparePayload,
  PoolRegistrationPrepareResult,
  PoolRegistrationSubmitPayload,
  PoolRegistrationSubmitResult,
  PoolOnchainStatus,
  PoolInitPayload,
  PoolUpdatePayload,
  PreflightReport,
  RecentTaskSummary,
  RuntimeProbe,
  SshKeyInfo,
  TaskLogPage,
  TaskLogQueryPayload,
} from "./types";

export async function ping(): Promise<void> {
  await invoke("ping");
}

export async function dbVersion(): Promise<DbVersionResult> {
  return invoke("db_version");
}

export async function runPlaybookTest(): Promise<string> {
  return invoke("run_playbook_test");
}

export async function poolGet(): Promise<Pool> {
  return invoke("pool_get");
}

export async function poolInit(payload: PoolInitPayload): Promise<Pool> {
  return invoke("pool_init", { payload });
}

export async function poolUpdate(payload: PoolUpdatePayload): Promise<Pool> {
  return invoke("pool_update", { payload });
}

export async function poolOnchainStatus(
  payload: PoolOnchainQueryPayload,
): Promise<PoolOnchainStatus> {
  return invoke("pool_onchain_status", { payload });
}

export async function poolBindOnchain(payload: PoolBindOnchainPayload): Promise<Pool> {
  return invoke("pool_bind_onchain", { payload });
}

export async function poolRegistrationPrepare(
  payload: PoolRegistrationPreparePayload,
): Promise<PoolRegistrationPrepareResult> {
  return invoke("pool_registration_prepare", { payload });
}

export async function poolRegistrationSubmit(
  payload: PoolRegistrationSubmitPayload,
): Promise<PoolRegistrationSubmitResult> {
  return invoke("pool_registration_submit", { payload });
}

export async function poolRefreshBoundOnchain(): Promise<Pool> {
  return invoke("pool_refresh_bound_onchain");
}

export async function poolUnbindOnchain(): Promise<Pool> {
  return invoke("pool_unbind_onchain");
}

export async function machineList(filter?: MachineFilter): Promise<Machine[]> {
  return invoke("machine_list", { filter: filter ?? null });
}

export async function machineAdd(payload: MachineAddPayload): Promise<Machine> {
  return invoke("machine_add", { payload });
}

export async function machineRemove(machineId: number): Promise<void> {
  await invoke("machine_remove", { machineId });
}

export async function machinePreflight(machineId: number): Promise<PreflightReport> {
  return invoke("machine_preflight", { machineId });
}

export async function machineRuntimeProbe(machineId: number): Promise<RuntimeProbe> {
  return invoke("machine_runtime_probe", { machineId });
}

export async function sshAgentListKeys(): Promise<SshKeyInfo[]> {
  return invoke("ssh_agent_list_keys");
}

export async function sshAgentAddKey(keyPath: string): Promise<SshKeyInfo[]> {
  return invoke("ssh_agent_add_key", { keyPath });
}

export async function deployStart(payload: DeployPayload): Promise<string> {
  return invoke("deploy_start", { payload });
}

export async function deployStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("deploy_status", { taskId });
}

export async function deployCancel(taskId: string): Promise<void> {
  await invoke("deploy_cancel", { taskId });
}

export async function monitorSnapshot(machineIds?: number[]): Promise<MonitorSnapshot[]> {
  return invoke("monitor_snapshot", { machineIds: machineIds ?? null });
}

export async function monitorStartPolling(
  machineIds?: number[],
  intervalSeconds?: number,
): Promise<{ running: boolean; interval_seconds: number }> {
  return invoke("monitor_start_polling", {
    machineIds: machineIds ?? null,
    intervalSeconds: intervalSeconds ?? null,
  });
}

export async function monitorStopPolling(): Promise<{ running: boolean; interval_seconds: number }> {
  return invoke("monitor_stop_polling");
}

export async function kesStatusAll(): Promise<KesStatus[]> {
  return invoke("kes_status_all");
}

export async function kesGenerate(machineId: number): Promise<KesSignRequest> {
  return invoke("kes_generate", { machineId });
}

export async function kesPrepareBundle(
  machineId: number,
  includeCli: boolean,
  targetPlatform: string | null,
): Promise<KesBundleResult> {
  return invoke("kes_prepare_bundle", { machineId, includeCli, targetPlatform });
}

export async function kesImportCert(machineId: number, certPath: string): Promise<string> {
  return invoke("kes_import_cert", { machineId, certPath });
}

export async function kesPushStart(taskId: string): Promise<string> {
  return invoke("kes_push_start", { taskId });
}

export async function kesRotationStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("kes_rotation_status", { taskId });
}

export async function runtimeApplyConfig(machineId: number): Promise<string> {
  return invoke("runtime_apply_config", { machineId });
}

export async function runtimeConfigStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("runtime_config_status", { taskId });
}

export async function runtimeRestart(machineId: number): Promise<string> {
  return invoke("runtime_restart", { machineId });
}

export async function runtimeRestartStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("runtime_restart_status", { taskId });
}

export async function taskRecentList(limit?: number): Promise<RecentTaskSummary[]> {
  return invoke("task_recent_list", { limit: limit ?? null });
}

export async function taskLogQuery(query?: TaskLogQueryPayload): Promise<TaskLogPage> {
  return invoke("task_log_query", { query: query ?? null });
}

export async function observabilityGatewayStatus(): Promise<ObservabilityGatewayStatus> {
  return invoke("observability_gateway_status");
}

export async function observabilityBootstrapStart(payload?: ObservabilityTaskPayload): Promise<string> {
  return invoke("observability_bootstrap_start", { payload: payload ?? null });
}

export async function observabilityBootstrapStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("observability_bootstrap_status", { taskId });
}

export async function observabilityRollbackStart(payload?: ObservabilityTaskPayload): Promise<string> {
  return invoke("observability_rollback_start", { payload: payload ?? null });
}

export async function observabilityRollbackStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("observability_rollback_status", { taskId });
}

export async function upgradeStart(payload: UpgradePayload): Promise<string> {
  return invoke("upgrade_start", { payload });
}

export async function upgradeStatus(taskId: string): Promise<DeployTaskStatus> {
  return invoke("upgrade_status", { taskId });
}

export async function upgradeConfirmNext(taskId: string): Promise<void> {
  await invoke("upgrade_confirm_next", { taskId });
}

export async function upgradeRollback(taskId: string, machineId: number): Promise<string> {
  return invoke("upgrade_rollback", { taskId, machineId });
}

// --- Staking ---

export async function poolStakingSummary(): Promise<import("./types").StakingSummary> {
  return invoke("pool_staking_summary");
}

export async function poolStakingHistory(epochCount?: number): Promise<import("./types").StakingEpochEntry[]> {
  return invoke("pool_staking_history", { epochCount: epochCount ?? null });
}

export async function poolDelegatorList(): Promise<import("./types").Delegator[]> {
  return invoke("pool_delegator_list");
}
