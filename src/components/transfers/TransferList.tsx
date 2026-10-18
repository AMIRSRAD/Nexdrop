import React, { useCallback, useRef } from 'react';
import { ArrowDown, ArrowUp } from 'lucide-react';
import type { DownloadTask } from '../../types/download';
import { Checkbox } from '../ui/Field';
import { cn } from '../../lib/cn';
import { TransferRow } from './TransferRow';
import { canPause, canResume, canRetry, type TaskHandlers } from './taskMenu';

export type SortKey = 'added' | 'name' | 'size' | 'progress' | 'speed' | 'eta';
export interface SortState {
  key: SortKey;
  dir: 'asc' | 'desc';
}

interface TransferListProps {
  tasks: DownloadTask[];
  sort: SortState;
  onSort: (key: SortKey) => void;
  expandedId: string | null;
  focusedId: string | null;
  selection: Set<string>;
  onSelectAll: (select: boolean) => void;
  handlers: TaskHandlers;
}

export const TransferList: React.FC<TransferListProps> = ({
  tasks,
  sort,
  onSort,
  expandedId,
  focusedId,
  selection,
  onSelectAll,
  handlers,
}) => {
  const bodyRef = useRef<HTMLDivElement>(null);
  const selectionMode = selection.size > 0;
  const allSelected = tasks.length > 0 && tasks.every((t) => selection.has(t.id));
  const someSelected = !allSelected && tasks.some((t) => selection.has(t.id));

  const focusRow = useCallback(
    (id: string) => {
      handlers.focus(id);
      requestAnimationFrame(() => {
        const el = bodyRef.current?.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(id)}"]`);
        el?.focus();
        el?.scrollIntoView({ block: 'nearest' });
      });
    },
    [handlers],
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest('input, textarea, select')) return;
    const idx = tasks.findIndex((t) => t.id === focusedId);
    const task = idx >= 0 ? tasks[idx] : undefined;

    switch (e.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        e.preventDefault();
        if (tasks.length === 0) return;
        const next = e.key === 'ArrowDown' ? Math.min(tasks.length - 1, idx + 1) : Math.max(0, idx < 0 ? 0 : idx - 1);
        focusRow(tasks[next].id);
        if (e.shiftKey) handlers.toggleSelect(tasks[next].id, true);
        break;
      }
      case 'Home':
      case 'End':
        e.preventDefault();
        if (tasks.length) focusRow(tasks[e.key === 'Home' ? 0 : tasks.length - 1].id);
        break;
      case 'Enter':
        if (!task || (e.target as HTMLElement).closest('button')) return;
        e.preventDefault();
        if (task.status === 'completed' && !e.shiftKey) handlers.open(task.id);
        else handlers.toggleExpand(task.id);
        break;
      case ' ':
        if (!task || (e.target as HTMLElement).closest('button')) return;
        e.preventDefault();
        if (canPause(task)) handlers.pause(task.id);
        else if (canResume(task)) handlers.resume(task.id);
        else if (canRetry(task)) handlers.retry(task.id);
        break;
      case 'Delete':
        if (selection.size > 0) handlers.requestRemove([...selection]);
        else if (task) handlers.requestRemove([task.id]);
        break;
      case 'a':
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          onSelectAll(true);
        }
        break;
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" onKeyDown={onKeyDown}>
      <div role="row" className="nx-grid h-8 shrink-0 border-b border-line bg-canvas text-[11.5px] font-medium text-fg-3 select-none">
        <div className="flex justify-center">
          <Checkbox checked={allSelected} indeterminate={someSelected} onChange={(v) => onSelectAll(v)} label="Select all" />
        </div>
        <HeaderCell label="Name" k="name" sort={sort} onSort={onSort} className="pl-11" />
        <HeaderCell label="Size" k="size" sort={sort} onSort={onSort} align="right" />
        <HeaderCell label="Progress" k="progress" sort={sort} onSort={onSort} />
        <HeaderCell label="Speed" k="speed" sort={sort} onSort={onSort} align="right" />
        <HeaderCell label="Time left" k="eta" sort={sort} onSort={onSort} align="right" />
        <HeaderCell label="Added" k="added" sort={sort} onSort={onSort} align="right" className="nx-col-added" />
        <div />
      </div>

      <div ref={bodyRef} role="rowgroup" className="min-h-0 flex-1 overflow-y-auto">
        {tasks.map((task) => (
          <TransferRow
            key={task.id}
            task={task}
            expanded={expandedId === task.id}
            selected={selection.has(task.id)}
            focused={focusedId === task.id || (!focusedId && task === tasks[0])}
            selectionMode={selectionMode}
            handlers={handlers}
          />
        ))}
      </div>
    </div>
  );
};

const HeaderCell: React.FC<{
  label: string;
  k: SortKey;
  sort: SortState;
  onSort: (k: SortKey) => void;
  align?: 'left' | 'right';
  className?: string;
}> = ({ label, k, sort, onSort, align = 'left', className }) => {
  const active = sort.key === k;
  const Arrow = sort.dir === 'asc' ? ArrowUp : ArrowDown;
  return (
    <button
      type="button"
      onClick={() => onSort(k)}
      className={cn(
        'flex h-full items-center gap-1 pr-4 transition-colors hover:text-fg-2',
        align === 'right' && 'justify-end',
        active && 'text-fg-2',
        className,
      )}
    >
      {align === 'right' && active && <Arrow size={12} />}
      {label}
      {align === 'left' && active && <Arrow size={12} />}
    </button>
  );
};
