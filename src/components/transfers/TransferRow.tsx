import React from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ArrowUp, CalendarClock, CircleCheck, Ellipsis, ExternalLink, FolderOpen, Pause, Play, Radio, RotateCw, Square } from 'lucide-react';
import type { DownloadTask } from '../../types/download';
import { formatBytes, formatDate, formatDuration, formatRelative, formatSpeed, hostOf } from '../../utils/format';
import { progressOf } from '../../lib/meta';
import { cn } from '../../lib/cn';
import { Checkbox } from '../ui/Field';
import { IconButton } from '../ui/Button';
import { FileTile } from './FileTile';
import { ProgressTrack } from './ProgressTrack';
import { TransferDetails } from './TransferDetails';
import { canPause, canResume, canRetry, type TaskHandlers } from './taskMenu';

interface TransferRowProps {
  task: DownloadTask;
  expanded: boolean;
  selected: boolean;
  focused: boolean;
  selectionMode: boolean;
  handlers: TaskHandlers;
}

export const TransferRow: React.FC<TransferRowProps> = React.memo(
  ({ task, expanded, selected, focused, selectionMode, handlers: h }) => {
    const progress = progressOf(task);
    const isActive = task.status === 'downloading';
    const done = task.status === 'completed';
    const seeding = done && task.kind === 'torrent' && !!task.seeding;

    const onRowClick = (e: React.MouseEvent) => {
      h.focus(task.id);
      if (e.ctrlKey || e.metaKey) return h.toggleSelect(task.id);
      if (e.shiftKey) return h.toggleSelect(task.id, true);
      if (selectionMode) return h.toggleSelect(task.id);
      h.toggleExpand(task.id);
    };

    return (
      <div
        className={cn(
          'border-b border-line transition-colors duration-75',
          expanded && 'bg-surface',
          selected && 'bg-accent-soft',
        )}
      >
        <div
          data-row-id={task.id}
          tabIndex={focused ? 0 : -1}
          role="row"
          aria-selected={selected}
          aria-expanded={expanded}
          onClick={onRowClick}
          onDoubleClick={() => (done ? h.open(task.id) : undefined)}
          onContextMenu={(e) => {
            e.preventDefault();
            h.focus(task.id);
            h.contextMenu(task, e.clientX, e.clientY);
          }}
          className={cn(
            'nx-grid group relative h-[58px] outline-none',
            !selected && 'hover:bg-hover',
            'focus-visible:shadow-[inset_2px_0_0_var(--accent)]',
          )}
        >
          {/* Select */}
          <div className="flex justify-center">
            <Checkbox
              checked={selected}
              onChange={() => h.toggleSelect(task.id)}
              label={`Select ${task.filename}`}
              className={cn(
                'transition-opacity duration-100',
                selected || selectionMode ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
              )}
            />
          </div>

          {/* Name */}
          <div className="flex min-w-0 items-center gap-3 pr-4">
            <FileTile category={task.category} torrent={task.kind === 'torrent'} />
            <div className="min-w-0">
              <div className={cn('truncate text-[13px] font-medium', done || isActive ? 'text-fg' : 'text-fg-2')} title={task.filename}>
                {task.filename}
              </div>
              <Subline task={task} />
            </div>
          </div>

          {/* Size */}
          <div className="pr-4 text-right text-[12.5px] text-fg-2 tabular">
            {task.total_bytes > 0 ? formatBytes(task.total_bytes) : '—'}
          </div>

          {/* Progress */}
          <div className="min-w-0 pr-5">
            <ProgressCell task={task} progress={progress} />
          </div>

          {/* Speed */}
          <div
            className={cn(
              'pr-4 text-right text-[12.5px] tabular',
              (isActive && task.speed_bps > 0) || seeding ? 'font-medium text-fg' : 'text-fg-3',
            )}
          >
            {isActive && task.speed_bps > 0 ? (
              formatSpeed(task.speed_bps)
            ) : seeding && (task.upload_bps ?? 0) > 0 ? (
              <span className="inline-flex items-center gap-0.5 text-accent">
                <ArrowUp size={12} strokeWidth={2.4} />
                {formatSpeed(task.upload_bps ?? 0)}
              </span>
            ) : (
              '—'
            )}
          </div>

          {/* ETA */}
          <div className={cn('pr-4 text-right text-[12.5px] tabular', isActive ? 'text-fg-2' : 'text-fg-3')}>
            {isActive ? formatDuration(task.eta_seconds) : '—'}
          </div>

          {/* Added */}
          <div className="nx-col-added pr-4 text-right text-[12px] text-fg-3 tabular" title={formatDate(task.created_at)}>
            {formatRelative(task.created_at)}
          </div>

          {/* Actions */}
          <div
            className={cn(
              'flex items-center justify-end gap-0.5 pr-3 transition-opacity duration-100',
              expanded ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 focus-within:opacity-100',
            )}
            onClick={(e) => e.stopPropagation()}
            onDoubleClick={(e) => e.stopPropagation()}
          >
            <PrimaryAction task={task} h={h} />
            {done && task.kind === 'torrent' && (
              <IconButton
                size="sm"
                icon={seeding ? Square : Radio}
                label={seeding ? 'Stop sharing' : 'Start sharing'}
                onClick={() => (seeding ? h.stopSeeding(task.id) : h.startSeeding(task.id))}
              />
            )}
            <IconButton size="sm" icon={FolderOpen} label="Show in folder" onClick={() => h.reveal(task.id)} />
            <IconButton
              size="sm"
              icon={Ellipsis}
              label="More"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                h.focus(task.id);
                h.contextMenu(task, r.right, r.bottom + 4);
              }}
            />
          </div>
        </div>

        <AnimatePresence initial={false}>
          {expanded && (
            <motion.div
              key="details"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.18, ease: [0.2, 0.9, 0.3, 1] }}
              className="overflow-hidden"
            >
              <TransferDetails task={task} handlers={h} />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    );
  },
);
TransferRow.displayName = 'TransferRow';

const PrimaryAction: React.FC<{ task: DownloadTask; h: TaskHandlers }> = ({ task, h }) => {
  if (task.status === 'completed') return <IconButton size="sm" icon={ExternalLink} label="Open" onClick={() => h.open(task.id)} />;
  if (canPause(task)) return <IconButton size="sm" icon={Pause} label="Pause" onClick={() => h.pause(task.id)} />;
  if (canResume(task))
    return <IconButton size="sm" icon={Play} label={task.status === 'scheduled' ? 'Start now' : 'Resume'} onClick={() => h.resume(task.id)} />;
  if (canRetry(task)) return <IconButton size="sm" icon={RotateCw} label="Retry" onClick={() => h.retry(task.id)} />;
  return null;
};

const Subline: React.FC<{ task: DownloadTask }> = ({ task }) => {
  if (task.kind === 'torrent' && task.status !== 'failed') {
    const files = task.torrent_files?.length ?? 0;
    const ratio = task.total_bytes > 0 ? (task.uploaded_bytes ?? 0) / task.total_bytes : 0;
    const bits: string[] = [files > 1 ? `Torrent · ${files} files` : 'Torrent'];
    if (task.status === 'downloading') bits.push(`${task.peers ?? 0} peers`);
    if (task.status === 'completed') bits.push(task.seeding ? `ratio ${ratio.toFixed(2)}` : 'not sharing');
    return <div className="truncate text-[12px] text-fg-3">{bits.join(' · ')}</div>;
  }
  const host = hostOf(task.url);
  if ((task.status === 'failed' || task.status === 'canceled') && task.error_message) {
    return (
      <div className="truncate text-[12px] text-danger" title={task.error_message}>
        {task.error_message}
      </div>
    );
  }
  return (
    <div className="flex min-w-0 items-center gap-1.5 text-[12px] text-fg-3">
      <span className="truncate">{host}</span>
      {task.priority >= 1 && task.status !== 'completed' && (
        <span
          className={cn(
            'shrink-0 rounded px-1 text-[10.5px] leading-4 font-semibold',
            task.priority >= 2 ? 'bg-danger/14 text-danger' : 'bg-warning/16 text-warning',
          )}
        >
          {task.priority >= 2 ? 'Urgent' : 'High'}
        </span>
      )}
      {task.status === 'downloading' && task.connections > 1 && (
        <span className="shrink-0 tabular">· {task.connections} connections</span>
      )}
    </div>
  );
};

const ProgressCell: React.FC<{ task: DownloadTask; progress: number }> = ({ task, progress }) => {
  if (task.status === 'completed' && task.kind === 'torrent' && task.seeding) {
    const ratio = task.total_bytes > 0 ? (task.uploaded_bytes ?? 0) / task.total_bytes : 0;
    return (
      <div className="flex items-center gap-1.5 text-[12.5px] text-accent">
        <Radio size={14} className="nx-pulse" />
        <span className="font-medium">Seeding</span>
        <span className="truncate text-fg-3 tabular">· {ratio.toFixed(2)}</span>
      </div>
    );
  }
  if (task.status === 'completed') {
    return (
      <div className="flex items-center gap-1.5 text-[12.5px] text-success">
        <CircleCheck size={14} strokeWidth={2.2} />
        <span className="font-medium">Done</span>
        {task.completed_at && <span className="truncate text-fg-3">· {formatRelative(task.completed_at)}</span>}
      </div>
    );
  }
  if (task.status === 'scheduled') {
    return (
      <div className="flex items-center gap-1.5 text-[12.5px] text-violet" title={formatDate(task.scheduled_time)}>
        <CalendarClock size={14} />
        <span className="truncate font-medium">{task.scheduled_time ? formatDate(task.scheduled_time) : 'Scheduled'}</span>
      </div>
    );
  }

  const pct = Math.floor(progress * 1000) / 10;
  const label =
    task.status === 'queued'
      ? 'Queued'
      : task.status === 'paused'
        ? 'Paused'
        : task.status === 'failed'
          ? 'Failed'
          : task.status === 'canceled'
            ? 'Canceled'
            : `${pct.toFixed(pct >= 100 ? 0 : 1)}%`;

  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-2 text-[11.5px] leading-none tabular">
        <span
          className={cn(
            'font-medium',
            task.status === 'downloading' && 'text-fg',
            task.status === 'paused' && 'text-warning',
            task.status === 'failed' && 'text-danger',
            (task.status === 'queued' || task.status === 'canceled') && 'text-fg-3',
          )}
        >
          {label}
        </span>
        <span className="truncate text-fg-3">
          {task.total_bytes > 0 && task.status !== 'downloading' && task.downloaded_bytes > 0
            ? `${Math.round(progress * 100)}% · ${formatBytes(task.downloaded_bytes)}`
            : task.downloaded_bytes > 0
              ? formatBytes(task.downloaded_bytes)
              : ''}
        </span>
      </div>
      <ProgressTrack progress={progress} status={task.status} segments={task.segments} totalBytes={task.total_bytes} />
    </div>
  );
};
