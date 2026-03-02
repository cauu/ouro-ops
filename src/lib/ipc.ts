import { invoke } from "@tauri-apps/api/core";

/**
 * Tauri invoke 封装
 */
export async function ping(): Promise<void> {
  await invoke("ping");
}

export async function dbVersion(): Promise<{ user_version: number; tables: Record<string, boolean> }> {
  return invoke("db_version");
}

export async function runPlaybookTest(): Promise<string> {
  return invoke("run_playbook_test");
}
