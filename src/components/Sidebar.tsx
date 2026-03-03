import { NavLink } from "react-router-dom";

const linkBase =
  "block rounded-md px-3 py-2 text-sm font-medium transition-colors";

function navClass(isActive: boolean): string {
  if (isActive) {
    return `${linkBase} bg-zinc-100 text-zinc-900`;
  }
  return `${linkBase} text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100`;
}

interface SidebarProps {
  ticker: string;
}

export default function Sidebar({ ticker }: SidebarProps) {
  return (
    <aside className="w-full max-w-64 border-r border-zinc-800 bg-zinc-900 p-4">
      <div className="mb-6">
        <p className="text-xs uppercase tracking-widest text-zinc-500">Ouro Ops</p>
        <p className="mt-2 text-lg font-semibold text-zinc-100">{ticker}</p>
      </div>
      <nav className="space-y-2">
        <NavLink to="/" end className={({ isActive }) => navClass(isActive)}>
          Dashboard
        </NavLink>
        <NavLink to="/machines" className={({ isActive }) => navClass(isActive)}>
          Machines
        </NavLink>
        <NavLink to="/deploy" className={({ isActive }) => navClass(isActive)}>
          Deploy
        </NavLink>
        <NavLink to="/settings" className={({ isActive }) => navClass(isActive)}>
          Settings
        </NavLink>
      </nav>
    </aside>
  );
}
