import React, { useMemo } from 'react';
import type { DownloadTask } from '../../types/download';
import { CATEGORIES, categoryMeta } from '../../lib/meta';
import { formatBytes, formatSpeed } from '../../utils/format';
import { Dialog } from '../ui/Dialog';

export const StatsDialog: React.FC<{
  open: boolean;
  onClose: () => void;
  downloads: DownloadTask[];
  history: number[];
  peak: number;
  speed: number;
}> = ({ open, onClose, downloads, history, peak, speed }) => {
  const stats = useMemo(() => {
    const completed = downloads.filter((d) => d.status === 'completed');
    const failed = downloads.filter((d) => d.status === 'failed');
    const transferred = downloads.reduce((a, d) => a + (d.downloaded_bytes || 0), 0);
    const finishedOrFailed = completed.length + failed.length;
    const byCat = CATEGORIES.map((c) => {
      const items = downloads.filter((d) => categoryMeta(d.category).id === c.id);
      return { ...c, count: items.length, bytes: items.reduce((a, d) => a + (d.downloaded_bytes || 0), 0) };
    }).filter((c) => c.count > 0);
    const avgConn = completed.length ? completed.reduce((a, d) => a + (d.connections || 1), 0) / completed.length : 0;
    return {
      transferred,
      completed: completed.length,
      successRate: finishedOrFailed ? Math.round((completed.length / finishedOrFailed) * 100) : null,
      avgConn,
      byCat,
    };
  }, [downloads]);

  return (
    <Dialog open={open} onClose={onClose} title="Statistics" description="Across everything in your download list" width={600}>
      <div className="space-y-5">
        <div className="grid grid-cols-4 gap-px overflow-hidden rounded-lg bg-line shadow-[0_0_0_1px_var(--line)]">
          <Kpi label="Transferred" value={formatBytes(stats.transferred)} />
          <Kpi label="Completed" value={String(stats.completed)} sub={`of ${downloads.length}`} />
          <Kpi label="Success rate" value={stats.successRate === null ? '—' : `${stats.successRate}%`} />
          <Kpi label="Avg. connections" value={stats.avgConn ? stats.avgConn.toFixed(1) : '—'} />
        </div>

        <section>
          <div className="mb-2 flex items-baseline justify-between">
            <h3 className="text-[12.5px] font-medium text-fg-2">Throughput · last 90 seconds</h3>
            <span className="text-[12px] text-fg-3 tabular">
              Now <span className="font-medium text-fg">{formatSpeed(speed)}</span> · Peak {formatSpeed(peak)}
            </span>
          </div>
          <SpeedChart values={history} />
        </section>

        <section>
          <h3 className="mb-2 text-[12.5px] font-medium text-fg-2">By type</h3>
          {stats.byCat.length === 0 ? (
            <p className="text-[12.5px] text-fg-3">No downloads yet.</p>
          ) : (
            <>
              <div className="mb-3 flex h-2 gap-[2px] overflow-hidden rounded-full">
                {stats.byCat.map((c) => (
                  <div key={c.id} style={{ flexGrow: Math.max(c.bytes, 1), background: c.color }} title={c.label} />
                ))}
              </div>
              <div className="grid grid-cols-2 gap-x-6 gap-y-1.5">
                {stats.byCat.map((c) => (
                  <div key={c.id} className="flex items-center gap-2 text-[12.5px]">
                    <span className="h-2 w-2 rounded-full" style={{ background: c.color }} />
                    <span className="text-fg">{c.label}</span>
                    <span className="text-fg-3 tabular">{c.count}</span>
                    <span className="ml-auto text-fg-2 tabular">{formatBytes(c.bytes)}</span>
                  </div>
                ))}
              </div>
            </>
          )}
        </section>
      </div>
    </Dialog>
  );
};

const Kpi: React.FC<{ label: string; value: string; sub?: string }> = ({ label, value, sub }) => (
  <div className="bg-surface-2 px-3.5 py-3">
    <div className="text-[11.5px] text-fg-3">{label}</div>
    <div className="mt-1 text-[19px] leading-6 font-semibold tracking-[-0.01em] text-fg tabular">
      {value}
      {sub && <span className="ml-1 text-[12px] font-normal text-fg-3">{sub}</span>}
    </div>
  </div>
);

const SpeedChart: React.FC<{ values: number[] }> = ({ values }) => {
  const w = 560;
  const h = 96;
  const max = Math.max(...values, 1);
  const pts = values.map((v, i) => [(i / (values.length - 1)) * w, h - 2 - (v / max) * (h - 10)] as const);
  const line = pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  return (
    <div className="rounded-lg bg-surface-2 p-2 shadow-[inset_0_0_0_1px_var(--line)]">
      <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className="block h-24 w-full" aria-label="Download speed over time">
        <defs>
          <linearGradient id="nx-speed-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1="0" x2={w} y1={h * f} y2={h * f} stroke="var(--line)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
        ))}
        <polygon points={`0,${h} ${line} ${w},${h}`} fill="url(#nx-speed-fill)" />
        <polyline points={line} fill="none" stroke="var(--accent)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
      </svg>
    </div>
  );
};
