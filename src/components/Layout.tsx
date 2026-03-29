import { useMemo, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { onDragRegionMouseDown } from "../lib/useDragRegion";
import type { Pool } from "../lib/types";
import Sidebar from "./Sidebar";

interface ToolbarContext {
  section: string;
  title: string;
  status: string;
  statusTone: "info" | "warn" | "ok";
}

interface LayoutProps {
  pool: Pool;
}

function networkLabel(network: Pool["network"]): string {
  if (network === "mainnet") {
    return "Mainnet";
  }
  if (network === "preprod") {
    return "Preprod";
  }
  return "Preview";
}

function toolbarContextFromPath(pathname: string): ToolbarContext {
  if (pathname === "/kes") {
    return {
      section: "Operations",
      title: "KES Rotate",
      status: "Ready",
      statusTone: "ok",
    };
  }
  if (pathname === "/upgrade") {
    return {
      section: "Operations",
      title: "Upgrade",
      status: "Guarded",
      statusTone: "warn",
    };
  }
  if (pathname === "/telemetry") {
    return {
      section: "Operations",
      title: "Telemetry API",
      status: "Managed",
      statusTone: "info",
    };
  }
  if (pathname === "/logs") {
    return {
      section: "Operations",
      title: "Operation Logs",
      status: "Indexed",
      statusTone: "info",
    };
  }
  if (pathname === "/deploy") {
    return {
      section: "Workspace",
      title: "Deploy",
      status: "In Progress",
      statusTone: "info",
    };
  }
  if (pathname === "/settings") {
    return {
      section: "Workspace",
      title: "Settings",
      status: "Editable",
      statusTone: "info",
    };
  }
  return {
    section: "Workspace",
    title: "Dashboard",
    status: "Synced",
    statusTone: "ok",
  };
}

function statusToneClass(tone: ToolbarContext["statusTone"]): string {
  if (tone === "ok") {
    return "border-emerald-200 bg-emerald-50 text-emerald-700";
  }
  if (tone === "warn") {
    return "border-amber-200 bg-amber-50 text-amber-700";
  }
  return "border-sky-200 bg-sky-50 text-sky-700";
}

export default function Layout({ pool }: LayoutProps) {
  const location = useLocation();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const toolbar = useMemo(() => toolbarContextFromPath(location.pathname), [location.pathname]);

  return (
    <div className="h-screen overflow-hidden bg-slate-100 text-slate-900">
      <div className="mx-auto flex h-full max-w-[1600px] overflow-hidden border-x border-slate-200/80 bg-white/80 shadow-app">
        <Sidebar
          network={pool.network}
          collapsed={sidebarCollapsed}
        />

        <section className="flex min-w-0 flex-1 flex-col">
          <header className="drag-region border-b border-slate-200/80 bg-white/78 backdrop-blur-md" data-tauri-drag-region onMouseDown={onDragRegionMouseDown}>
            <div className="drag-region flex h-14 items-center gap-4 px-5" data-tauri-drag-region>
              <button
                type="button"
                title={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"}
                aria-label={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"}
                onClick={() => setSidebarCollapsed((current) => !current)}
                className="no-drag inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-slate-300 bg-white text-slate-600 transition hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
              >
                {sidebarCollapsed ? (
                  <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                    <rect x="3.5" y="4" width="13" height="12" rx="2" />
                    <path d="M8.5 4v12" />
                    <path d="M11 7l3 3-3 3" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                ) : (
                  <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                    <rect x="3.5" y="4" width="13" height="12" rx="2" />
                    <path d="M8.5 4v12" />
                    <path d="M13 7l-3 3 3 3" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                )}
              </button>

              <div className="drag-region min-w-0" data-tauri-drag-region>
                <p className="drag-region text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                  {toolbar.section} · {networkLabel(pool.network)}
                </p>
                <div className="drag-region mt-0.5 flex items-center gap-2">
                  <h1 className="drag-region truncate text-[14px] font-semibold text-slate-900">{toolbar.title}</h1>
                  <span className={`drag-region inline-flex min-h-6 items-center rounded-full border px-2 text-[11px] font-semibold ${statusToneClass(toolbar.statusTone)}`}>
                    {toolbar.status}
                  </span>
                </div>
              </div>

              <div className="drag-region flex-1" data-tauri-drag-region />
            </div>
          </header>

          <main className="min-h-0 flex-1 overflow-auto p-6 md:p-7">
            <Outlet />
          </main>
        </section>
      </div>
    </div>
  );
}
