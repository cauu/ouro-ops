import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { dbVersion, ping, runPlaybookTest } from "../lib/ipc";
import type { DbVersionResult } from "../lib/types";

export default function Dashboard() {
  const [status, setStatus] = useState<string>("loading");
  const [dbInfo, setDbInfo] = useState<DbVersionResult | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [events, setEvents] = useState<string[]>([]);

  useEffect(() => {
    void (async () => {
      try {
        await ping();
        setStatus("Sidecar OK");
        const version = await dbVersion();
        setDbInfo(version);
      } catch (error) {
        setStatus(`Error: ${String(error)}`);
      }
    })();
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const eventNames = ["task:progress", "task:log", "task:completed", "task:failed"];
    eventNames.forEach((eventName) => {
      void listen(eventName, (event) => {
        setEvents((prev) => [...prev.slice(-59), `${eventName}: ${JSON.stringify(event.payload)}`]);
      }).then((unlisten) => {
        unlisteners.push(unlisten);
      });
    });
    return () => {
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  const handleRunTest = async () => {
    try {
      const id = await runPlaybookTest();
      setTaskId(id);
    } catch (error) {
      setEvents((prev) => [...prev, `run_playbook_test error: ${String(error)}`]);
    }
  };

  return (
    <section className="space-y-4">
      <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
      <p className="text-sm text-zinc-300">
        <span className="font-medium text-zinc-100">Sidecar:</span> {status}
      </p>
      {dbInfo && (
        <p className="text-sm text-zinc-300">
          <span className="font-medium text-zinc-100">DB:</span> user_version={dbInfo.user_version}
        </p>
      )}
      <button
        type="button"
        onClick={handleRunTest}
        className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
      >
        Run playbook test (mock)
      </button>
      {taskId && <p className="text-xs text-zinc-400">Task ID: {taskId}</p>}
      <div className="max-h-72 overflow-y-auto rounded-md border border-zinc-800 bg-black/30 p-3 font-mono text-xs">
        {events.length === 0
          ? "Events will appear here."
          : events.map((line, index) => (
              <div key={`${index}-${line.slice(0, 16)}`} className="break-all py-0.5 text-zinc-300">
                {line}
              </div>
            ))}
      </div>
    </section>
  );
}
