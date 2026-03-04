import { invoke } from "@tauri-apps/api/core";
import type {
  DbVersionResult,
  DeployPayload,
  DeployTaskStatus,
  Machine,
  MachineAddPayload,
  MachineFilter,
  Pool,
  PoolInitPayload,
  PoolUpdatePayload,
  PreflightReport,
  SshKeyInfo,
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
