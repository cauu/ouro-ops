import { useQuery } from "@tanstack/react-query";
import { kesStatusAll, taskRecentList } from "./ipc";
import type { KesStatus, RecentTaskSummary } from "./types";

const FOREGROUND_MS = 15_000;
const BACKGROUND_MS = 60_000;

function visibilityInterval(): number {
  return typeof document !== "undefined" && document.visibilityState === "hidden"
    ? BACKGROUND_MS
    : FOREGROUND_MS;
}

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
