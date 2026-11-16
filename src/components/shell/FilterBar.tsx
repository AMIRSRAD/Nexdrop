import React from 'react';
import { ArrowUpToLine, ChevronDown, Ellipsis, Eraser, Layers, ListOrdered, PanelLeftOpen, Pause, Play, RotateCw, Search, Trash, X, Zap } from 'lucide-react';
import { CATEGORIES, STATUS_FILTERS, categoryMeta, type StatusFilter } from '../../lib/meta';
import type { FileCategory, QueueInfo } from '../../types/download';
import { Menu, useDropdown, type MenuItem } from '../ui/Menu';
import { Button } from '../ui/Button';
import { cn } from '../../lib/cn';

interface FilterBarProps {
  status: StatusFilter;
  onStatus: (s: StatusFilter) => void;
  statusCounts: Record<StatusFilter, number>;
  category: FileCategory;
  onCategory: (c: FileCategory) => void;
  categoryCounts: Record<string, number>;
  search: string;
  onSearch: (q: string) => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
  selectionCount: number;
  onClearSelection: () => void;
  onBulkPause: () => void;
  onBulkResume: () => void;
  onBulkRetry: () => void;
  onBulkRemove: () => void;
  bulkCan: { pause: boolean; resume: boolean; retry: boolean };
  queues: QueueInfo[];
  /** The queue panel is collapsed: offer a button to bring it back. */
  queuePanelHidden: boolean;
  onShowQueuePanel: () => void;
  onBulkMove: (queueId: string) => void;
  /** Put the selected waiting downloads first in their queue. */
  onBulkNext: () => void;
  canBulkNext: boolean;
  /** Start the selected downloads together, right now. */
  onBulkNow: () => void;
  canBulkNow: boolean;
  /** Actions on the whole list (not the selection). Counts are what each would act on. */
  all: {
    running: number;
    paused: number;
    completed: number;
    failed: number;
    onPauseAll: () => void;
    onResumeAll: () => void;
    onClearCompleted: () => void;
    onRetryFailed: () => void;
    onRemoveFailed: () => void;
  };
}

export const FilterBar: React.FC<FilterBarProps> = (p) => {
  const typeMenu = useDropdown('end');
  const moreMenu = useDropdown('end');
  const moveMenu = useDropdown('end');
  const multiQueue = p.queues.length > 1;

  // The bulk bar takes over the tabs only for two or more rows; one selected row has its own menu.
  if (p.selectionCount >= 2) {
    return (
      <div className="flex h-11 shrink-0 items-center gap-2 border-b border-line bg-accent-soft px-4">
        <button
          type="button"
          onClick={p.onClearSelection}
          aria-label="Clear selection"
          className="flex h-6 w-6 items-center justify-center rounded-md text-fg-2 hover:bg-hover hover:text-fg"
        >
          <X size={15} />
        </button>
        <span className="text-[13px] font-medium text-fg tabular">{p.selectionCount} selected</span>
        <div className="ml-auto flex items-center gap-1.5">
          {p.bulkCan.resume && (
            <Button size="sm" variant="ghost" icon={Play} onClick={p.onBulkResume}>
              Resume
            </Button>
          )}
          {p.bulkCan.pause && (
            <Button size="sm" variant="ghost" icon={Pause} onClick={p.onBulkPause}>
              Pause
            </Button>
          )}
          {p.bulkCan.retry && (
            <Button size="sm" variant="ghost" icon={RotateCw} onClick={p.onBulkRetry}>
              Retry
            </Button>
          )}
          {p.canBulkNow && (
            <Button size="sm" variant="ghost" icon={Zap} onClick={p.onBulkNow}>
              Download now
            </Button>
          )}
          {p.canBulkNext && (
            <Button size="sm" variant="ghost" icon={ArrowUpToLine} onClick={p.onBulkNext}>
              Download next
            </Button>
          )}
          {multiQueue && (
            <>
              <Button size="sm" variant="ghost" icon={ListOrdered} onClick={moveMenu.open}>
                Move to queue
              </Button>
              <Menu
                position={moveMenu.position}
                onClose={moveMenu.close}
                items={p.queues.map<MenuItem>((q) => ({ label: q.name, onSelect: () => p.onBulkMove(q.id) }))}
                minWidth={180}
              />
            </>
          )}
          <Button size="sm" variant="ghost" icon={Trash} onClick={p.onBulkRemove} className="hover:text-danger">
            Remove
          </Button>
        </div>
      </div>
    );
  }

  // Buttons where they make sense: the tab you are on offers its own bulk action;
  // everything else is in the ⋯ menu.
  const a = p.all;
  const tabAction =
    p.status === 'completed' && a.completed > 0
      ? { label: 'Clear completed', icon: Eraser, run: a.onClearCompleted }
      : p.status === 'failed' && a.failed > 0
        ? { label: 'Retry all', icon: RotateCw, run: a.onRetryFailed }
        : p.status === 'active' && a.running > 0
          ? { label: 'Pause all', icon: Pause, run: a.onPauseAll }
          : p.status === 'paused' && a.paused > 0
            ? { label: 'Resume all', icon: Play, run: a.onResumeAll }
            : null;
  const moreItems: MenuItem[] = [
    { label: 'Pause all', icon: Pause, onSelect: a.onPauseAll, disabled: a.running === 0, hint: a.running ? String(a.running) : undefined },
    { label: 'Resume all', icon: Play, onSelect: a.onResumeAll, disabled: a.paused === 0, hint: a.paused ? String(a.paused) : undefined },
    { separator: true },
    { label: 'Clear completed…', icon: Eraser, onSelect: a.onClearCompleted, disabled: a.completed === 0, hint: a.completed ? String(a.completed) : undefined },
    { label: 'Retry all failed', icon: RotateCw, onSelect: a.onRetryFailed, disabled: a.failed === 0, hint: a.failed ? String(a.failed) : undefined },
    { label: 'Remove failed…', icon: Trash, onSelect: a.onRemoveFailed, disabled: a.failed === 0, hint: a.failed ? String(a.failed) : undefined },
  ];

  const activeCategory = p.category === 'all' ? null : categoryMeta(p.category);
  const typeItems: MenuItem[] = [
    { label: 'All types', icon: Layers, onSelect: () => p.onCategory('all'), checked: p.category === 'all', hint: String(p.categoryCounts.all ?? 0) },
    { separator: true },
    ...CATEGORIES.map<MenuItem>((c) => ({
      label: c.label,
      icon: c.icon,
      onSelect: () => p.onCategory(c.id),
      checked: p.category === c.id,
      hint: String(p.categoryCounts[c.id] ?? 0),
    })),
  ];

  return (
    <div className="flex h-11 shrink-0 items-center gap-3 border-b border-line px-4">
      {p.queuePanelHidden && (
        <button
          type="button"
          onClick={p.onShowQueuePanel}
          title="Show queues"
          aria-label="Show queues"
          className="-ml-1.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
        >
          <PanelLeftOpen size={15} />
        </button>
      )}
      <nav className="-mb-px flex h-full min-w-0 items-stretch gap-1 overflow-x-auto" role="tablist">
        {STATUS_FILTERS.map((f) => {
          const active = p.status === f.id;
          const count = p.statusCounts[f.id];
          return (
            <button
              key={f.id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => p.onStatus(f.id)}
              className={cn(
                'relative flex shrink-0 items-center gap-1.5 px-2 text-[13px] font-medium transition-colors duration-100',
                active ? 'text-fg' : 'text-fg-3 hover:text-fg-2',
              )}
            >
              {f.label}
              {count > 0 && (
                <span
                  className={cn(
                    'min-w-[18px] rounded-full px-1.5 text-center text-[11px] leading-[17px] tabular',
                    active ? 'bg-surface-3 text-fg-2' : 'bg-surface-2 text-fg-3',
                    f.id === 'failed' && 'bg-danger/14 text-danger',
                  )}
                >
                  {count}
                </span>
              )}
              {active && <span className="absolute inset-x-1.5 bottom-0 h-[2px] rounded-full bg-accent" />}
            </button>
          );
        })}
      </nav>

      <div className="ml-auto flex shrink-0 items-center gap-2">
        {tabAction && (
          <Button size="sm" variant="secondary" icon={tabAction.icon} onClick={tabAction.run}>
            {tabAction.label}
          </Button>
        )}
        <button
          type="button"
          onClick={typeMenu.open}
          className={cn(
            'flex h-7 items-center gap-1.5 rounded-md px-2 text-[12.5px] font-medium transition-colors',
            activeCategory ? 'bg-accent-soft text-accent' : 'text-fg-2 hover:bg-hover hover:text-fg',
          )}
        >
          {activeCategory ? <activeCategory.icon size={14} /> : <Layers size={14} />}
          {activeCategory ? activeCategory.label : 'All types'}
          <ChevronDown size={13} className="opacity-70" />
        </button>
        <Menu position={typeMenu.position} onClose={typeMenu.close} items={typeItems} minWidth={190} />
        <button
          type="button"
          onClick={moreMenu.open}
          title="More actions"
          aria-label="More actions"
          className="flex h-7 w-7 items-center justify-center rounded-md text-fg-2 transition-colors hover:bg-hover hover:text-fg"
        >
          <Ellipsis size={16} />
        </button>
        <Menu position={moreMenu.position} onClose={moreMenu.close} items={moreItems} minWidth={210} />

        <div className="relative">
          <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-fg-3" />
          <input
            ref={p.searchRef}
            value={p.search}
            onChange={(e) => p.onSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                p.onSearch('');
                e.currentTarget.blur();
              }
            }}
            spellCheck={false}
            placeholder="Filter"
            className="h-7 w-44 rounded-md bg-surface-2 pr-7 pl-8 text-[12.5px] text-fg shadow-[inset_0_0_0_1px_var(--line)] outline-none transition-[width,box-shadow] duration-150 focus:w-56 focus:shadow-[inset_0_0_0_1px_var(--accent)]"
          />
          {p.search && (
            <button
              type="button"
              aria-label="Clear filter"
              onClick={() => p.onSearch('')}
              className="absolute top-1/2 right-1 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded text-fg-3 hover:text-fg"
            >
              <X size={12} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
