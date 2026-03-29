import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import { poolGet } from "./lib/ipc";
import {
  ensureMonitorEventListeners,
  setMonitorStorePollingInterval,
  startMonitorStore,
  stopMonitorStore,
} from "./lib/monitorStore";
import { prefetchDashboardQueries } from "./lib/queries";
import type { Pool } from "./lib/types";
import BindPool from "./pages/BindPool";
import Dashboard from "./pages/Dashboard";
import Delegators from "./pages/Delegators";
import DeployWizard from "./pages/DeployWizard";
import KesManager from "./pages/KesManager";
import OperationLogs from "./pages/OperationLogs";
import Settings from "./pages/Settings";
import SetupWizard from "./pages/SetupWizard";
import TelemetryApi from "./pages/TelemetryApi";
import UpgradeWizard from "./pages/UpgradeWizard";

const FOREGROUND_INTERVAL_S = 15;
const BACKGROUND_INTERVAL_S = 60;

function LoadingScreen() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-slate-100 text-slate-600">
      <p className="text-sm">Initializing...</p>
    </div>
  );
}

function App() {
  const [booting, setBooting] = useState(true);
  const [pool, setPool] = useState<Pool | null>(null);

  const queryClient = useMemo(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 10_000,
            gcTime: 5 * 60_000,
            retry: 2,
          },
        },
      }),
    [],
  );

  const refreshPool = useCallback(async () => {
    try {
      const currentPool = await poolGet();
      if (currentPool) {
        prefetchDashboardQueries(queryClient, currentPool.id);
        void startMonitorStore(
          typeof document !== "undefined" && document.visibilityState === "hidden"
            ? BACKGROUND_INTERVAL_S
            : FOREGROUND_INTERVAL_S,
        );
      }
      setPool(currentPool);
    } catch {
      setPool(null);
    } finally {
      setBooting(false);
    }
  }, [queryClient]);

  useEffect(() => {
    void ensureMonitorEventListeners();
    void refreshPool();
  }, [refreshPool]);

  // Monitor store lifecycle: started in refreshPool, visibility interval managed here
  useEffect(() => {
    if (!pool) return;

    const onVisibilityChange = () => {
      const hidden =
        typeof document !== "undefined" && document.visibilityState === "hidden";
      void setMonitorStorePollingInterval(hidden ? BACKGROUND_INTERVAL_S : FOREGROUND_INTERVAL_S).catch(() => {});
    };

    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      void stopMonitorStore();
    };
  }, [pool]);

  // Clean stale pool's query cache on pool switch
  useEffect(() => {
    if (!pool) return;
    const poolId = pool.id;
    return () => {
      queryClient.removeQueries({ queryKey: ["dashboard", "kes", poolId] });
      queryClient.removeQueries({ queryKey: ["dashboard", "tasks", poolId] });
    };
  }, [pool, queryClient]);

  if (booting) {
    return <LoadingScreen />;
  }

  return (
    <QueryClientProvider client={queryClient}>
      <Routes>
        <Route
          path="/setup"
          element={
            pool ? (
              <Navigate to="/" replace />
            ) : (
              <SetupWizard
                onCreated={(createdPool) => {
                  setPool(createdPool);
                }}
              />
            )
          }
        />

        <Route element={pool ? <Layout pool={pool} /> : <Navigate to="/setup" replace />}>
          <Route path="/" element={<Dashboard poolId={pool?.id} onchainPoolId={pool?.onchain_pool_id} poolTicker={pool?.ticker} poolNetwork={pool?.network} />} />
          <Route path="/bind-pool" element={pool ? <BindPool poolTicker={pool.ticker} onBound={setPool} /> : null} />
          <Route path="/delegators" element={pool ? <Delegators onchainPoolId={pool.onchain_pool_id} /> : null} />
          <Route path="/logs" element={pool ? <OperationLogs /> : null} />
          <Route path="/kes" element={pool ? <KesManager poolTicker={pool.ticker} /> : null} />
          <Route path="/telemetry" element={pool ? <TelemetryApi /> : null} />
          <Route path="/deploy" element={pool ? <DeployWizard pool={pool} /> : null} />
          <Route path="/upgrade" element={pool ? <UpgradeWizard poolTicker={pool.ticker} /> : null} />
          <Route path="/settings" element={pool ? <Settings pool={pool} /> : null} />
        </Route>

        <Route path="*" element={<Navigate to={pool ? "/" : "/setup"} replace />} />
      </Routes>
    </QueryClientProvider>
  );
}

export default App;
