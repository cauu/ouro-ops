import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState } from "react";
import type { TaskLogEvent } from "../lib/types";

interface TaskLogStreamProps {
  taskId: string;
}

export default function TaskLogStream({ taskId }: TaskLogStreamProps) {
  const [logs, setLogs] = useState<TaskLogEvent[]>([]);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setLogs([]);
  }, [taskId]);

  useEffect(() => {
    let active = true;
    const unlistenPromise = listen<TaskLogEvent>("task:log", (event) => {
      if (!active || event.payload.task_id !== taskId) {
        return;
      }
      setLogs((prev) => {
        const next = [...prev, event.payload];
        if (next.length > 500) {
          return next.slice(next.length - 500);
        }
        return next;
      });
    });
    return () => {
      active = false;
      void unlistenPromise.then((unlisten) => {
        unlisten();
      });
    };
  }, [taskId]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  const hasLogs = useMemo(() => logs.length > 0, [logs]);

  return (
    <div className="rounded-lg border border-slate-200 bg-white p-3">
      <p className="mb-2 text-xs text-slate-500">TaskLogStream · task_id={taskId}</p>
      <div ref={scrollRef} className="h-72 overflow-y-auto rounded-md border border-slate-200 bg-slate-50 p-2 font-mono text-xs">
        {!hasLogs ? (
          <p className="text-slate-500">Waiting for logs...</p>
        ) : (
          <div className="space-y-1">
            {logs.map((log, idx) => (
              <div key={`${log.timestamp}-${idx}`} className={log.stream === "stderr" ? "text-red-700" : "text-slate-800"}>
                <span className="mr-2 text-slate-500">{log.timestamp || "-"}</span>
                <span>{log.line}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
