import { useCallback, useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import {
  refreshDashboardData,
  resetDashboardStore,
  startDashboardPolling,
  stopDashboardPolling,
} from "./lib/dashboardStore";
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

const FOREGROUND_POLL_INTERVAL_SECONDS = 15;
const BACKGROUND_POLL_INTERVAL_SECONDS = 60;

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

  useEffect(() => {
    if (!pool) return;
    let disposed = false;

    const currentIntervalSeconds = () =>
      typeof document !== "undefined" && document.visibilityState === "hidden"
        ? BACKGROUND_POLL_INTERVAL_SECONDS
        : FOREGROUND_POLL_INTERVAL_SECONDS;

    const applyPollingMode = async (intervalSeconds: number) => {
      try {
        await setMonitorStorePollingInterval(intervalSeconds);
      } catch {
        // Keep auxiliary polling alive even if monitor interval update fails.
      }
      if (disposed) {
        return;
      }
      startDashboardPolling(intervalSeconds);
    };

    const onVisibilityChange = () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        void applyPollingMode(BACKGROUND_POLL_INTERVAL_SECONDS);
        return;
      }
      void (async () => {
        await applyPollingMode(FOREGROUND_POLL_INTERVAL_SECONDS);
        await refreshDashboardData();
      })();
    };

    const onWindowFocus = () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
      void (async () => {
        await applyPollingMode(FOREGROUND_POLL_INTERVAL_SECONDS);
        await refreshDashboardData();
      })();
    };

    void (async () => {
      const initialInterval = currentIntervalSeconds();
      await startMonitorStore(initialInterval);
      if (disposed) {
        return;
      }
      startDashboardPolling(initialInterval);
      await refreshDashboardData();
    })();

    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }
    if (typeof window !== "undefined") {
      window.addEventListener("focus", onWindowFocus);
    }

    return () => {
      disposed = true;
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
      if (typeof window !== "undefined") {
        window.removeEventListener("focus", onWindowFocus);
      }
      void stopMonitorStore();
      stopDashboardPolling();
      resetDashboardStore();
    };
  }, [pool]);

  if (booting) {
    return <LoadingScreen />;
  }

  return (
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
        <Route
          path="/"
          element={
            <Dashboard />
          }
        />
        <Route path="/logs" element={pool ? <OperationLogs /> : null} />
        <Route path="/kes" element={pool ? <KesManager poolTicker={pool.ticker} /> : null} />
        <Route path="/telemetry" element={pool ? <TelemetryApi /> : null} />
        <Route path="/deploy" element={pool ? <DeployWizard pool={pool} /> : null} />
        <Route path="/upgrade" element={pool ? <UpgradeWizard poolTicker={pool.ticker} /> : null} />
        <Route
          path="/settings"
          element={pool ? <Settings pool={pool} /> : null}
        />
      </Route>

      <Route path="*" element={<Navigate to={pool ? "/" : "/setup"} replace />} />
    </Routes>
  );
}

export default App;
