import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toUserError } from "../lib/errors";
import { poolInit } from "../lib/ipc";
import type { Pool, PoolInitPayload } from "../lib/types";

interface SetupWizardProps {
  onCreated: (pool: Pool) => void;
}

const DEFAULT_PAYLOAD: PoolInitPayload = {
  ticker: "OURO",
  network: "mainnet",
  margin: 0.02,
  fixed_cost: 340000000,
};

export default function SetupWizard({ onCreated }: SetupWizardProps) {
  const navigate = useNavigate();
  const [submittingTarget, setSubmittingTarget] = useState<"/" | "/deploy" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const initializeWorkspace = async (target: "/" | "/deploy") => {
    setSubmittingTarget(target);
    setError(null);
    try {
      const pool = await poolInit(DEFAULT_PAYLOAD);
      onCreated(pool);
      navigate(target, { replace: true });
    } catch (e) {
      setError(toUserError(e));
    } finally {
      setSubmittingTarget(null);
    }
  };

  const isSubmitting = submittingTarget != null;

  return (
    <div className="welcome-shell min-h-screen bg-slate-100 p-6 text-slate-900">
      <div className="welcome-window mx-auto max-w-5xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-sm">
        <header className="titlebar flex items-center justify-between border-b border-slate-200 bg-slate-50 px-4 py-3">
          <div className="flex items-center gap-3">
            <div className="traffic-lights flex items-center gap-1.5" aria-hidden="true">
              <span className="h-2.5 w-2.5 rounded-full bg-red-400" />
              <span className="h-2.5 w-2.5 rounded-full bg-amber-400" />
              <span className="h-2.5 w-2.5 rounded-full bg-emerald-400" />
            </div>
            <h1 className="text-sm font-semibold">Welcome · Ouro Ops</h1>
          </div>
          <span className="rounded-full border border-amber-300 bg-amber-50 px-2.5 py-1 text-xs font-medium text-amber-700">
            Not Deployed
          </span>
        </header>

        <main className="flex items-center justify-center px-6 py-12">
          <section className="welcome-panel w-full max-w-xl rounded-xl border border-slate-200 bg-white px-8 py-10 text-center">
            <div
              aria-hidden="true"
              className="hero-mark mx-auto mb-4 h-12 w-12 rounded-full bg-gradient-to-br from-blue-500 to-indigo-500"
            />
            <h2 className="text-2xl font-semibold tracking-tight">欢迎使用 Ouro Ops</h2>
            <p className="mt-3 text-sm text-slate-600">
              未检测到部署环境。点击「开始部署」进入沉浸式向导。
            </p>

            <div className="mt-6 flex flex-wrap items-center justify-center gap-3">
              <button
                type="button"
                onClick={() => void initializeWorkspace("/deploy")}
                disabled={isSubmitting}
                className="rounded-md bg-blue-600 px-4 py-2 text-sm font-semibold text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-70"
              >
                {submittingTarget === "/deploy" ? "准备部署中..." : "开始部署"}
              </button>
              <button
                type="button"
                disabled
                className="rounded-md border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-500 disabled:cursor-not-allowed"
                title="导入能力将在后续阶段开放。"
              >
                导入已有配置
              </button>
            </div>

            <p className="mt-6 text-sm text-slate-600">
              已经部署完成？
              <button
                type="button"
                onClick={() => void initializeWorkspace("/")}
                disabled={isSubmitting}
                className="ml-1 text-blue-700 underline-offset-2 hover:underline disabled:opacity-60"
              >
                直接进入 Dashboard
              </button>
            </p>

            <p className="mt-4 text-xs text-slate-500">
              首次进入会使用默认工作区参数初始化：Ticker = OURO，Network = mainnet。
            </p>

            {error && (
              <p className="mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-left text-sm text-red-700">
                {error}
              </p>
            )}
          </section>
        </main>
      </div>
    </div>
  );
}
