import { Outlet } from "react-router-dom";
import type { Pool } from "../lib/types";
import Sidebar from "./Sidebar";

interface LayoutProps {
  pool: Pool;
}

export default function Layout({ pool }: LayoutProps) {
  return (
    <div className="min-h-screen bg-slate-100 text-slate-900">
      <div className="mx-auto flex min-h-screen max-w-7xl flex-col md:flex-row">
        <Sidebar network={pool.network} />
        <main className="flex-1 p-6 md:p-8">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
