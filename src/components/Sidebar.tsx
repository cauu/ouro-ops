import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";

const linkBase = "flex items-center gap-2 rounded-md border px-3 py-2 text-sm font-medium transition-colors";

function navClass(isActive: boolean, collapsed: boolean): string {
  if (isActive) {
    return `${linkBase} border-blue-200 bg-blue-50 text-blue-700 ${collapsed ? "justify-center px-2" : ""}`;
  }
  return `${linkBase} border-transparent text-slate-600 hover:bg-slate-100 hover:text-slate-900 ${collapsed ? "justify-center px-2" : ""}`;
}

interface SidebarProps {
  network: "mainnet" | "preprod" | "preview";
  collapsed: boolean;
  onToggleCollapse: () => void;
}

function workspaceLabel(network: SidebarProps["network"]): string {
  if (network === "mainnet") {
    return "Mainnet Workspace";
  }
  if (network === "preprod") {
    return "Preprod Workspace";
  }
  return "Preview Workspace";
}

function IconButton({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      className="no-drag inline-flex h-7 w-7 items-center justify-center rounded-md border border-slate-300 bg-white text-slate-600 transition hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300"
    >
      {children}
    </button>
  );
}

export default function Sidebar({ network, collapsed, onToggleCollapse }: SidebarProps) {
  return (
    <aside
      className={`flex h-full flex-col border-r border-slate-200/80 bg-slate-50/85 backdrop-blur-md transition-[width] duration-200 ${
        collapsed ? "w-[88px]" : "w-64"
      }`}
    >
      <header className="drag-region border-b border-slate-200/80" data-tauri-drag-region>
        <div className={`flex h-14 items-center pl-[74px] ${collapsed ? "pr-2" : "pr-3"}`}>
          <div className="no-drag flex items-center gap-2">
            {!collapsed && (
              <IconButton title="新建工作区">
                <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                  <path d="M10 4v12M4 10h12" strokeLinecap="round" />
                </svg>
              </IconButton>
            )}
            <IconButton title={collapsed ? "展开侧边栏" : "收起侧边栏"} onClick={onToggleCollapse}>
              {collapsed ? (
                <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                  <path d="M6 4l8 6-8 6" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              ) : (
                <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
                  <path d="M14 4l-8 6 8 6" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              )}
            </IconButton>
          </div>
          <div className="flex-1" />
        </div>
      </header>

      <div className={`px-3 pb-3 pt-4 ${collapsed ? "text-center" : ""}`}>
        <div className={`mb-5 flex items-center ${collapsed ? "justify-center" : "gap-3"}`}>
          <span className="inline-block h-8 w-8 rounded-full bg-gradient-to-br from-blue-500 to-indigo-500 shadow-sm" aria-hidden="true" />
          {!collapsed && (
            <div>
              <p className="text-xs uppercase tracking-widest text-slate-500">OURO OPS</p>
              <p className="mt-1 text-xs text-slate-600">{workspaceLabel(network)}</p>
            </div>
          )}
        </div>

        <div className="space-y-5">
          <section>
            {!collapsed && <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">Workspace</p>}
            <NavLink to="/" end className={({ isActive }) => navClass(isActive, collapsed)} title="⌂ Dashboard">
              <span aria-hidden="true" className="text-[15px] leading-none">⌂</span>
              {!collapsed && <span>⌂ Dashboard</span>}
            </NavLink>
          </section>

          <section>
            {!collapsed && <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">Operations</p>}
            <nav className="space-y-2">
              <NavLink to="/kes" className={({ isActive }) => navClass(isActive, collapsed)} title="↻ KES Rotate">
                <span aria-hidden="true" className="text-[15px] leading-none">↻</span>
                {!collapsed && <span>↻ KES Rotate</span>}
              </NavLink>
              <NavLink to="/upgrade" className={({ isActive }) => navClass(isActive, collapsed)} title="⬆ Upgrade">
                <span aria-hidden="true" className="text-[15px] leading-none">⬆</span>
                {!collapsed && <span>⬆ Upgrade</span>}
              </NavLink>
            </nav>
          </section>
        </div>

        <div className={`mt-6 rounded-md border border-slate-200 bg-white/80 px-3 py-2 text-xs text-slate-600 ${collapsed ? "hidden" : ""}`}>
          日常操作入口统一：
          <br />Dashboard / KES Rotate / Upgrade
        </div>
        {collapsed && (
          <div className="mt-6 text-[11px] text-slate-500" title="日常操作入口统一：Dashboard / KES Rotate / Upgrade">
            OPS
          </div>
        )}
      </div>
    </aside>
  );
}
