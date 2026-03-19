import { useCallback, useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import { refreshDashboardData, startDashboardPolling, stopDashboardPolling } from "./lib/dashboardStore";
import { poolGet } from "./lib/ipc";
import { startMonitorStore, stopMonitorStore } from "./lib/monitorStore";
import type { Pool } from "./lib/types";
import Dashboard from "./pages/Dashboard";
import DeployWizard from "./pages/DeployWizard";
import KesManager from "./pages/KesManager";
import OperationLogs from "./pages/OperationLogs";
import Settings from "./pages/Settings";
import SetupWizard from "./pages/SetupWizard";
import TelemetryApi from "./pages/TelemetryApi";
import UpgradeWizard from "./pages/UpgradeWizard";

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
    void startMonitorStore(15);
    void refreshDashboardData();
    startDashboardPolling(15);
    return () => {
      void stopMonitorStore();
      stopDashboardPolling();
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
