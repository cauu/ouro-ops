import { NavLink } from "react-router-dom";

const linkBase =
  "block rounded-md border px-3 py-2 text-sm font-medium transition-colors";

function navClass(isActive: boolean): string {
  if (isActive) {
    return `${linkBase} border-blue-200 bg-blue-50 text-blue-700`;
  }
  return `${linkBase} border-transparent text-slate-600 hover:bg-slate-100 hover:text-slate-900`;
}

interface SidebarProps {
  network: "mainnet" | "preprod" | "preview";
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

export default function Sidebar({ network }: SidebarProps) {
  return (
    <aside className="flex w-full max-w-64 flex-col border-r border-slate-200 bg-white p-4">
      <div className="mb-6 flex items-center gap-3">
        <span className="inline-block h-8 w-8 rounded-full bg-gradient-to-br from-blue-500 to-indigo-500" aria-hidden="true" />
        <div>
          <p className="text-xs uppercase tracking-widest text-slate-500">OURO OPS</p>
          <p className="mt-1 text-xs text-slate-600">{workspaceLabel(network)}</p>
        </div>
      </div>

      <div className="space-y-5">
        <section>
          <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">Workspace</p>
          <NavLink to="/" end className={({ isActive }) => navClass(isActive)}>
            ⌂ Dashboard
          </NavLink>
        </section>

        <section>
          <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">Operations</p>
          <nav className="space-y-2">
            <NavLink to="/kes" className={({ isActive }) => navClass(isActive)}>
              ↻ KES Rotate
            </NavLink>
            <NavLink to="/upgrade" className={({ isActive }) => navClass(isActive)}>
              ⬆ Upgrade
            </NavLink>
          </nav>
        </section>
      </div>

      <div className="mt-auto rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-600">
        日常操作入口统一：
        <br />Dashboard / KES Rotate / Upgrade
      </div>
    </aside>
  );
}
