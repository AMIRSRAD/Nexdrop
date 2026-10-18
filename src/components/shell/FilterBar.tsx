import React from 'react';
import { CalendarClock, ChevronDown, Layers, ListOrdered, Pause, Play, RotateCw, Search, Trash, X } from 'lucide-react';
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
  /** 'all' or a queue id. */
  queue: string;
  onQueue: (id: string) => void;
  onBulkMove: (queueId: string) => void;
}

export const FilterBar: React.FC<FilterBarProps> = (p) => {
  const typeMenu = useDropdown('end');
  const queueMenu = useDropdown('end');
  const moveMenu = useDropdown('end');
  const multiQueue = p.queues.length > 1;

  if (p.selectionCount > 0) {
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

  const activeQueue = p.queue === 'all' ? null : p.queues.find((q) => q.id === p.queue);
  const queueItems: MenuItem[] = [
    { label: 'All queues', icon: Layers, onSelect: () => p.onQueue('all'), checked: p.queue === 'all' },
    { separator: true },
    ...p.queues.map<MenuItem>((q) => ({
      label: q.name,
      icon: q.schedule_enabled ? CalendarClock : ListOrdered,
      onSelect: () => p.onQueue(q.id),
      checked: p.queue === q.id,
      hint: q.active + q.queued > 0 ? String(q.active + q.queued) : undefined,
    })),
  ];

  return (
    <div className="flex h-11 shrink-0 items-center gap-3 border-b border-line px-4">
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
        {multiQueue && (
          <>
            <button
              type="button"
              onClick={queueMenu.open}
              className={cn(
                'flex h-7 items-center gap-1.5 rounded-md px-2 text-[12.5px] font-medium transition-colors',
                activeQueue ? 'bg-accent-soft text-accent' : 'text-fg-2 hover:bg-hover hover:text-fg',
              )}
            >
              <ListOrdered size={14} />
              {activeQueue ? activeQueue.name : 'All queues'}
              <ChevronDown size={13} className="opacity-70" />
            </button>
            <Menu position={queueMenu.position} onClose={queueMenu.close} items={queueItems} minWidth={190} />
          </>
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
