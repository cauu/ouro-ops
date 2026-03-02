import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ping, dbVersion, runPlaybookTest } from "./lib/ipc";

function App() {
  const [status, setStatus] = useState<string>("loading");
  const [dbInfo, setDbInfo] = useState<{ user_version: number; tables: Record<string, boolean> } | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [events, setEvents] = useState<string[]>([]);

  useEffect(() => {
    (async () => {
      try {
        await ping();
        setStatus("Sidecar OK");
        const v = await dbVersion();
        setDbInfo(v as { user_version: number; tables: Record<string, boolean> });
      } catch (e) {
        setStatus(`Error: ${String(e)}`);
      }
    })();
  }, []);

  useEffect(() => {
    const unlisteners: (() => void)[] = [];
    ["task:progress", "task:completed", "task:failed"].forEach((ev) => {
      listen(ev, (payload) => {
        setEvents((prev) => [...prev.slice(-49), `${ev}: ${JSON.stringify(payload)}`]);
      }).then((fn) => unlisteners.push(fn));
    });
    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  const runTest = async () => {
    try {
      const id = await runPlaybookTest();
      setTaskId(id);
    } catch (e) {
      setEvents((prev) => [...prev, `run_playbook_test error: ${String(e)}`]);
    }
  };

  return (
    <div className="min-h-screen bg-zinc-900 text-zinc-100 p-6 font-sans">
      <h1 className="text-2xl font-bold mb-4">Ouro Ops — Phase 1</h1>
      <p className="mb-2">
        <strong>Status:</strong> {status}
      </p>
      {dbInfo && (
        <p className="mb-2">
          <strong>DB user_version:</strong> {dbInfo.user_version} | tables:{" "}
          {Object.entries(dbInfo.tables)
            .filter(([, v]) => v)
            .map(([k]) => k)
            .join(", ")}
        </p>
      )}
      <button
        type="button"
        onClick={runTest}
        className="px-4 py-2 bg-blue-600 rounded hover:bg-blue-700 mb-4"
      >
        Run playbook test (mock)
      </button>
      {taskId && <p className="mb-2 text-sm text-zinc-400">TaskId: {taskId}</p>}
      <div className="font-mono text-sm bg-black/40 p-3 rounded max-h-48 overflow-y-auto">
        {events.length === 0 ? "Events will appear here…" : events.map((e, i) => <div key={i}>{e}</div>)}
      </div>
    </div>
  );
}

export default App;
