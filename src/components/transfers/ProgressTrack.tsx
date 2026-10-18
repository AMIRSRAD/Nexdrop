import React from 'react';
import type { DownloadSegment, DownloadStatus } from '../../types/download';
import { cn } from '../../lib/cn';

interface ProgressTrackProps {
  progress: number; // 0..1
  status: DownloadStatus;
  segments?: DownloadSegment[];
  totalBytes: number;
  height?: number;
  className?: string;
}

const fillFor = (status: DownloadStatus) =>
  status === 'downloading'
    ? 'var(--accent)'
    : status === 'completed'
      ? 'var(--success)'
      : status === 'failed'
        ? 'var(--danger)'
        : status === 'paused'
          ? 'var(--warning)'
          : 'var(--fg-3)';

/**
 * A byte map of the file. With multiple connections each segment is drawn
 * at its real offset, so you see exactly which parts of the file have
 * arrived — the same picture IDM shows, just quieter.
 */
export const ProgressTrack: React.FC<ProgressTrackProps> = React.memo(
  ({ progress, status, segments, totalBytes, height = 4, className }) => {
    const fill = fillFor(status);
    const active = status === 'downloading';
    const useMap = segments && segments.length > 1 && totalBytes > 0 && status !== 'completed';

    return (
      <div
        className={cn('relative w-full overflow-hidden rounded-full bg-track', className)}
        style={{ height }}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(progress * 100)}
      >
        {useMap ? (
          segments!.map((seg) => {
            const left = (seg.start_byte / totalBytes) * 100;
            const width = (Math.max(0, seg.downloaded_bytes) / totalBytes) * 100;
            const segActive = active && seg.status === 'downloading';
            return (
              <div
                key={seg.segment_index}
                className={cn('absolute inset-y-0 overflow-hidden transition-[width] duration-300 ease-linear', segActive && 'nx-sheen')}
                style={{ left: `${left}%`, width: `${Math.min(width, 100 - left)}%`, background: fill }}
              />
            );
          })
        ) : (
          <div
            className={cn('absolute inset-y-0 left-0 overflow-hidden rounded-full transition-[width] duration-300 ease-linear', active && 'nx-sheen')}
            style={{ width: `${progress * 100}%`, background: fill }}
          />
        )}
      </div>
    );
  },
);
ProgressTrack.displayName = 'ProgressTrack';
