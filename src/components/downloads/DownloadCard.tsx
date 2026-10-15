import React from 'react';
import { motion } from 'motion/react';
import {
  Play,
  Pause,
  RotateCw,
  FolderOpen,
  ExternalLink,
  Trash2,
  MoreVertical,
  Copy,
  Zap,
} from 'lucide-react';
import { DownloadTask } from '../../types/download';
import { formatBytes, formatDuration, formatSpeed } from '../../utils/format';
import { FileIcon } from './FileIcon';
import { SegmentBar } from './SegmentBar';
import { StatusBadge } from './StatusBadge';

interface DownloadCardProps {
  task: DownloadTask;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onRetry: (id: string) => void;
  onDelete: (id: string, deleteFile?: boolean) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
  onSelect?: (task: DownloadTask) => void;
}

export const DownloadCard: React.FC<DownloadCardProps> = ({
  task,
  onPause,
  onResume,
  onRetry,
  onDelete,
  onOpen,
  onReveal,
  onSelect,
}) => {
  const percent =
    task.total_bytes > 0
      ? Math.min(100, (task.downloaded_bytes / task.total_bytes) * 100)
      : 0;

  const isDownloading = task.status === 'downloading';
  const isCompleted = task.status === 'completed';
  const isPaused = task.status === 'paused';
  const isFailed = task.status === 'failed';

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.98 }}
      transition={{ duration: 0.2 }}
      onClick={() => onSelect?.(task)}
      className="group relative rounded-2xl bg-slate-900/40 p-4 border border-white/5 hover:border-white/10 hover:bg-slate-900/60 transition-all duration-200 shadow-lg shadow-black/20 cursor-pointer"
    >
      <div className="flex items-start justify-between gap-4">
        {/* File icon and title */}
        <div className="flex items-start gap-3.5 min-w-0 flex-1">
          <FileIcon category={task.category} filename={task.filename} size={22} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h3
                className="text-sm font-semibold text-slate-100 truncate hover:text-indigo-300 transition-colors cursor-pointer"
                title={task.filename}
                onClick={() => (isCompleted ? onOpen(task.id) : null)}
              >
                {task.filename}
              </h3>
              {task.resumable && task.connections > 1 && (
                <span className="flex items-center gap-0.5 text-[10px] font-medium px-1.5 py-0.5 rounded bg-indigo-500/10 text-indigo-400 border border-indigo-500/20" title="Accelerated multi-part download">
                  <Zap size={10} />
                  {task.connections}x
                </span>
              )}
            </div>
            <p className="text-xs text-slate-400 truncate mt-0.5 flex items-center gap-1.5">
              <span className="truncate max-w-[280px]" title={task.url}>
                {task.url}
              </span>
            </p>
          </div>
        </div>

        {/* Status badge and actions */}
        <div className="flex items-center gap-2 shrink-0">
          <StatusBadge status={task.status} />

          {/* Quick Action buttons */}
          <div className="flex items-center gap-1 bg-slate-800/50 p-1 rounded-xl border border-white/5">
            {isDownloading && (
              <button
                onClick={() => onPause(task.id)}
                className="p-1.5 rounded-lg text-slate-300 hover:text-white hover:bg-slate-700/60 transition-colors"
                title="Pause Download"
              >
                <Pause size={15} />
              </button>
            )}

            {isPaused && (
              <button
                onClick={() => onResume(task.id)}
                className="p-1.5 rounded-lg text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/20 transition-colors"
                title="Resume Download"
              >
                <Play size={15} />
              </button>
            )}

            {isFailed && (
              <button
                onClick={() => onRetry(task.id)}
                className="p-1.5 rounded-lg text-amber-400 hover:text-amber-300 hover:bg-amber-500/20 transition-colors"
                title="Retry Download"
              >
                <RotateCw size={15} />
              </button>
            )}

            {isCompleted && (
              <>
                <button
                  onClick={() => onOpen(task.id)}
                  className="p-1.5 rounded-lg text-indigo-400 hover:text-indigo-300 hover:bg-indigo-500/20 transition-colors"
                  title="Open File"
                >
                  <ExternalLink size={15} />
                </button>
                <button
                  onClick={() => onReveal(task.id)}
                  className="p-1.5 rounded-lg text-slate-300 hover:text-white hover:bg-slate-700/60 transition-colors"
                  title="Show in Folder"
                >
                  <FolderOpen size={15} />
                </button>
              </>
            )}

            <button
              onClick={() => onDelete(task.id, true)}
              className="p-1.5 rounded-lg text-slate-400 hover:text-rose-400 hover:bg-rose-500/10 transition-colors"
              title="Delete Download"
            >
              <Trash2 size={15} />
            </button>
          </div>
        </div>
      </div>

      {/* Progress & Speed section */}
      <div className="mt-3.5 space-y-2">
        <SegmentBar
          segments={task.segments}
          progressPercent={percent}
          isDownloading={isDownloading}
          connections={task.connections}
        />

        <div className="flex items-center justify-between text-xs text-slate-400 pt-0.5">
          <div className="flex items-center gap-2">
            <span className="font-medium text-slate-300">
              {formatBytes(task.downloaded_bytes)}
            </span>
            <span>of</span>
            <span>{task.total_bytes > 0 ? formatBytes(task.total_bytes) : 'Unknown'}</span>
          </div>

          <div className="flex items-center gap-3">
            {isDownloading && task.speed_bps > 0 && (
              <>
                <span className="text-indigo-400 font-mono font-medium">
                  {formatSpeed(task.speed_bps)}
                </span>
                <span className="text-slate-500">•</span>
                <span>ETA {formatDuration(task.eta_seconds)}</span>
              </>
            )}

            {isFailed && task.error_message && (
              <span className="text-rose-400 truncate max-w-[200px]" title={task.error_message}>
                {task.error_message}
              </span>
            )}
          </div>
        </div>
      </div>
    </motion.div>
  );
};
