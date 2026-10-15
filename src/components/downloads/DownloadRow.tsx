import React from 'react';
import {
  Play,
  Pause,
  RotateCw,
  FolderOpen,
  ExternalLink,
  Trash2,
} from 'lucide-react';
import { DownloadTask } from '../../types/download';
import { formatBytes, formatDuration, formatSpeed } from '../../utils/format';
import { FileIcon } from './FileIcon';
import { StatusBadge } from './StatusBadge';

interface DownloadRowProps {
  task: DownloadTask;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onRetry: (id: string) => void;
  onDelete: (id: string, deleteFile?: boolean) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
  onSelect?: (task: DownloadTask) => void;
}

export const DownloadRow: React.FC<DownloadRowProps> = ({
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
    <div
      onClick={() => onSelect?.(task)}
      className="group flex items-center gap-3 px-3.5 py-2.5 rounded-xl hover:bg-slate-800/40 border border-transparent hover:border-white/5 transition-all duration-150 cursor-pointer"
    >
      <FileIcon category={task.category} filename={task.filename} size={18} className="p-1.5 rounded-lg shrink-0" />

      {/* Name and size */}
      <div className="min-w-0 flex-1">
        <h4
          className="text-xs font-semibold text-slate-200 truncate cursor-pointer hover:text-indigo-300"
          title={task.filename}
          onClick={() => (isCompleted ? onOpen(task.id) : null)}
        >
          {task.filename}
        </h4>
        <div className="flex items-center gap-2 text-[11px] text-slate-400 mt-0.5">
          <span>{formatBytes(task.downloaded_bytes)}</span>
          <span>/</span>
          <span>{task.total_bytes > 0 ? formatBytes(task.total_bytes) : 'Unknown'}</span>
          {task.connections > 1 && (
            <>
              <span>•</span>
              <span className="text-indigo-400">{task.connections} streams</span>
            </>
          )}
        </div>
      </div>

      {/* Mini Progress */}
      <div className="w-32 shrink-0">
        <div className="flex items-center justify-between text-[10px] text-slate-400 mb-1">
          <span>{percent.toFixed(0)}%</span>
          {isDownloading && task.speed_bps > 0 && (
            <span className="text-indigo-300 font-mono">{formatSpeed(task.speed_bps)}</span>
          )}
        </div>
        <div className="h-1.5 w-full rounded-full bg-slate-800 overflow-hidden">
          <div
            className={`h-full rounded-full transition-all duration-300 ${
              isCompleted ? 'bg-emerald-500' : isDownloading ? 'bg-indigo-500' : 'bg-slate-600'
            }`}
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>

      {/* Status */}
      <div className="shrink-0 w-28 flex justify-center">
        <StatusBadge status={task.status} />
      </div>

      {/* Speed & ETA */}
      <div className="w-24 shrink-0 text-right text-[11px] text-slate-400 font-mono">
        {isDownloading ? (
          <span>{formatDuration(task.eta_seconds)}</span>
        ) : (
          <span>{isCompleted ? 'Done' : '--'}</span>
        )}
      </div>

      {/* Actions */}
      <div className="flex items-center gap-1 shrink-0 opacity-80 group-hover:opacity-100 transition-opacity">
        {isDownloading && (
          <button
            onClick={() => onPause(task.id)}
            className="p-1 rounded-lg text-slate-300 hover:text-white hover:bg-slate-700/60 transition-colors"
            title="Pause"
          >
            <Pause size={14} />
          </button>
        )}

        {isPaused && (
          <button
            onClick={() => onResume(task.id)}
            className="p-1 rounded-lg text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/20 transition-colors"
            title="Resume"
          >
            <Play size={14} />
          </button>
        )}

        {isFailed && (
          <button
            onClick={() => onRetry(task.id)}
            className="p-1 rounded-lg text-amber-400 hover:text-amber-300 hover:bg-amber-500/20 transition-colors"
            title="Retry"
          >
            <RotateCw size={14} />
          </button>
        )}

        {isCompleted && (
          <>
            <button
              onClick={() => onOpen(task.id)}
              className="p-1 rounded-lg text-indigo-400 hover:text-indigo-300 hover:bg-indigo-500/20 transition-colors"
              title="Open"
            >
              <ExternalLink size={14} />
            </button>
            <button
              onClick={() => onReveal(task.id)}
              className="p-1 rounded-lg text-slate-300 hover:text-white hover:bg-slate-700/60 transition-colors"
              title="Folder"
            >
              <FolderOpen size={14} />
            </button>
          </>
        )}

        <button
          onClick={() => onDelete(task.id, true)}
          className="p-1 rounded-lg text-slate-500 hover:text-rose-400 hover:bg-rose-500/10 transition-colors"
          title="Delete"
        >
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
};
