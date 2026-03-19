import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import { poolGet } from "./lib/ipc";
import {
  setMonitorStorePollingInterval,
  startMonitorStore,
  stopMonitorStore,
} from "./lib/monitorStore";
import type { Pool } from "./lib/types";
import Dashboard from "./pages/Dashboard";
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
      setPool(currentPool);
    } catch {
      setPool(null);
    } finally {
      setBooting(false);
    }
  }, []);

  useEffect(() => {
    void refreshPool();
  }, [refreshPool]);

  // Monitor store lifecycle (event-driven telemetry — not migrated to Query)
  useEffect(() => {
    if (!pool) return;
    let disposed = false;

    const applyMonitorInterval = async (seconds: number) => {
      try {
        await setMonitorStorePollingInterval(seconds);
      } catch {
        /* keep going */
      }
    };

    const onVisibilityChange = () => {
      const hidden =
        typeof document !== "undefined" && document.visibilityState === "hidden";
      void applyMonitorInterval(hidden ? BACKGROUND_INTERVAL_S : FOREGROUND_INTERVAL_S);
    };

    void (async () => {
      const initial =
        typeof document !== "undefined" && document.visibilityState === "hidden"
          ? BACKGROUND_INTERVAL_S
          : FOREGROUND_INTERVAL_S;
      await startMonitorStore(initial);
      if (disposed) return;
    })();

    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", onVisibilityChange);
      void stopMonitorStore();
    };
  }, [pool]);

  // Clean query cache on pool switch
  useEffect(() => {
    if (!pool) return;
    return () => {
      queryClient.removeQueries({ queryKey: ["dashboard"] });
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
          <Route path="/" element={<Dashboard poolId={pool?.id} />} />
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
