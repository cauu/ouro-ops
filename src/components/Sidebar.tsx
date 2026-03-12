import { NavLink } from "react-router-dom";

const linkBase =
  "block rounded-md px-3 py-2 text-sm font-medium transition-colors";

function navClass(isActive: boolean): string {
  if (isActive) {
    return `${linkBase} border border-blue-200 bg-blue-50 text-blue-700`;
  }
  return `${linkBase} border border-transparent text-slate-600 hover:bg-slate-100 hover:text-slate-900`;
}

interface SidebarProps {
  ticker: string;
}

export default function Sidebar({ ticker }: SidebarProps) {
  return (
    <aside className="w-full max-w-64 border-r border-slate-200 bg-white p-4">
      <div className="mb-6">
        <p className="text-xs uppercase tracking-widest text-slate-500">Ouro Ops</p>
        <p className="mt-2 text-lg font-semibold text-slate-900">{ticker}</p>
      </div>
      <nav className="space-y-2">
        <NavLink to="/" end className={({ isActive }) => navClass(isActive)}>
          Dashboard
        </NavLink>
        <NavLink to="/deploy" className={({ isActive }) => navClass(isActive)}>
          Deploy
        </NavLink>
        <NavLink to="/kes" className={({ isActive }) => navClass(isActive)}>
          KES
        </NavLink>
        <NavLink to="/upgrade" className={({ isActive }) => navClass(isActive)}>
          Upgrade
        </NavLink>
        <NavLink to="/settings" className={({ isActive }) => navClass(isActive)}>
          Settings
        </NavLink>
      </nav>
    </aside>
  );
}
