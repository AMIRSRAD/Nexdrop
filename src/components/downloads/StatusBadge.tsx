import React from 'react';
import { DownloadStatus } from '../../types/download';
import { CheckCircle2, PauseCircle, AlertCircle, Clock, ArrowDownCircle, XCircle } from 'lucide-react';

interface StatusBadgeProps {
  status: DownloadStatus;
  className?: string;
}

export const StatusBadge: React.FC<StatusBadgeProps> = ({ status, className = '' }) => {
  switch (status) {
    case 'downloading':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-indigo-500/15 text-indigo-300 border border-indigo-500/30 ${className}`}
        >
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-indigo-400 opacity-75"></span>
            <span className="relative inline-flex rounded-full h-2 w-2 bg-indigo-500"></span>
          </span>
          Downloading
        </span>
      );
    case 'completed':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-emerald-500/15 text-emerald-300 border border-emerald-500/30 ${className}`}
        >
          <CheckCircle2 size={12} className="text-emerald-400" />
          Completed
        </span>
      );
    case 'paused':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-amber-500/15 text-amber-300 border border-amber-500/30 ${className}`}
        >
          <PauseCircle size={12} className="text-amber-400" />
          Paused
        </span>
      );
    case 'failed':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-rose-500/15 text-rose-300 border border-rose-500/30 ${className}`}
        >
          <AlertCircle size={12} className="text-rose-400" />
          Failed
        </span>
      );
    case 'scheduled':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-purple-500/15 text-purple-300 border border-purple-500/30 ${className}`}
        >
          <Clock size={12} className="text-purple-400" />
          Scheduled
        </span>
      );
    case 'queued':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-slate-500/15 text-slate-300 border border-slate-500/30 ${className}`}
        >
          <ArrowDownCircle size={12} className="text-slate-400" />
          Queued
        </span>
      );
    case 'canceled':
      return (
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium bg-zinc-500/15 text-zinc-400 border border-zinc-500/30 ${className}`}
        >
          <XCircle size={12} className="text-zinc-400" />
          Canceled
        </span>
      );
    default:
      return null;
  }
};
