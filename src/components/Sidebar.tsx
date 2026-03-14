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

export default function Sidebar({ network, collapsed }: SidebarProps) {
  return (
    <aside
      className={`flex h-full flex-col border-r border-slate-200/80 bg-slate-50/85 backdrop-blur-md transition-[width] duration-200 ${collapsed ? "w-[88px]" : "w-64"
        }`}
    >
      <header className="drag-region border-slate-200/80" data-tauri-drag-region>
        <div className={`drag-region flex h-14 items-center pl-[74px] ${collapsed ? "pr-2" : "pr-3"}`} data-tauri-drag-region>
          <div className="drag-region flex-1" data-tauri-drag-region />
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
            <NavLink to="/" end className={({ isActive }) => navClass(isActive, collapsed)} title="Dashboard">
              <svg
                viewBox="0 0 20 20"
                className="h-[17px] w-[17px] shrink-0"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                aria-hidden="true"
              >
                <path d="M3.5 8.8L10 3.8l6.5 5" strokeLinecap="round" strokeLinejoin="round" />
                <path d="M5.5 8.7V16h9V8.7" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              {!collapsed && <span>Dashboard</span>}
            </NavLink>
          </section>

          <section>
            {!collapsed && <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">Operations</p>}
            <nav className="space-y-2">
              <NavLink to="/kes" className={({ isActive }) => navClass(isActive, collapsed)} title="KES Rotate">
                <svg
                  viewBox="0 0 20 20"
                  className="h-[17px] w-[17px] shrink-0"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  aria-hidden="true"
                >
                  <path d="M15.5 7.2A6 6 0 005 8.5" strokeLinecap="round" />
                  <path d="M5.1 4.8v3.8h3.8" strokeLinecap="round" strokeLinejoin="round" />
                  <path d="M4.5 12.8A6 6 0 0015 11.5" strokeLinecap="round" />
                  <path d="M14.9 15.2v-3.8h-3.8" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
                {!collapsed && <span>KES Rotate</span>}
              </NavLink>
              <NavLink to="/telemetry" className={({ isActive }) => navClass(isActive, collapsed)} title="Telemetry API">
                <svg
                  viewBox="0 0 20 20"
                  className="h-[17px] w-[17px] shrink-0"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  aria-hidden="true"
                >
                  <circle cx="10" cy="10" r="6.5" />
                  <path d="M10 3.5v3.2M10 13.3v3.2M3.5 10h3.2M13.3 10h3.2" strokeLinecap="round" />
                </svg>
                {!collapsed && <span>Telemetry API</span>}
              </NavLink>
              <NavLink to="/upgrade" className={({ isActive }) => navClass(isActive, collapsed)} title="Upgrade">
                <svg
                  viewBox="0 0 20 20"
                  className="h-[17px] w-[17px] shrink-0"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  aria-hidden="true"
                >
                  <path d="M10 14.8V4.6" strokeLinecap="round" />
                  <path d="M6.7 7.8L10 4.5l3.3 3.3" strokeLinecap="round" strokeLinejoin="round" />
                  <path d="M4.8 16h10.4" strokeLinecap="round" />
                </svg>
                {!collapsed && <span>Upgrade</span>}
              </NavLink>
            </nav>
          </section>
        </div>

        <div className={`mt-6 rounded-md border border-slate-200 bg-white/80 px-3 py-2 text-xs text-slate-600 ${collapsed ? "hidden" : ""}`}>
          日常操作入口统一：
          <br />Dashboard / KES Rotate / Telemetry API / Upgrade
        </div>
        {collapsed && (
          <div className="mt-6 text-[11px] text-slate-500" title="日常操作入口统一：Dashboard / KES Rotate / Telemetry API / Upgrade">
            OPS
          </div>
        )}
      </div>
    </aside>
  );
}
