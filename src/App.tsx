import { useCallback, useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import { poolGet } from "./lib/ipc";
import type { Pool } from "./lib/types";
import Dashboard from "./pages/Dashboard";
import MachineManager from "./pages/MachineManager";
import Settings from "./pages/Settings";
import SetupWizard from "./pages/SetupWizard";

function LoadingScreen() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-zinc-950 text-zinc-300">
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
        <Route path="/" element={<Dashboard />} />
        <Route path="/machines" element={pool ? <MachineManager pool={pool} /> : null} />
        <Route
          path="/settings"
          element={
            pool ? (
              <Settings
                pool={pool}
                onUpdated={(updatedPool) => {
                  setPool(updatedPool);
                }}
              />
            ) : null
          }
        />
      </Route>

      <Route path="*" element={<Navigate to={pool ? "/" : "/setup"} replace />} />
    </Routes>
  );
}

export default App;
