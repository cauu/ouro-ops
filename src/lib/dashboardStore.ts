import { useSyncExternalStore } from "react";
import { kesStatusAll, taskRecentList } from "./ipc";
import type { KesStatus, RecentTaskSummary } from "./types";

type DashboardStoreState = {
  kesStatuses: KesStatus[];
  recentTasks: RecentTaskSummary[];
  refreshError: string | null;
};

const DEFAULT_STATE: DashboardStoreState = {
  kesStatuses: [],
  recentTasks: [],
  refreshError: null,
};

let state: DashboardStoreState = DEFAULT_STATE;
const listeners = new Set<() => void>();
let refreshInFlight = false;
let refreshTimer: number | null = null;
let storeGeneration = 0;

function emit(): void {
  for (const listener of listeners) {
    listener();
  }
}

function setState(partial: Partial<DashboardStoreState>): void {
  state = { ...state, ...partial };
  emit();
}

export function subscribeDashboardStore(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getDashboardStoreSnapshot(): DashboardStoreState {
  return state;
}

export function useDashboardStore(): DashboardStoreState {
  return useSyncExternalStore(subscribeDashboardStore, getDashboardStoreSnapshot);
}

export async function refreshDashboardData(): Promise<void> {
  const requestGeneration = storeGeneration;
  if (refreshInFlight) return;
  refreshInFlight = true;
  try {
    const [kesResult, taskResult] = await Promise.allSettled([
      kesStatusAll(),
      taskRecentList(8),
    ]);
    if (requestGeneration !== storeGeneration) {
      return;
    }

    const failures: string[] = [];
    const partial: Partial<DashboardStoreState> = {};
    if (kesResult.status === "fulfilled") {
      partial.kesStatuses = kesResult.value;
    } else {
      failures.push("KES");
    }
    if (taskResult.status === "fulfilled") {
      partial.recentTasks = taskResult.value;
    } else {
      failures.push("日志");
    }
    partial.refreshError =
      failures.length > 0
        ? `部分数据刷新失败（${failures.join(" / ")}），将自动重试。`
        : null;
    setState(partial);
  } finally {
    if (requestGeneration === storeGeneration) {
      refreshInFlight = false;
    }
  }
}

export function startDashboardPolling(intervalSeconds: number): void {
  stopDashboardPolling();
  refreshTimer = window.setInterval(() => {
    void refreshDashboardData();
  }, intervalSeconds * 1000);
}

export function stopDashboardPolling(): void {
  if (refreshTimer != null) {
    window.clearInterval(refreshTimer);
    refreshTimer = null;
  }
}

export function resetDashboardStore(): void {
  storeGeneration += 1;
  refreshInFlight = false;
  stopDashboardPolling();
  state = {
    ...DEFAULT_STATE,
  };
  emit();
}
