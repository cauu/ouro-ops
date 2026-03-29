import { useMemo } from "react";
import type { StakingEpochEntry } from "../lib/types";

interface StakingTrendChartProps {
  data: StakingEpochEntry[];
}

const CHART_W = 600;
const CHART_H = 160;
const PAD_L = 48;
const PAD_R = 56;
const PAD_T = 12;
const PAD_B = 28;
const PLOT_W = CHART_W - PAD_L - PAD_R;
const PLOT_H = CHART_H - PAD_T - PAD_B;

function niceMax(value: number): number {
  if (value <= 0) return 1;
  const mag = 10 ** Math.floor(Math.log10(value));
  return Math.ceil(value / mag) * mag;
}

function formatCompact(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(0)}K`;
  return value.toFixed(0);
}

export default function StakingTrendChart({ data }: StakingTrendChartProps) {
  const { delegatorPath, stakePath, xLabels, maxDelegators, maxStakeAda } = useMemo(() => {
    if (data.length < 2) {
      return { delegatorPath: "", stakePath: "", xLabels: [] as { x: number; label: string }[], maxDelegators: 0, maxStakeAda: 0 };
    }

    const maxD = niceMax(Math.max(...data.map((d) => d.delegator_cnt)));
    const maxS = niceMax(Math.max(...data.map((d) => d.active_stake_ada)));

    const toX = (i: number) => PAD_L + (i / (data.length - 1)) * PLOT_W;
    const toDY = (v: number) => PAD_T + PLOT_H - (v / maxD) * PLOT_H;
    const toSY = (v: number) => PAD_T + PLOT_H - (v / maxS) * PLOT_H;

    const dPoints = data.map((d, i) => `${toX(i).toFixed(1)},${toDY(d.delegator_cnt).toFixed(1)}`);
    const sPoints = data.map((d, i) => `${toX(i).toFixed(1)},${toSY(d.active_stake_ada).toFixed(1)}`);

    const step = Math.max(1, Math.floor(data.length / 6));
    const labels: { x: number; label: string }[] = [];
    for (let i = 0; i < data.length; i += step) {
      labels.push({ x: toX(i), label: `E${data[i].epoch_no}` });
    }
    if (data.length - 1 > (labels.length - 1) * step) {
      labels.push({ x: toX(data.length - 1), label: `E${data[data.length - 1].epoch_no}` });
    }

    return {
      delegatorPath: `M${dPoints.join("L")}`,
      stakePath: `M${sPoints.join("L")}`,
      xLabels: labels,
      maxDelegators: maxD,
      maxStakeAda: maxS,
    };
  }, [data]);

  if (data.length < 2) {
    return (
      <div className="flex h-[160px] items-center justify-center text-xs text-slate-400">
        趋势数据不足（需至少 2 个 epoch）
      </div>
    );
  }

  return (
    <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} className="w-full" aria-label="Staking trend chart">
      {/* grid lines */}
      {[0, 0.25, 0.5, 0.75, 1].map((frac) => (
        <line
          key={frac}
          x1={PAD_L}
          x2={CHART_W - PAD_R}
          y1={PAD_T + PLOT_H * (1 - frac)}
          y2={PAD_T + PLOT_H * (1 - frac)}
          stroke="#e2e8f0"
          strokeWidth="0.5"
        />
      ))}

      {/* delegator line (left axis, blue) */}
      <path d={delegatorPath} fill="none" stroke="#3b82f6" strokeWidth="1.8" strokeLinejoin="round" />

      {/* stake line (right axis, emerald) */}
      <path d={stakePath} fill="none" stroke="#10b981" strokeWidth="1.8" strokeLinejoin="round" strokeDasharray="4 2" />

      {/* left axis labels (delegators) */}
      <text x={PAD_L - 4} y={PAD_T + 4} textAnchor="end" className="fill-blue-500 text-[9px]">
        {formatCompact(maxDelegators)}
      </text>
      <text x={PAD_L - 4} y={PAD_T + PLOT_H} textAnchor="end" className="fill-blue-500 text-[9px]">
        0
      </text>

      {/* right axis labels (stake ADA) */}
      <text x={CHART_W - PAD_R + 4} y={PAD_T + 4} textAnchor="start" className="fill-emerald-500 text-[9px]">
        {formatCompact(maxStakeAda)}
      </text>
      <text x={CHART_W - PAD_R + 4} y={PAD_T + PLOT_H} textAnchor="start" className="fill-emerald-500 text-[9px]">
        0
      </text>

      {/* x axis labels */}
      {xLabels.map((label) => (
        <text key={label.label} x={label.x} y={CHART_H - 4} textAnchor="middle" className="fill-slate-400 text-[9px]">
          {label.label}
        </text>
      ))}

      {/* legend */}
      <line x1={PAD_L} y1={CHART_H - 16} x2={PAD_L + 14} y2={CHART_H - 16} stroke="#3b82f6" strokeWidth="1.5" />
      <text x={PAD_L + 18} y={CHART_H - 13} className="fill-slate-500 text-[8px]">Delegators</text>
      <line x1={PAD_L + 80} y1={CHART_H - 16} x2={PAD_L + 94} y2={CHART_H - 16} stroke="#10b981" strokeWidth="1.5" strokeDasharray="4 2" />
      <text x={PAD_L + 98} y={CHART_H - 13} className="fill-slate-500 text-[8px]">Stake (ADA)</text>
    </svg>
  );
}
