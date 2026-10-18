import React from 'react';
import { ArrowDown, CalendarClock, Gauge, Power, Puzzle } from 'lucide-react';
import type { AppSettings, BrowserInfo, QueueInfo } from '../../types/download';
import { formatSpeed } from '../../utils/format';
import { Menu, useDropdown, type MenuItem } from '../ui/Menu';
import { Tooltip } from '../ui/Tooltip';
import { GRABBER_PORT } from '../../lib/meta';
import { cn } from '../../lib/cn';

interface StatusBarProps {
  speed: number;
  history: number[];
  downloading: number;
  queued: number;
  settings: AppSettings;
  onChangeSettings: (patch: Partial<AppSettings>) => void;
  onOpenSettings: (section: string) => void;
  queues: QueueInfo[];
  /** Browsers found / connected, to nudge the user until the extension is set up. */
  browsers?: BrowserInfo[];
  onSetupBrowsers?: () => void;
}

const LIMITS = [0, 256, 512, 1024, 2048, 5120, 10240]; // KB/s

const limitLabel = (bytes: number) => (bytes > 0 ? formatSpeed(bytes) : 'Unlimited');

export const StatusBar: React.FC<StatusBarProps> = ({
  speed,
  history,
  downloading,
  queued,
  settings,
  onChangeSettings,
  onOpenSettings,
  queues,
  browsers = [],
  onSetupBrowsers,
}) => {
  const found = browsers.filter((b) => b.installed);
  const connected = browsers.some((b) => b.connected);
  const nudge = found.length > 0 && !browsers.some((b) => b.ever_connected);
  const scheduled = queues.filter((q) => q.schedule_enabled);
  const first = scheduled[0];
  const limitMenu = useDropdown('end');
  const doneMenu = useDropdown('end');

  const limitItems: MenuItem[] = [
    { heading: 'Global speed limit' },
    ...LIMITS.map<MenuItem>((kb) => ({
      label: kb === 0 ? 'Unlimited' : formatSpeed(kb * 1024),
      checked: settings.global_speed_limit === kb * 1024,
      onSelect: () => onChangeSettings({ global_speed_limit: kb * 1024 }),
    })),
    { separator: true },
    { label: 'Custom…', icon: Gauge, onSelect: () => onOpenSettings('connections') },
  ];

  const doneOptions: { value: AppSettings['on_complete_action']; label: string }[] = [
    { value: 'notify', label: 'Notify me' },
    { value: 'none', label: 'Do nothing' },
    { value: 'sleep', label: 'Sleep computer' },
    { value: 'shutdown', label: 'Shut down computer' },
  ];
  const doneItems: MenuItem[] = [
    { heading: 'When all downloads finish' },
    ...doneOptions.map<MenuItem>((o) => ({
      label: o.label,
      checked: settings.on_complete_action === o.value,
      onSelect: () => onChangeSettings({ on_complete_action: o.value }),
    })),
  ];

  const powerAction = settings.on_complete_action === 'sleep' || settings.on_complete_action === 'shutdown';

  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-line bg-surface px-3.5 text-[12px] text-fg-2">
      <div className="flex items-center gap-2">
        <ArrowDown size={13} className={speed > 0 ? 'text-accent' : 'text-fg-3'} strokeWidth={2.2} />
        <span className={cn('w-[74px] font-medium tabular', speed > 0 ? 'text-fg' : 'text-fg-3')}>{formatSpeed(speed)}</span>
        <Sparkline values={history} />
      </div>

      <span className="text-fg-3 tabular">
        {downloading > 0 ? `${downloading} downloading` : 'Idle'}
        {queued > 0 && ` · ${queued} queued`}
      </span>

      <div className="ml-auto flex items-center gap-1">
        {first && (
          <Chip
            icon={CalendarClock}
            onClick={() => onOpenSettings('scheduler')}
            tip={scheduled.map((q) => `${q.name}: ${q.open ? 'open' : 'waiting'}`).join(' · ')}
          >
            {first.start_time}
            {first.stop_time ? `–${first.stop_time}` : ''}
            {scheduled.length > 1 && ` +${scheduled.length - 1}`}
          </Chip>
        )}

        <Chip icon={Power} onClick={doneMenu.open} tone={powerAction ? 'warning' : undefined} tip="When all downloads finish">
          {doneOptions.find((o) => o.value === settings.on_complete_action)?.label ?? 'Notify me'}
        </Chip>
        <Menu position={doneMenu.position} onClose={doneMenu.close} items={doneItems} />

        <Chip icon={Gauge} onClick={limitMenu.open} tone={settings.global_speed_limit > 0 ? 'accent' : undefined} tip="Speed limit">
          {limitLabel(settings.global_speed_limit)}
        </Chip>
        <Menu position={limitMenu.position} onClose={limitMenu.close} items={limitItems} minWidth={180} />

        <Chip
          icon={Puzzle}
          onClick={() => (nudge && onSetupBrowsers ? onSetupBrowsers() : onOpenSettings('integrations'))}
          tone={nudge ? 'warning' : undefined}
          tip={
            connected
              ? 'Your browser is connected'
              : nudge
                ? 'Connect your browser to catch downloads and videos'
                : `Browser bridge on 127.0.0.1:${GRABBER_PORT}`
          }
        >
          <span className={cn('h-1.5 w-1.5 rounded-full', connected ? 'bg-success' : nudge ? 'bg-warning' : 'bg-fg-3')} />
          {nudge ? 'Connect browser' : 'Browser'}
        </Chip>
      </div>
    </footer>
  );
};

const Chip: React.FC<{
  icon: React.ElementType;
  onClick: (e: React.MouseEvent<HTMLElement>) => void;
  children: React.ReactNode;
  tone?: 'warning' | 'accent';
  tip?: string;
}> = ({ icon: Icon, onClick, children, tone, tip }) => (
  <Tooltip content={tip} side="top">
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex h-6 items-center gap-1.5 rounded-md px-2 text-[12px] font-medium tabular transition-colors hover:bg-hover hover:text-fg',
        tone === 'warning' && 'text-warning hover:text-warning',
        tone === 'accent' && 'text-accent hover:text-accent',
      )}
    >
      <Icon size={13} />
      {children}
    </button>
  </Tooltip>
);

const Sparkline: React.FC<{ values: number[] }> = React.memo(({ values }) => {
  const w = 96;
  const h = 16;
  const recent = values.slice(-40);
  const max = Math.max(...recent, 1);
  const pts = recent.map((v, i) => `${(i / (recent.length - 1)) * w},${h - 1 - (v / max) * (h - 3)}`);
  const line = pts.join(' ');
  return (
    <svg width={w} height={h} className="overflow-visible" aria-hidden>
      <polygon points={`0,${h} ${line} ${w},${h}`} fill="var(--accent)" opacity={0.12} />
      <polyline points={line} fill="none" stroke="var(--accent)" strokeWidth={1.25} strokeLinejoin="round" opacity={max > 1 ? 0.9 : 0.3} />
    </svg>
  );
});
Sparkline.displayName = 'Sparkline';
