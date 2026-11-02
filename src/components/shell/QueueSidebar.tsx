import React, { useState } from 'react';
import { CalendarClock, Inbox, ListOrdered, PanelLeftClose, Pencil, Play, Plus, Square } from 'lucide-react';
import type { QueueInfo } from '../../types/download';
import { Menu, type MenuItem, type MenuPosition } from '../ui/Menu';
import { whenText } from '../../hooks/useQueues';
import { cn } from '../../lib/cn';

interface QueueSidebarProps {
  queues: QueueInfo[];
  /** 'all' or a queue id. */
  selected: string;
  onSelect: (id: string) => void;
  /** Downloads in each queue (any state), and in total. */
  counts: Record<string, number>;
  total: number;
  /** Queue ids that hold something that could start (waiting or paused). */
  startable: Set<string>;
  onStart: (id: string) => void;
  onStop: (id: string) => void;
  onEdit: () => void;
  onNew: () => void;
  onCollapse: () => void;
}

/** What a queue is doing, in a few words. */
function stateOf(q: QueueInfo, count: number): { text: string; tone: 'run' | 'wait' | 'idle' } {
  if (q.active > 0) return { text: `Downloading ${q.active}${q.queued ? ` · ${q.queued} waiting` : ''}`, tone: 'run' };
  if (!q.open && q.next_start) return { text: `Starts ${whenText(q.next_start)}`, tone: 'wait' };
  if (!q.open) return { text: 'Stopped', tone: 'idle' };
  if (q.queued > 0) return { text: `${q.queued} waiting`, tone: 'wait' };
  return { text: count > 0 ? 'Nothing running' : 'Empty', tone: 'idle' };
}

/** The queues, always in view: pick one to see only its downloads, start or stop it in place. */
export const QueueSidebar: React.FC<QueueSidebarProps> = (p) => {
  const [menu, setMenu] = useState<{ q: QueueInfo; pos: MenuPosition } | null>(null);

  const running = (q: QueueInfo) => q.active > 0 || (q.open && q.queued > 0);
  const menuItems = (q: QueueInfo): MenuItem[] => [
    { label: 'Start now', icon: Play, onSelect: () => p.onStart(q.id), disabled: !p.startable.has(q.id) && q.queued === 0 },
    { label: 'Stop', icon: Square, onSelect: () => p.onStop(q.id), disabled: !running(q) },
    { separator: true },
    { label: 'Edit queue…', icon: Pencil, onSelect: p.onEdit },
  ];

  return (
    <aside className="flex w-[212px] shrink-0 flex-col border-r border-line bg-surface/40" aria-label="Queues">
      <div className="flex h-11 shrink-0 items-center gap-1 border-b border-line pr-1.5 pl-4">
        <span className="text-[12px] font-medium text-fg-3">Queues</span>
        <button
          type="button"
          onClick={p.onNew}
          title="New queue"
          aria-label="New queue"
          className="ml-auto flex h-7 w-7 items-center justify-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
        >
          <Plus size={15} />
        </button>
        <button
          type="button"
          onClick={p.onCollapse}
          title="Hide queues"
          aria-label="Hide queues"
          className="flex h-7 w-7 items-center justify-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
        >
          <PanelLeftClose size={15} />
        </button>
      </div>

      <nav className="min-h-0 flex-1 space-y-0.5 overflow-y-auto p-2">
        <Row
          active={p.selected === 'all'}
          icon={Inbox}
          name="All downloads"
          count={p.total}
          onClick={() => p.onSelect('all')}
        />
        <div className="mx-2 my-1.5 h-px bg-line" />
        {p.queues.map((q) => {
          const count = p.counts[q.id] ?? 0;
          const st = stateOf(q, count);
          const isRunning = running(q);
          const canStart = !isRunning && (p.startable.has(q.id) || q.queued > 0);
          return (
            <Row
              key={q.id}
              active={p.selected === q.id}
              icon={q.schedule_enabled ? CalendarClock : ListOrdered}
              name={q.name}
              count={count}
              sub={st.text}
              tone={st.tone}
              onClick={() => p.onSelect(q.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ q, pos: { x: e.clientX, y: e.clientY } });
              }}
              action={
                isRunning ? (
                  <ActionButton icon={Square} label={`Stop ${q.name}`} onClick={() => p.onStop(q.id)} />
                ) : canStart ? (
                  <ActionButton icon={Play} label={`Start ${q.name}`} onClick={() => p.onStart(q.id)} />
                ) : null
              }
            />
          );
        })}
      </nav>

      <Menu position={menu?.pos ?? null} onClose={() => setMenu(null)} items={menu ? menuItems(menu.q) : []} minWidth={180} />
    </aside>
  );
};

const Row: React.FC<{
  active: boolean;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  name: string;
  count: number;
  sub?: string;
  tone?: 'run' | 'wait' | 'idle';
  onClick: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  action?: React.ReactNode;
}> = ({ active, icon: Icon, name, count, sub, tone, onClick, onContextMenu, action }) => (
  <div
    role="button"
    tabIndex={0}
    aria-pressed={active}
    onClick={onClick}
    onKeyDown={(e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        onClick();
      }
    }}
    onContextMenu={onContextMenu}
    className={cn(
      'group relative flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 outline-none transition-colors duration-100 focus-visible:shadow-[0_0_0_1.5px_var(--accent)]',
      active ? 'bg-accent-soft' : 'hover:bg-hover',
    )}
  >
    <Icon size={15} className={cn('shrink-0', active ? 'text-accent' : 'text-fg-3')} />
    <div className="min-w-0 flex-1">
      <div className={cn('truncate text-[13px] font-medium', active ? 'text-fg' : 'text-fg-2')}>{name}</div>
      {sub && (
        <div
          className={cn(
            'truncate text-[11.5px]',
            tone === 'run' ? 'text-accent' : tone === 'wait' ? 'text-warning' : 'text-fg-3',
          )}
        >
          {sub}
        </div>
      )}
    </div>
    {action ? (
      <>
        <span className="text-[11.5px] text-fg-3 tabular group-hover:hidden">{count > 0 ? count : ''}</span>
        <span className="hidden group-hover:block">{action}</span>
      </>
    ) : (
      count > 0 && <span className="text-[11.5px] text-fg-3 tabular">{count}</span>
    )}
  </div>
);

const ActionButton: React.FC<{ icon: React.ComponentType<{ size?: number }>; label: string; onClick: () => void }> = ({ icon: Icon, label, onClick }) => (
  <button
    type="button"
    title={label}
    aria-label={label}
    onClick={(e) => {
      e.stopPropagation();
      onClick();
    }}
    className="flex h-6 w-6 items-center justify-center rounded-md text-fg-2 hover:bg-surface-3 hover:text-fg"
  >
    <Icon size={13} />
  </button>
);
