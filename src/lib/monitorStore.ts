import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { monitorSnapshot, monitorStartPolling, monitorStopPolling } from "./ipc";
import type { MonitorSnapshot } from "./types";

export type TelemetryPhase = "idle" | "loading_cache" | "syncing_live" | "live" | "degraded";
export type TelemetryBehavior = "idle" | "cache_ready" | "syncing_live" | "live" | "degraded_retrying";

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

export function resolveTelemetryBehavior(input: {
  telemetryPhase: TelemetryPhase;
  usingCachedData: boolean;
  snapshots: MonitorSnapshot[];
}): TelemetryBehavior {
  if (input.telemetryPhase === "degraded") {
    return "degraded_retrying";
  }
  if (input.telemetryPhase === "syncing_live") {
    return "syncing_live";
  }
  if (input.telemetryPhase === "loading_cache") {
    return input.usingCachedData || input.snapshots.length > 0 ? "cache_ready" : "syncing_live";
  }
  if (input.telemetryPhase === "live") {
    return input.usingCachedData ? "cache_ready" : "live";
  }
  return "idle";
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

export async function ensureMonitorEventListeners(): Promise<void> {
  return ensureEventListeners();
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
      status: "Telemetry refresh failed; keeping cached data and retrying.",
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
    status: "Starting telemetry polling...",
    polling: false,
    telemetryPhase: "syncing_live",
    usingCachedData: false,
    lastError: null,
  });

  // Skip eager monitorSnapshot() call — it holds the DB lock during HTTP
  // calls to relay API, blocking other fast DB queries (kes_status_all,
  // task_recent_list). Instead, start polling directly; the first poll
  // cycle will push data via the monitor:snapshot event.
  try {
    await monitorStartPolling(undefined, intervalSeconds);
    setState({ polling: true });
  } catch (error) {
    started = false;
    setState({
      status: "Live telemetry unavailable; retrying.",
      polling: false,
      telemetryPhase: "degraded",
      usingCachedData: false,
      lastError: String(error),
    });
  }
}

export async function setMonitorStorePollingInterval(intervalSeconds: number): Promise<void> {
  await ensureEventListeners();
  if (!started) {
    await startMonitorStore(intervalSeconds);
    return;
  }
  try {
    await monitorStartPolling(undefined, intervalSeconds);
    setState({ polling: true });
  } catch (error) {
    setState({
      status:
        state.snapshots.length > 0
          ? "Live telemetry unavailable; showing cached data and retrying."
          : "Live telemetry unavailable; retrying.",
      polling: false,
      telemetryPhase: "degraded",
      usingCachedData: state.snapshots.length > 0,
      lastError: String(error),
    });
  }
}

export async function refreshMonitorStore(): Promise<void> {
  setState({
    status: "Refreshing live telemetry in background...",
    telemetryPhase: "syncing_live",
  });
  try {
    const snapshots = await monitorSnapshot();
    const hadSnapshots = state.snapshots.length > 0;
    if (snapshots.length > 0) {
      setState({
        snapshots,
        status: "Live telemetry updated.",
        telemetryPhase: "live",
        usingCachedData: false,
        lastCollectedAt: pickLatestCollectedAt(snapshots),
        lastError: null,
      });
    } else if (hadSnapshots) {
      setState({
        status: "Refresh returned no data; showing previous snapshot.",
        telemetryPhase: "degraded",
        usingCachedData: true,
        lastError: "本次刷新未返回数据，继续展示上次数据。",
      });
    } else {
      setState({
        snapshots,
        status: "Live telemetry updated.",
        telemetryPhase: "live",
        usingCachedData: false,
        lastCollectedAt: null,
        lastError: null,
      });
    }
  } catch (error) {
    setState({
      status: "Telemetry refresh failed; keeping cached data and retrying.",
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
