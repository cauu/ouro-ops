import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { monitorSnapshot, monitorStartPolling, monitorStopPolling } from "./ipc";
import type { MonitorSnapshot } from "./types";

type TelemetryPhase = "idle" | "loading_cache" | "syncing_live" | "live" | "degraded";

type MonitorStoreState = {
  snapshots: MonitorSnapshot[];
  status: string;
  polling: boolean;
  telemetryPhase: TelemetryPhase;
  usingCachedData: boolean;
  lastCollectedAt: string | null;
  lastError: string | null;
};

const DEFAULT_STATE: MonitorStoreState = {
  snapshots: [],
  status: "idle",
  polling: false,
  telemetryPhase: "idle",
  usingCachedData: false,
  lastCollectedAt: null,
  lastError: null,
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

function pickLatestCollectedAt(snapshots: MonitorSnapshot[]): string | null {
  let latest: string | null = null;
  snapshots.forEach((snapshot) => {
    if (!snapshot.collected_at) {
      return;
    }
    if (latest === null || snapshot.collected_at > latest) {
      latest = snapshot.collected_at;
    }
  });
  return latest;
}

async function ensureEventListeners(): Promise<void> {
  if (unlisteners.length > 0) {
    return;
  }
  const snapshotUnlisten = await listen<MonitorSnapshot[]>("monitor:snapshot", (event) => {
    const latestCollectedAt = pickLatestCollectedAt(event.payload);
    setState({
      snapshots: event.payload,
      status: "Live telemetry updated",
      polling: true,
      telemetryPhase: "live",
      usingCachedData: false,
      lastCollectedAt: latestCollectedAt,
      lastError: null,
    });
  });
  const errorUnlisten = await listen<{ message?: string }>("monitor:error", (event) => {
    const message = event.payload?.message ?? "unknown error";
    setState({
      status: "Live telemetry delayed, showing cached data.",
      polling: true,
      telemetryPhase: "degraded",
      usingCachedData: state.snapshots.length > 0,
      lastError: message,
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
  setState({
    status: "Loading cached telemetry...",
    polling: false,
    telemetryPhase: "loading_cache",
    usingCachedData: false,
    lastError: null,
  });
  try {
    const cachedSnapshots = await monitorSnapshot();
    setState({
      snapshots: cachedSnapshots,
      status: cachedSnapshots.length > 0 ? "Loaded cached telemetry" : "No cached telemetry yet",
      usingCachedData: cachedSnapshots.length > 0,
      lastCollectedAt: pickLatestCollectedAt(cachedSnapshots),
    });
  } catch (error) {
    setState({
      status: "Cached telemetry unavailable",
      telemetryPhase: "degraded",
      usingCachedData: false,
      lastError: String(error),
    });
  }

  setState({
    status: state.snapshots.length > 0 ? "Refreshing live telemetry..." : "Waiting for live telemetry...",
    telemetryPhase: "syncing_live",
  });

  try {
    await monitorStartPolling(undefined, intervalSeconds);
    setState({ polling: true });
  } catch (error) {
    started = false;
    setState({
      status:
        state.snapshots.length > 0
          ? "Live telemetry unavailable, showing cached data."
          : "Live telemetry unavailable",
      polling: false,
      telemetryPhase: "degraded",
      usingCachedData: state.snapshots.length > 0,
      lastError: String(error),
    });
  }
}

export async function refreshMonitorStore(): Promise<void> {
  setState({
    status: "Refreshing live telemetry...",
    telemetryPhase: "syncing_live",
  });
  try {
    const snapshots = await monitorSnapshot();
    setState({
      snapshots,
      status: "Live telemetry updated",
      telemetryPhase: "live",
      usingCachedData: false,
      lastCollectedAt: pickLatestCollectedAt(snapshots),
      lastError: null,
    });
  } catch (error) {
    setState({
      status: "Live telemetry delayed, showing cached data.",
      telemetryPhase: "degraded",
      usingCachedData: state.snapshots.length > 0,
      lastError: String(error),
    });
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
    setState({
      status: "Monitor stop error",
      polling: false,
      telemetryPhase: "degraded",
      lastError: String(error),
    });
    return;
  }
  setState({
    polling: false,
    status: "Monitor polling stopped",
    telemetryPhase: "idle",
  });
}
