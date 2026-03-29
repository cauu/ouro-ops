import { type QueryClient, useQuery } from "@tanstack/react-query";
import {
  kesStatusAll,
  machineList,
  observabilityGatewayStatus,
  taskLogQuery,
  taskRecentList,
} from "./ipc";
import type {
  KesStatus,
  Machine,
  MachineFilter,
  ObservabilityGatewayStatus,
  RecentTaskSummary,
  TaskLogPage,
  TaskLogQueryPayload,
} from "./types";

const FOREGROUND_MS = 15_000;
const BACKGROUND_MS = 60_000;

function visibilityInterval(): number {
  return typeof document !== "undefined" && document.visibilityState === "hidden"
    ? BACKGROUND_MS
    : FOREGROUND_MS;
}

// --- Dashboard prefetch (called from App.tsx after pool_get) ---

export function prefetchDashboardQueries(client: QueryClient, poolId: number): void {
  void client.prefetchQuery({
    queryKey: ["dashboard", "kes", poolId],
    queryFn: kesStatusAll,
    staleTime: 10_000,
  });
  void client.prefetchQuery({
    queryKey: ["dashboard", "tasks", poolId, 8],
    queryFn: () => taskRecentList(8),
    staleTime: 10_000,
  });
}

// --- Dashboard ---

export function useKesStatusQuery(poolId: number | undefined) {
  return useQuery<KesStatus[]>({
    queryKey: ["dashboard", "kes", poolId],
    queryFn: kesStatusAll,
    enabled: poolId != null,
    staleTime: 10_000,
    refetchInterval: visibilityInterval,
    refetchOnWindowFocus: true,
    placeholderData: (prev) => prev,
    retry: 2,
  });
}

export function useRecentTasksQuery(poolId: number | undefined, limit = 8) {
  return useQuery<RecentTaskSummary[]>({
    queryKey: ["dashboard", "tasks", poolId, limit],
    queryFn: () => taskRecentList(limit),
    enabled: poolId != null,
    staleTime: 10_000,
    refetchInterval: visibilityInterval,
    refetchOnWindowFocus: true,
    placeholderData: (prev) => prev,
    retry: 2,
  });
}

// --- Operation Logs ---

export function useTaskLogQuery(query: TaskLogQueryPayload) {
  return useQuery<TaskLogPage>({
    queryKey: ["logs", query.keyword, query.status, query.task_type, query.page, query.page_size],
    queryFn: () => taskLogQuery(query),
    staleTime: 10_000,
    placeholderData: (prev) => prev,
    retry: 1,
  });
}

// --- KesManager ---

export function useKesStatusListQuery() {
  return useQuery<KesStatus[]>({
    queryKey: ["kes", "statuses"],
    queryFn: kesStatusAll,
    staleTime: 10_000,
    refetchOnWindowFocus: true,
    placeholderData: (prev) => prev,
    retry: 2,
  });
}

// --- Telemetry API ---

export function useGatewayStatusQuery() {
  return useQuery<ObservabilityGatewayStatus>({
    queryKey: ["telemetry", "gateway"],
    queryFn: observabilityGatewayStatus,
    staleTime: 10_000,
    refetchInterval: visibilityInterval,
    refetchOnWindowFocus: true,
    placeholderData: (prev) => prev,
    retry: 2,
  });
}

// --- Shared: Machine List ---

export function useMachineListQuery(filter?: MachineFilter) {
  return useQuery<Machine[]>({
    queryKey: ["machines", filter?.role ?? null, filter?.network ?? null],
    queryFn: () => machineList(filter),
    staleTime: 30_000,
    refetchOnWindowFocus: true,
    placeholderData: (prev) => prev,
    retry: 2,
  });
}
