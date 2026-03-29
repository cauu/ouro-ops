import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdateStatus {
  available: boolean;
  version?: string;
  body?: string;
  error?: string;
}

export async function checkForUpdate(): Promise<UpdateStatus> {
  try {
    const update = await check();
    if (!update) {
      return { available: false };
    }
    return {
      available: true,
      version: update.version,
      body: update.body ?? undefined,
    };
  } catch (e) {
    return { available: false, error: String(e) };
  }
}

export async function installUpdate(): Promise<void> {
  const update = await check();
  if (!update) return;
  await update.downloadAndInstall();
  await relaunch();
}
