import React, { useEffect, useState } from 'react';
import { ArrowRight, ChartColumn, Link2, ListPlus, Magnet, Pause, Play, Settings, Terminal } from 'lucide-react';
import { isTauri } from '../../services/api';
import { Button, IconButton, Kbd } from '../ui/Button';
import { isMagnet, looksLikeCurl } from '../../lib/meta';
import { cn } from '../../lib/cn';
import appIcon from '../../assets/app-icon.png';

interface TitleBarProps {
  omniboxRef: React.RefObject<HTMLInputElement | null>;
  onSubmitLink: (text: string) => void;
  onAddMultiple: () => void;
  onAddTorrent: () => void;
  onStats: () => void;
  onSettings: () => void;
  onPauseAll: () => void;
  onResumeAll: () => void;
  canPauseAll: boolean;
  canResumeAll: boolean;
  settingsOpen: boolean;
}

export const TitleBar: React.FC<TitleBarProps> = ({
  omniboxRef,
  onSubmitLink,
  onAddMultiple,
  onAddTorrent,
  onStats,
  onSettings,
  onPauseAll,
  onResumeAll,
  canPauseAll,
  canResumeAll,
  settingsOpen,
}) => {
  const [value, setValue] = useState('');
  const [focused, setFocused] = useState(false);
  const isCurl = looksLikeCurl(value);
  const isTorrent = isMagnet(value);

  const submit = () => {
    const text = value.trim();
    if (!text) return;
    onSubmitLink(text);
    setValue('');
    omniboxRef.current?.blur();
  };

  return (
    <header
      data-tauri-drag-region
      className="relative z-20 flex h-12 shrink-0 items-center gap-3 border-b border-line bg-surface pl-3.5"
    >
      {/* Brand */}
      <div data-tauri-drag-region className="flex w-[148px] shrink-0 items-center gap-2">
        <LogoMark />
        <span data-tauri-drag-region className="text-[13.5px] font-semibold tracking-[-0.01em] text-fg">
          Nexdrop
        </span>
      </div>

      {/* Omnibox — the primary way in: paste a link, press Enter */}
      <form
        className="flex min-w-0 flex-1 justify-center"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div
          className={cn(
            'relative flex h-8 w-full max-w-[560px] items-center rounded-lg bg-surface-2 transition-[box-shadow,background-color] duration-100',
            focused ? 'bg-canvas shadow-[0_0_0_1px_var(--accent),0_0_0_4px_var(--accent-soft)]' : 'shadow-[inset_0_0_0_1px_var(--line)] hover:shadow-[inset_0_0_0_1px_var(--line-strong)]',
          )}
        >
          {isCurl ? (
            <Terminal size={15} className="pointer-events-none absolute left-2.5 text-accent" />
          ) : (
            <Link2 size={15} className="pointer-events-none absolute left-2.5 text-fg-3" />
          )}
          <input
            ref={omniboxRef}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setValue('');
                e.currentTarget.blur();
              }
            }}
            spellCheck={false}
            placeholder="Paste a link, magnet or cURL command to download"
            className="h-full w-full bg-transparent pr-24 pl-8 text-[13px] text-fg outline-none"
          />
          <div className="absolute right-1 flex items-center gap-1">
            {value.trim() ? (
              <Button type="submit" variant="primary" size="sm" className="h-6 gap-1 px-2">
                {isCurl ? 'Import' : isTorrent ? 'Add torrent' : 'Download'}
                <ArrowRight size={13} />
              </Button>
            ) : (
              !focused && (
                <span className="flex items-center gap-0.5 pr-1">
                  <Kbd>Ctrl</Kbd>
                  <Kbd>L</Kbd>
                </span>
              )
            )}
          </div>
        </div>
      </form>

      {/* Actions */}
      <div className="flex shrink-0 items-center gap-0.5">
        <IconButton icon={Magnet} label="Add torrent" shortcut="Ctrl+T" onClick={onAddTorrent} />
        <IconButton icon={ListPlus} label="Add multiple" shortcut="Ctrl+B" onClick={onAddMultiple} />
        {canPauseAll || !canResumeAll ? (
          <IconButton icon={Pause} label="Pause all" onClick={onPauseAll} disabled={!canPauseAll} />
        ) : (
          <IconButton icon={Play} label="Resume all" onClick={onResumeAll} />
        )}
        <IconButton icon={ChartColumn} label="Statistics" onClick={onStats} />
        <IconButton icon={Settings} label="Settings" shortcut="Ctrl+," onClick={onSettings} active={settingsOpen} />
      </div>

      <WindowControls />
    </header>
  );
};

/** The real app icon, so the title bar matches the taskbar, tray and installer. */
export const LogoMark: React.FC<{ size?: number }> = ({ size = 22 }) => (
  <img src={appIcon} width={size} height={size} alt="" draggable={false} className="shrink-0 select-none" data-tauri-drag-region />
);

/** Native-feeling caption buttons for the frameless window (Windows layout). */
const WindowControls: React.FC = () => {
  const [maximized, setMaximized] = useState(false);
  const [win, setWin] = useState<any>(null);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
      const w = getCurrentWindow();
      if (cancelled) return;
      setWin(w);
      setMaximized(await w.isMaximized().catch(() => false));
      unlisten = await w.onResized(async () => setMaximized(await w.isMaximized().catch(() => false)));
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!isTauri()) return <div className="w-2" />;

  const btn = 'flex h-12 w-[46px] items-center justify-center text-fg-2 transition-colors duration-75 hover:bg-hover hover:text-fg';
  return (
    <div className="ml-1 flex h-full items-stretch self-stretch">
      <button type="button" aria-label="Minimize" className={btn} onClick={() => win?.minimize()}>
        <svg width="10" height="10" viewBox="0 0 10 10">
          <path d="M0 5h10" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
      <button type="button" aria-label={maximized ? 'Restore' : 'Maximize'} className={btn} onClick={() => win?.toggleMaximize()}>
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
            <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="0.5" y="0.5" width="9" height="9" rx="1.2" />
          </svg>
        )}
      </button>
      <button
        type="button"
        aria-label="Close"
        className={cn(btn, 'hover:bg-[#c42b1c] hover:text-white')}
        onClick={() => win?.close()}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor" strokeWidth="1.1">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
        </svg>
      </button>
    </div>
  );
};
