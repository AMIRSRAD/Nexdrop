import React, { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowDown, ArrowUp } from 'lucide-react';
import type { DownloadTask } from '../../types/download';
import { Checkbox } from '../ui/Field';
import { cn } from '../../lib/cn';
import { TransferRow } from './TransferRow';
import { canPause, canResume, canRetry, type TaskHandlers } from './taskMenu';
import { COLUMNS, useColumnWidths, type ColumnKey } from '../../hooks/useColumns';

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
  /** Rows picked by dragging a box over them (`additive`: with Ctrl, keep what was selected). */
  onSelectMany: (ids: string[], additive: boolean) => void;
  /** A click on empty space. */
  onClearSelection: () => void;
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
  onSelectMany,
  onClearSelection,
  handlers,
}) => {
  const bodyRef = useRef<HTMLDivElement>(null);
  const columns = useColumnWidths();

  // ── drag a box over rows to select them ──
  const [box, setBox] = useState<{ x1: number; y1: number; x2: number; y2: number } | null>(null);
  const drag = useRef<{ x: number; y: number; additive: boolean; base: string[]; moved: boolean; client: { x: number; y: number } } | null>(null);
  const suppressClick = useRef(false);
  const scrollTimer = useRef<number | undefined>(undefined);
  const tasksRef = useRef(tasks);
  tasksRef.current = tasks;

  const contentPoint = (clientX: number, clientY: number) => {
    const body = bodyRef.current!;
    const r = body.getBoundingClientRect();
    return { x: clientX - r.left, y: clientY - r.top + body.scrollTop };
  };

  const updateBox = () => {
    const d = drag.current;
    const body = bodyRef.current;
    if (!d || !body) return;
    const now = contentPoint(d.client.x, d.client.y);
    const rect = { x1: Math.min(d.x, now.x), y1: Math.min(d.y, now.y), x2: Math.max(d.x, now.x), y2: Math.max(d.y, now.y) };
    setBox(rect);
    const bodyTop = body.getBoundingClientRect().top;
    const hits: string[] = [];
    body.querySelectorAll<HTMLElement>('[data-row-id]').forEach((el) => {
      const r = el.getBoundingClientRect();
      const top = r.top - bodyTop + body.scrollTop;
      if (top + r.height > rect.y1 && top < rect.y2) hits.push(el.dataset.rowId!);
    });
    onSelectMany(d.additive ? [...new Set([...d.base, ...hits])] : hits, d.additive);
  };

  const stopDrag = () => {
    window.clearInterval(scrollTimer.current);
    drag.current = null;
    setBox(null);
  };
  useEffect(() => () => window.clearInterval(scrollTimer.current), []);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest('button, a, input, textarea, select, [role="checkbox"], [role="switch"]')) return;
    const row = target.closest<HTMLElement>('[data-row-id]');
    const onBackground = target === bodyRef.current;
    if (!row && !onBackground) return; // inside a details panel
    const additive = e.ctrlKey || e.metaKey;
    // A selected row is dragged (to a queue); the box starts from anything else.
    if (row && selection.has(row.dataset.rowId!) && !additive) return;
    const p = contentPoint(e.clientX, e.clientY);
    drag.current = { x: p.x, y: p.y, additive, base: additive ? [...selection] : [], moved: false, client: { x: e.clientX, y: e.clientY } };
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    d.client = { x: e.clientX, y: e.clientY };
    if (!d.moved) {
      const p = contentPoint(e.clientX, e.clientY);
      if (Math.abs(p.x - d.x) < 5 && Math.abs(p.y - d.y) < 5) return;
      d.moved = true;
      bodyRef.current?.setPointerCapture(e.pointerId);
      // Keep scrolling while the pointer rests near the top or bottom edge.
      scrollTimer.current = window.setInterval(() => {
        const body = bodyRef.current;
        const cur = drag.current;
        if (!body || !cur) return;
        const r = body.getBoundingClientRect();
        const edge = 36;
        if (cur.client.y < r.top + edge) body.scrollTop -= 14;
        else if (cur.client.y > r.bottom - edge) body.scrollTop += 14;
        else return;
        updateBox();
      }, 40);
    }
    updateBox();
  };

  const onPointerUp = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    if (d.moved) {
      suppressClick.current = true;
      window.setTimeout(() => (suppressClick.current = false), 0);
      try {
        bodyRef.current?.releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
    } else if (e.target === bodyRef.current && !d.additive) {
      onClearSelection(); // a click on empty space
    }
    stopDrag();
  };
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
      <div role="row" style={columns.vars} className="nx-grid h-8 shrink-0 border-b border-line bg-canvas text-[11.5px] font-medium text-fg-3 select-none">
        <div className="flex justify-center">
          <Checkbox checked={allSelected} indeterminate={someSelected} onChange={(v) => onSelectAll(v)} label="Select all" />
        </div>
        <HeaderCell label="Name" k="name" sort={sort} onSort={onSort} className="pl-11" />
        <HeaderCell label="Size" k="size" sort={sort} onSort={onSort} align="right" col="size" columns={columns} />
        <HeaderCell label="Progress" k="progress" sort={sort} onSort={onSort} col="progress" columns={columns} />
        <HeaderCell label="Speed" k="speed" sort={sort} onSort={onSort} align="right" col="speed" columns={columns} />
        <HeaderCell label="Time left" k="eta" sort={sort} onSort={onSort} align="right" col="eta" columns={columns} />
        <HeaderCell label="Added" k="added" sort={sort} onSort={onSort} align="right" className="nx-col-added" col="added" columns={columns} />
        <div />
      </div>

      <div
        ref={bodyRef}
        role="rowgroup"
        style={columns.vars}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={stopDrag}
        onClickCapture={(e) => {
          if (suppressClick.current) {
            e.stopPropagation();
            e.preventDefault();
          }
        }}
        className="relative min-h-0 flex-1 overflow-y-auto select-none"
      >
        {box && (
          <div
            aria-hidden
            className="pointer-events-none absolute z-20 rounded-sm border border-accent/70 bg-accent/10"
            style={{ left: box.x1, top: box.y1, width: box.x2 - box.x1, height: box.y2 - box.y1 }}
          />
        )}
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
  /** The column this header can resize (every column but Name). */
  col?: ColumnKey;
  columns?: ReturnType<typeof useColumnWidths>;
}> = ({ label, k, sort, onSort, align = 'left', className, col, columns }) => {
  const active = sort.key === k;
  const Arrow = sort.dir === 'asc' ? ArrowUp : ArrowDown;
  const wrap = col && className?.includes('nx-col-added') ? 'nx-col-added' : '';

  // The handle sits on the column's left edge and follows the pointer: the column grows
  // to the left and Name gives way.
  const startResize = (e: React.PointerEvent) => {
    if (!col || !columns) return;
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = columns.widths[col];
    const move = (ev: PointerEvent) => columns.setWidth(col, startW - (ev.clientX - startX));
    const up = (ev: PointerEvent) => {
      columns.setWidth(col, startW - (ev.clientX - startX), true);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      document.body.style.cursor = '';
    };
    document.body.style.cursor = 'col-resize';
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  return (
    <div className={cn('relative flex h-full min-w-0', wrap)}>
      {col && (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label={`Resize ${label}`}
          title="Drag to resize · double-click to reset"
          onPointerDown={startResize}
          onDoubleClick={() => columns?.reset(col)}
          className="group/handle absolute top-0 left-0 z-10 flex h-full w-2 -translate-x-1 cursor-col-resize items-center justify-center"
        >
          <span className="h-4 w-px bg-line transition-colors group-hover/handle:bg-accent" />
        </div>
      )}
      <button
        type="button"
        onClick={() => onSort(k)}
        className={cn(
          'flex h-full min-w-0 flex-1 items-center gap-1 pr-4 transition-colors hover:text-fg-2',
          align === 'right' && 'justify-end',
          active && 'text-fg-2',
          className?.replace('nx-col-added', ''),
        )}
      >
        {align === 'right' && active && <Arrow size={12} />}
        <span className="truncate">{label}</span>
        {align === 'left' && active && <Arrow size={12} />}
      </button>
    </div>
  );
};
