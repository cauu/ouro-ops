import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { monitorSnapshot, monitorStartPolling, monitorStopPolling } from "./ipc";
import type { MonitorSnapshot } from "./types";

type MonitorStoreState = {
  snapshots: MonitorSnapshot[];
  status: string;
  polling: boolean;
};

const DEFAULT_STATE: MonitorStoreState = {
  snapshots: [],
  status: "idle",
  polling: false,
};

let state: MonitorStoreState = DEFAULT_STATE;
const listeners = new Set<() => void>();
let started = false;
let unlisteners: UnlistenFn[] = [];

function emit(): void {
  for (const listener of listeners) {
    listener();
  }
}

function setState(partial: Partial<MonitorStoreState>): void {
  state = { ...state, ...partial };
  emit();
}

async function ensureEventListeners(): Promise<void> {
  if (unlisteners.length > 0) {
    return;
  }
  const snapshotUnlisten = await listen<MonitorSnapshot[]>("monitor:snapshot", (event) => {
    setState({
      snapshots: event.payload,
      status: `Updated ${new Date().toLocaleTimeString()}`,
      polling: true,
    });
  });
  const errorUnlisten = await listen<{ message?: string }>("monitor:error", (event) => {
    setState({
      status: `Monitor error: ${event.payload?.message ?? "unknown error"}`,
      polling: true,
    });
  });
  unlisteners = [snapshotUnlisten, errorUnlisten];
}

export function subscribeMonitorStore(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getMonitorStoreSnapshot(): MonitorStoreState {
  return state;
}

export function useMonitorStore(): MonitorStoreState {
  return useSyncExternalStore(subscribeMonitorStore, getMonitorStoreSnapshot);
}

export async function startMonitorStore(intervalSeconds = 30): Promise<void> {
  await ensureEventListeners();
  if (started) {
    return;
  }
  started = true;
  setState({ status: "Starting background monitor polling...", polling: false });
  try {
    await monitorStartPolling(undefined, intervalSeconds);
  } catch (error) {
    started = false;
    setState({ status: `Monitor error: ${String(error)}`, polling: false });
    throw error;
  }
}

export async function refreshMonitorStore(): Promise<void> {
  setState({ status: "Refreshing monitor snapshot..." });
  try {
    const snapshots = await monitorSnapshot();
    setState({
      snapshots,
      status: `Updated ${new Date().toLocaleTimeString()}`,
    });
  } catch (error) {
    setState({ status: `Monitor error: ${String(error)}` });
  }
}

export async function stopMonitorStore(): Promise<void> {
  if (!started && unlisteners.length === 0) {
    return;
  }
  started = false;
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
  try {
    await monitorStopPolling();
  } catch (error) {
    setState({ status: `Monitor stop error: ${String(error)}`, polling: false });
    return;
  }
  setState({ polling: false, status: "Monitor polling stopped" });
}
