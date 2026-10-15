import React from 'react';
import { DownloadSegment } from '../../types/download';
import { formatBytes } from '../../utils/format';

interface SegmentBarProps {
  segments?: DownloadSegment[];
  progressPercent: number;
  isDownloading: boolean;
  connections: number;
}

export const SegmentBar: React.FC<SegmentBarProps> = ({
  segments = [],
  progressPercent,
  isDownloading,
  connections,
}) => {
  // If no multi-part segments are provided or it's single stream
  if (!segments || segments.length <= 1) {
    return (
      <div className="w-full">
        <div className="relative h-2 w-full overflow-hidden rounded-full bg-slate-800/80">
          <div
            className={`h-full rounded-full transition-all duration-300 ${
              isDownloading
                ? 'bg-gradient-to-r from-indigo-500 via-purple-500 to-pink-500'
                : progressPercent >= 100
                ? 'bg-emerald-500'
                : 'bg-indigo-400'
            }`}
            style={{ width: `${Math.min(100, Math.max(0, progressPercent))}%` }}
          />
          {isDownloading && (
            <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/20 to-transparent animate-shimmer" />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="w-full space-y-1.5">
      {/* Visual multi-connection segment blocks */}
      <div className="flex h-2.5 w-full gap-1 overflow-hidden rounded-md bg-slate-900/90 p-0.5 border border-white/5">
        {segments.map((seg) => {
          const segPercent =
            seg.total_bytes > 0
              ? Math.min(100, Math.round((seg.downloaded_bytes / seg.total_bytes) * 100))
              : 0;
          const isSegActive = seg.status === 'downloading';
          const isSegComplete = segPercent >= 100;

          return (
            <div
              key={seg.segment_index}
              className="group relative flex-1 h-full overflow-hidden rounded-sm bg-slate-800/60"
              title={`Stream #${seg.segment_index + 1}: ${formatBytes(seg.downloaded_bytes)} / ${formatBytes(
                seg.total_bytes
              )} (${segPercent}%)`}
            >
              <div
                className={`h-full transition-all duration-200 ${
                  isSegComplete
                    ? 'bg-emerald-500/80'
                    : isSegActive
                    ? 'bg-indigo-500'
                    : 'bg-indigo-700/50'
                }`}
                style={{ width: `${segPercent}%` }}
              />
              {isSegActive && (
                <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/30 to-transparent animate-shimmer" />
              )}
            </div>
          );
        })}
      </div>

      {/* Stream indicators legend */}
      <div className="flex items-center justify-between text-[11px] text-slate-400 px-0.5">
        <span className="flex items-center gap-1.5">
          <span className="flex h-1.5 w-1.5 rounded-full bg-indigo-500 animate-pulse" />
          {connections} concurrent streams active
        </span>
        <span className="font-mono text-slate-300 font-medium">
          {progressPercent.toFixed(1)}%
        </span>
      </div>
    </div>
  );
};
