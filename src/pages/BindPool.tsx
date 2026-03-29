import { useNavigate } from "react-router-dom";
import type { Pool } from "../lib/types";
import PoolRegistrationStatus from "./PoolRegistrationStatus";

interface BindPoolProps {
  poolTicker: string;
  onBound: (pool: Pool) => void;
}

export default function BindPool({ poolTicker, onBound }: BindPoolProps) {
  const navigate = useNavigate();

  return (
    <section className="mx-auto max-w-3xl space-y-5">
      <header>
        <h1 className="text-sm font-semibold text-slate-900">绑定链上矿池</h1>
        <p className="mt-1 text-xs text-slate-500">
          选择查询节点，输入 Pool ID 或 cold.vkey 路径，验证链上注册状态后完成绑定。
        </p>
      </header>

      <PoolRegistrationStatus
        poolTicker={poolTicker}
        onBound={(pool) => {
          onBound(pool);
          navigate("/", { replace: true });
        }}
        embedded
      />
    </section>
  );
}
