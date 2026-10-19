import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { CheckSquare, ClipboardPaste, Copy, Download, Link2, ListPlus, Magnet, Settings as SettingsIcon } from 'lucide-react';
import { api, isTauri } from './services/api';
import type { BrowserRequest, GrabbedLink } from './types/download';
import { QueuesContext, useQueues } from './hooks/useQueues';
import { useBrowsers } from './hooks/useBrowsers';
import { BrowserSetupDialog } from './components/dialogs/BrowserSetupDialog';
import { listen } from '@tauri-apps/api/event';
import type { AddDownloadRequest, AddTorrentRequest, AppSettings, DownloadTask, FileCategory } from './types/download';
import { useDownloads, errText } from './hooks/useDownloads';
import { useSpeedHistory } from './hooks/useSpeedHistory';
import { useTheme } from './hooks/useTheme';
import { hasOpenLayer } from './lib/layers';
import { STATUS_FILTERS, categoryMeta, extractUrls, isMagnet, isTorrentUrl, looksLikeCurl, matchesStatus, progressOf, type StatusFilter } from './lib/meta';
import { plural } from './utils/format';
import { useToast } from './components/ui/Toast';
import { Menu, type MenuItem } from './components/ui/Menu';
import { TitleBar } from './components/shell/TitleBar';
import { FilterBar } from './components/shell/FilterBar';
import { StatusBar } from './components/shell/StatusBar';
import { TransferList, type SortKey, type SortState } from './components/transfers/TransferList';
import { EmptyState } from './components/transfers/EmptyState';
import { buildTaskMenu, canPause, canResume, canRetry, type TaskHandlers } from './components/transfers/taskMenu';
import { AddDownloadDialog } from './components/dialogs/AddDownloadDialog';
import { AddMultipleDialog, type AddMultipleTab } from './components/dialogs/AddMultipleDialog';
import { AddTorrentDialog } from './components/dialogs/AddTorrentDialog';
import { RemoveDialog } from './components/dialogs/RemoveDialog';
import { StatsDialog } from './components/dialogs/StatsDialog';
import { SettingsView, type SettingsSection } from './components/settings/SettingsView';

const DEFAULT_SETTINGS: AppSettings = {
  default_destination: 'Downloads',
  max_active_downloads: 3,
  default_connections: 8,
  max_connections_per_server: 8,
  min_chunk_size: 1048576,
  global_speed_limit: 0,
  auto_resume_on_launch: true,
  notify_on_complete: true,
  theme: 'dark',
  clipboard_watch: false,
  auto_categorize: true,
  proxy_mode: 'system',
  custom_proxy_url: undefined,
  on_complete_action: 'notify',
  sound_enabled: true,
  scheduler_enabled: false,
  scheduler_start_time: '02:00',
  scheduler_stop_time: '07:00',
  scheduler_days: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'],
  scheduler_speed_limit: 0,
  scheduler_action: 'none',
  scan_with_antivirus: true,
  antivirus_path: undefined,
  torrent_seed_after_download: true,
  torrent_ratio_limit: 1,
  torrent_upload_limit: 0,
  torrent_dht: true,
  torrent_upnp: false,
  browser_prompt: true,
  browser_setup_done: false,
  network_mode: 'auto',
  network_adapters: [],
};

const FILE_LINK_RE = /\.(zip|rar|7z|tar|gz|xz|bz2|iso|img|exe|msi|dmg|pkg|deb|rpm|apk|mp4|mkv|webm|mov|avi|mp3|flac|wav|m4a|pdf|epub|bin)(\?|#|$)/i;

export default function App() {
  const toast = useToast();

  /* ── dialogs & views ─────────────────────────────── */
  const [addState, setAddState] = useState<{ open: boolean; text: string; request?: BrowserRequest }>({ open: false, text: '' });
  const [multiState, setMultiState] = useState<{
    open: boolean;
    tab: AddMultipleTab;
    text: string;
    links?: GrabbedLink[];
    preselect?: 'all' | 'files';
    request?: BrowserRequest;
  }>({ open: false, tab: 'paste', text: '' });
  const [removeIds, setRemoveIds] = useState<string[]>([]);
  const [statsOpen, setStatsOpen] = useState(false);
  const [settingsView, setSettingsView] = useState<{ open: boolean; section: SettingsSection }>({ open: false, section: 'general' });
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [appMenu, setAppMenu] = useState<{ x: number; y: number; selection: string } | null>(null);

  const openAdd = useCallback((text = '', request?: BrowserRequest) => setAddState({ open: true, text, request }), []);
  const openMultiple = useCallback((tab: AddMultipleTab = 'paste', text = '') => setMultiState({ open: true, tab, text }), []);
  const [torrentState, setTorrentState] = useState<{ open: boolean; source: string; file: File | null }>({ open: false, source: '', file: null });
  const openTorrent = useCallback((source = '', file: File | null = null) => setTorrentState({ open: true, source, file }), []);

  /* ── data ────────────────────────────────────────── */
  const { downloads, loaded, actions } = useDownloads(
    () => openAdd(),
    (source) => openTorrent(source),
  );
  const { queues, refresh: refreshQueues } = useQueues();
  const [browserSetupOpen, setBrowserSetupOpen] = useState(false);
  const [queueFilter, setQueueFilter] = useState('all');
  // A deleted queue can't stay selected.
  useEffect(() => {
    if (queueFilter !== 'all' && queues.length > 0 && !queues.some((q) => q.id === queueFilter)) setQueueFilter('all');
    if (queues.length < 2 && queueFilter !== 'all') setQueueFilter('all');
  }, [queues, queueFilter]);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved'>('idle');
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const saveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        setSettings({ ...DEFAULT_SETTINGS, ...s });
        setSettingsLoaded(true);
      })
      .catch((err) => console.error('Failed to load settings', err));
  }, []);

  useTheme(settings.theme);

  const changeSettings = useCallback(
    (patch: Partial<AppSettings>) => {
      setSettings((prev) => ({ ...prev, ...patch }));
      setSaveState('saving');
      window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(async () => {
        try {
          await api.updateSettings(settingsRef.current);
          setSaveState('saved');
        } catch (err) {
          setSaveState('idle');
          toast.show({ tone: 'error', title: "Couldn't save settings", description: errText(err) });
        }
      }, 450);
    },
    [toast],
  );

  /* ── browsers: first-run setup and the status-bar nudge ─ */
  const { data: browserData } = useBrowsers(browserSetupOpen || (settingsView.open && settingsView.section === 'integrations'));
  const browserList = browserData?.browsers ?? [];
  const firstRunShown = useRef(false);
  useEffect(() => {
    if (!settingsLoaded || !browserData || firstRunShown.current || settings.browser_setup_done) return;
    firstRunShown.current = true;
    const installed = browserList.filter((b) => b.installed);
    // Right after installing: offer to connect the browsers found, once.
    if (installed.length > 0 && !browserList.some((b) => b.ever_connected)) setBrowserSetupOpen(true);
    changeSettings({ browser_setup_done: true });
  }, [settingsLoaded, browserData, browserList, settings.browser_setup_done, changeSettings]);

  /* ── filtering & sorting ─────────────────────────── */
  const [status, setStatus] = useState<StatusFilter>('all');
  const [category, setCategory] = useState<FileCategory>('all');
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState<SortState>({ key: 'added', dir: 'desc' });

  const statusCounts = useMemo(() => {
    const counts = Object.fromEntries(STATUS_FILTERS.map((f) => [f.id, 0])) as Record<StatusFilter, number>;
    for (const d of downloads) for (const f of STATUS_FILTERS) if (matchesStatus(d.status, f.id)) counts[f.id]++;
    return counts;
  }, [downloads]);

  const categoryCounts = useMemo(() => {
    const counts: Record<string, number> = { all: 0 };
    for (const d of downloads) {
      if (!matchesStatus(d.status, status)) continue;
      counts.all++;
      const id = categoryMeta(d.category).id;
      counts[id] = (counts[id] ?? 0) + 1;
    }
    return counts;
  }, [downloads, status]);

  const visible = useMemo(() => {
    const q = search.trim().toLowerCase();
    const list = downloads.filter(
      (d) =>
        matchesStatus(d.status, status) &&
        (queueFilter === 'all' || (d.queue_id ?? 'main') === queueFilter) &&
        (category === 'all' || categoryMeta(d.category).id === category) &&
        (!q || d.filename.toLowerCase().includes(q) || d.url.toLowerCase().includes(q)),
    );
    const dir = sort.dir === 'asc' ? 1 : -1;
    const val = (d: DownloadTask): number | string => {
      switch (sort.key) {
        case 'name':
          return d.filename.toLowerCase();
        case 'size':
          return d.total_bytes;
        case 'progress':
          return d.status === 'completed' ? 1 : progressOf(d);
        case 'speed':
          return d.status === 'downloading' ? d.speed_bps : -1;
        case 'eta':
          return d.status === 'downloading' && d.eta_seconds ? d.eta_seconds : Number.MAX_SAFE_INTEGER * (dir === 1 ? 1 : -1);
        default:
          return d.created_at;
      }
    };
    return list.sort((a, b) => {
      const va = val(a);
      const vb = val(b);
      if (typeof va === 'string' && typeof vb === 'string') return va.localeCompare(vb) * dir;
      return ((va as number) - (vb as number)) * dir || b.created_at - a.created_at;
    });
  }, [downloads, status, category, queueFilter, search, sort]);

  const onSort = useCallback(
    (key: SortKey) =>
      setSort((prev) => (prev.key === key ? { key, dir: prev.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: key === 'name' || key === 'eta' ? 'asc' : 'desc' })),
    [],
  );

  /* ── selection, focus, expansion ─────────────────── */
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  const anchorRef = useRef<string | null>(null);

  // Drop selection entries that no longer exist
  useEffect(() => {
    const ids = new Set(downloads.map((d) => d.id));
    setSelection((prev) => {
      const next = new Set([...prev].filter((id) => ids.has(id)));
      return next.size === prev.size ? prev : next;
    });
    if (expandedId && !ids.has(expandedId)) setExpandedId(null);
  }, [downloads, expandedId]);

  const selectAll = useCallback((select: boolean) => {
    setSelection(select ? new Set(visibleRef.current.map((t) => t.id)) : new Set());
  }, []);

  /* ── shared task handlers (stable across renders) ── */
  const handlers: TaskHandlers = useMemo(
    () => ({
      pause: (id) => actions.pause([id]),
      resume: (id) => actions.resume([id]),
      retry: (id) => actions.retry([id]),
      open: (id) => actions.open(id),
      reveal: (id) => actions.reveal(id),
      copyLink: (task) =>
        navigator.clipboard.writeText(task.url).then(() => toast.show({ id: 'copied', tone: 'success', title: 'Link copied', duration: 1800 })),
      copyCurl: async (task) => {
        let cmd: string;
        try {
          cmd = await api.getCurlCommand(task.id);
        } catch {
          cmd = `curl -L -C - -o "${task.filename}" "${task.url}"`;
        }
        await navigator.clipboard.writeText(cmd);
        toast.show({ id: 'copied', tone: 'success', title: 'cURL command copied', duration: 1800 });
      },
      stopSeeding: (id) => actions.stopSeeding(id).catch((err) => toast.show({ tone: 'error', title: "Couldn't stop sharing", description: errText(err) })),
      startSeeding: (id) => actions.startSeeding(id).catch((err) => toast.show({ tone: 'error', title: "Couldn't start sharing", description: errText(err) })),
      requestRemove: (ids) => setRemoveIds(ids),
      toggleExpand: (id) => setExpandedId((cur) => (cur === id ? null : id)),
      toggleSelect: (id, range) => {
        setSelection((prev) => {
          const next = new Set(prev);
          const list = visibleRef.current;
          if (range && anchorRef.current) {
            const a = list.findIndex((t) => t.id === anchorRef.current);
            const b = list.findIndex((t) => t.id === id);
            if (a >= 0 && b >= 0) {
              for (let i = Math.min(a, b); i <= Math.max(a, b); i++) next.add(list[i].id);
              return next;
            }
          }
          if (next.has(id)) next.delete(id);
          else next.add(id);
          anchorRef.current = id;
          return next;
        });
      },
      moveToQueue: (ids, queueId) => {
        api
          .moveToQueue(ids, queueId)
          .then(() => {
            refreshQueues();
            actions.refresh();
            toast.show({ tone: 'success', title: ids.length > 1 ? `Moved ${ids.length} downloads` : 'Moved to queue', duration: 1800 });
          })
          .catch((err) => toast.show({ tone: 'error', title: "Couldn't move the download", description: errText(err) }));
      },
      focus: (id) => setFocusedId(id),
      contextMenu: (task, x, y) => setMenu({ id: task.id, x, y }),
    }),
    [actions, toast, refreshQueues],
  );

  const menuTask = menu ? downloads.find((d) => d.id === menu.id) : undefined;

  /* ── bulk actions ────────────────────────────────── */
  const selectedTasks = useMemo(() => downloads.filter((d) => selection.has(d.id)), [downloads, selection]);
  const bulkCan = useMemo(
    () => ({
      pause: selectedTasks.some(canPause),
      resume: selectedTasks.some(canResume),
      retry: selectedTasks.some(canRetry),
    }),
    [selectedTasks],
  );

  const activeIds = downloads.filter(canPause).map((d) => d.id);
  const pausedIds = downloads.filter((d) => d.status === 'paused').map((d) => d.id);

  /* ── routing pasted / dropped / typed text ───────── */
  const routeText = useCallback(
    (raw: string) => {
      const text = raw.trim();
      if (!text) return;
      if (looksLikeCurl(text)) return openAdd(text);
      if (isMagnet(text) || isTorrentUrl(text)) return openTorrent(text);
      const urls = extractUrls(text);
      if (urls.length > 1) return openMultiple('paste', urls.join('\n'));
      if (urls.length === 1) return openAdd(urls[0]);
      if (/^[\w-]+(\.[\w-]+)+(\/\S*)?$/.test(text)) return openAdd(`https://${text}`);
      toast.show({ id: 'not-a-link', tone: 'error', title: 'That doesn’t look like a link', description: 'Links start with http:// or https://' });
    },
    [openAdd, openMultiple, openTorrent, toast],
  );

  const addOne = useCallback(
    async (req: AddDownloadRequest) => {
      const task = await actions.add(req);
      toast.show({
        id: `added-${task.id}`,
        tone: 'success',
        title: req.scheduled_time ? 'Download scheduled' : 'Download added',
        description: task.filename,
      });
      setStatus((s) => (s === 'all' || matchesStatus(task.status, s) ? s : 'all'));
    },
    [actions, toast],
  );

  const addTorrent = useCallback(
    async (req: AddTorrentRequest) => {
      const task = await actions.addTorrent(req);
      toast.show({ id: `added-${task.id}`, tone: 'success', title: 'Torrent added', description: task.filename });
      setStatus('all');
    },
    [actions, toast],
  );

  const addMany = useCallback(
    async (reqs: AddDownloadRequest[]) => {
      const created = await actions.addMany(reqs);
      toast.show({ tone: 'success', title: `Added ${plural(created.length, 'download')}` });
      setStatus('all');
    },
    [actions, toast],
  );

  // The browser extension found a streaming video: let the user pick its quality.
  useEffect(() => {
    if (!isTauri()) return;
    const un = listen<{ url: string; referer?: string | null; user_agent?: string | null }>('stream-requested', (e) =>
      openAdd(e.payload.url, { referer: e.payload.referer ?? undefined, userAgent: e.payload.user_agent ?? undefined }),
    );
    // A link the browser handed over: show the download window, filled in like the browser had it.
    const un2 = listen<{
      url: string;
      filename?: string | null;
      referer?: string | null;
      user_agent?: string | null;
      headers?: Record<string, string> | null;
      expected_checksum?: string | null;
    }>('download-requested', (e) =>
      openAdd(e.payload.url, {
        filename: e.payload.filename ?? undefined,
        referer: e.payload.referer ?? undefined,
        userAgent: e.payload.user_agent ?? undefined,
        headers: e.payload.headers ?? undefined,
        checksum: e.payload.expected_checksum ?? undefined,
      }),
    );
    // Several links from the browser (a selection, or a whole page): list them to choose from.
    const un3 = listen<{
      links: GrabbedLink[];
      referer?: string | null;
      user_agent?: string | null;
      headers?: Record<string, string> | null;
      preselect?: 'all' | 'files';
    }>('links-requested', (e) =>
      setMultiState({
        open: true,
        tab: 'page',
        text: '',
        links: e.payload.links,
        preselect: e.payload.preselect,
        request: { referer: e.payload.referer ?? undefined, userAgent: e.payload.user_agent ?? undefined, headers: e.payload.headers ?? undefined },
      }),
    );
    return () => {
      un.then((f) => f()).catch(() => {});
      un2.then((f) => f()).catch(() => {});
      un3.then((f) => f()).catch(() => {});
    };
  }, [openAdd]);

  // The tray's "Settings" entry
  useEffect(() => {
    if (!isTauri()) return;
    const un = listen('open-settings', () => setSettingsView((v) => ({ ...v, open: true })));
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  /* ── global keyboard, paste, clipboard, drag & drop ─ */
  const omniboxRef = useRef<HTMLInputElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (hasOpenLayer()) return;
      const mod = e.ctrlKey || e.metaKey;
      const typing = (e.target as HTMLElement).closest('input, textarea, select, [contenteditable="true"]');
      const k = e.key.toLowerCase();
      if (mod && k === 'n') {
        e.preventDefault();
        openAdd();
      } else if (mod && k === 'q') {
        e.preventDefault();
        api.quitApp();
      } else if (mod && k === 't') {
        e.preventDefault();
        openTorrent();
      } else if (mod && k === 'b') {
        e.preventDefault();
        openMultiple();
      } else if (mod && e.key === ',') {
        e.preventDefault();
        setSettingsView((v) => ({ ...v, open: !v.open }));
      } else if (mod && k === 'l') {
        e.preventDefault();
        omniboxRef.current?.focus();
      } else if (mod && k === 'f') {
        e.preventDefault();
        setSettingsView((v) => ({ ...v, open: false }));
        requestAnimationFrame(() => searchRef.current?.focus());
      } else if (e.key === 'Escape' && !typing) {
        if (settingsView.open) setSettingsView((v) => ({ ...v, open: false }));
        else if (selection.size) setSelection(new Set());
        else if (expandedId) setExpandedId(null);
      }
    };
    const onPaste = (e: ClipboardEvent) => {
      if (hasOpenLayer()) return;
      const target = e.target as HTMLElement;
      if (target.closest('input, textarea, [contenteditable="true"]')) return;
      const text = e.clipboardData?.getData('text') ?? '';
      if (text.trim()) {
        e.preventDefault();
        routeText(text);
      }
    };
    window.addEventListener('keydown', onKey);
    document.addEventListener('paste', onPaste);
    return () => {
      window.removeEventListener('keydown', onKey);
      document.removeEventListener('paste', onPaste);
    };
  }, [openAdd, openMultiple, openTorrent, routeText, settingsView.open, selection.size, expandedId]);

  // Right-click: never show the browser's menu. Text fields keep the native
  // cut/copy/paste menu; selected text gets Copy; empty space gets the quick actions.
  useEffect(() => {
    const onContext = (e: MouseEvent) => {
      if (e.defaultPrevented) return; // a row (or other component) already handled it
      const target = e.target as HTMLElement;
      if (target.closest('input, textarea, [contenteditable="true"]')) return;
      e.preventDefault();
      if (hasOpenLayer()) return;
      if (target.closest('header')) return; // title bar: nothing to offer
      const selection = window.getSelection()?.toString().trim() ?? '';
      setAppMenu({ x: e.clientX, y: e.clientY, selection });
    };
    document.addEventListener('contextmenu', onContext);
    return () => document.removeEventListener('contextmenu', onContext);
  }, []);

  const appMenuItems: MenuItem[] = useMemo(() => {
    const sel = appMenu?.selection ?? '';
    if (sel) {
      return [{ label: 'Copy', icon: Copy, shortcut: 'Ctrl+C', onSelect: () => void navigator.clipboard.writeText(sel) }];
    }
    return [
      { label: 'Add download…', icon: Link2, shortcut: 'Ctrl+N', onSelect: () => openAdd() },
      { label: 'Add torrent…', icon: Magnet, shortcut: 'Ctrl+T', onSelect: () => openTorrent() },
      { label: 'Add multiple…', icon: ListPlus, shortcut: 'Ctrl+B', onSelect: () => openMultiple() },
      {
        label: 'Paste link',
        icon: ClipboardPaste,
        onSelect: () =>
          navigator.clipboard
            .readText()
            .then((t) => (t.trim() ? routeText(t) : toast.show({ tone: 'info', title: 'Clipboard is empty', duration: 1800 })))
            .catch(() => toast.show({ tone: 'error', title: "Couldn't read the clipboard" })),
      },
      { separator: true },
      { label: 'Select all', icon: CheckSquare, shortcut: 'Ctrl+A', onSelect: () => selectAll(true) },
      { label: 'Settings', icon: SettingsIcon, shortcut: 'Ctrl+,', onSelect: () => setSettingsView((v) => ({ ...v, open: true })) },
    ];
  }, [appMenu, openAdd, openTorrent, openMultiple, routeText, selectAll, toast]);

  // Offer copied file links when the window regains focus
  const lastClip = useRef('');
  const downloadsRef = useRef(downloads);
  downloadsRef.current = downloads;
  useEffect(() => {
    if (!settings.clipboard_watch) return;
    const check = async () => {
      try {
        const text = (await navigator.clipboard.readText()).trim();
        if (!text || text === lastClip.current) return;
        lastClip.current = text;
        const [url] = extractUrls(text);
        if (!url || url !== text || !FILE_LINK_RE.test(url) || downloadsRef.current.some((d) => d.url === url)) return;
        toast.show({
          id: 'clipboard',
          title: 'Link copied to clipboard',
          description: url,
          duration: 12000,
          icon: <Download size={17} className="text-accent" />,
          action: { label: 'Download', onClick: () => openAdd(url) },
        });
      } catch {
        /* clipboard not available */
      }
    };
    window.addEventListener('focus', check);
    return () => window.removeEventListener('focus', check);
  }, [settings.clipboard_watch, toast, openAdd]);

  const dragDepth = useRef(0);
  const isLinkDrag = (e: React.DragEvent) => {
    const types = Array.from(e.dataTransfer.types);
    return types.includes('Files') || types.includes('text/uri-list') || types.includes('text/plain');
  };

  /* ── render ──────────────────────────────────────── */
  const totalSpeed = useMemo(() => downloads.reduce((a, d) => a + (d.status === 'downloading' ? d.speed_bps || 0 : 0), 0), [downloads]);
  const { history, peak } = useSpeedHistory(totalSpeed);
  const downloadingCount = downloads.filter((d) => d.status === 'downloading').length;
  const queuedCount = downloads.filter((d) => d.status === 'queued').length;
  const filtersActive = status !== 'all' || category !== 'all' || search.trim() !== '';

  return (
    <QueuesContext.Provider value={queues}>
          <div
      className="flex h-full flex-col bg-canvas text-fg"
      onDragEnter={(e) => {
        if (!isLinkDrag(e)) return;
        dragDepth.current++;
        setDragging(true);
      }}
      onDragOver={(e) => {
        if (isLinkDrag(e)) {
          e.preventDefault();
          e.dataTransfer.dropEffect = 'copy';
        }
      }}
      onDragLeave={() => {
        dragDepth.current = Math.max(0, dragDepth.current - 1);
        if (dragDepth.current === 0) setDragging(false);
      }}
      onDrop={(e) => {
        dragDepth.current = 0;
        setDragging(false);
        const files = Array.from(e.dataTransfer.files ?? []);
        if (files.length > 0) {
          // Dropping a file onto the window would otherwise navigate the webview to it.
          e.preventDefault();
          const torrent = files.find((f) => f.name.toLowerCase().endsWith('.torrent'));
          if (torrent) openTorrent('', torrent);
          else toast.show({ id: 'drop-file', tone: 'error', title: 'Only .torrent files can be dropped here', duration: 3000 });
          return;
        }
        const text = e.dataTransfer.getData('text/uri-list') || e.dataTransfer.getData('text/plain');
        if (text) {
          e.preventDefault();
          routeText(text.split('\n').filter((l) => !l.startsWith('#')).join('\n'));
        }
      }}
    >
      <TitleBar
        omniboxRef={omniboxRef}
        onSubmitLink={routeText}
        onAddMultiple={() => openMultiple()}
        onAddTorrent={() => openTorrent()}
        onStats={() => setStatsOpen(true)}
        onSettings={() => setSettingsView((v) => ({ ...v, open: !v.open }))}
        onPauseAll={() => actions.pause(activeIds)}
        onResumeAll={() => actions.resume(pausedIds)}
        canPauseAll={activeIds.length > 0}
        canResumeAll={pausedIds.length > 0}
        settingsOpen={settingsView.open}
      />

      <main className="relative flex min-h-0 flex-1 flex-col">
        {loaded && downloads.length === 0 ? (
          <EmptyState variant="empty" onAdd={() => openAdd()} onAddMultiple={() => openMultiple()} onClearFilters={() => {}} />
        ) : (
          <>
            <FilterBar
              status={status}
              onStatus={(s) => {
                setStatus(s);
                setSelection(new Set());
              }}
              statusCounts={statusCounts}
              category={category}
              onCategory={setCategory}
              categoryCounts={categoryCounts}
              search={search}
              onSearch={setSearch}
              searchRef={searchRef}
              selectionCount={selection.size}
              onClearSelection={() => setSelection(new Set())}
              onBulkPause={() => actions.pause(selectedTasks.filter(canPause).map((t) => t.id))}
              onBulkResume={() => actions.resume(selectedTasks.filter(canResume).map((t) => t.id))}
              onBulkRetry={() => actions.retry(selectedTasks.filter(canRetry).map((t) => t.id))}
              onBulkRemove={() => setRemoveIds([...selection])}
              bulkCan={bulkCan}
              queues={queues}
              queue={queueFilter}
              onQueue={setQueueFilter}
              onBulkMove={(queueId) => {
                handlers.moveToQueue([...selection], queueId);
                setSelection(new Set());
              }}
            />
            {visible.length === 0 && loaded ? (
              <EmptyState
                variant="filtered"
                onAdd={() => openAdd()}
                onAddMultiple={() => openMultiple()}
                onClearFilters={() => {
                  setStatus('all');
                  setCategory('all');
                  setSearch('');
                }}
                filterLabel={filtersActive ? undefined : 'No downloads yet.'}
              />
            ) : (
              <TransferList
                tasks={visible}
                sort={sort}
                onSort={onSort}
                expandedId={expandedId}
                focusedId={focusedId}
                selection={selection}
                onSelectAll={selectAll}
                handlers={handlers}
              />
            )}
          </>
        )}

        <AnimatePresence>
          {settingsView.open && (
            <SettingsView
              settings={settings}
              onChange={changeSettings}
              onClose={() => setSettingsView((v) => ({ ...v, open: false }))}
              section={settingsView.section}
              onSection={(section) => setSettingsView({ open: true, section })}
              saveState={saveState}
              queues={queues}
              onQueuesChanged={refreshQueues}
              browsers={browserList}
              onSetupBrowsers={() => setBrowserSetupOpen(true)}
            />
          )}
        </AnimatePresence>

        <AnimatePresence>
          {dragging && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.12 }}
              className="pointer-events-none absolute inset-3 z-30 flex flex-col items-center justify-center rounded-xl border-2 border-dashed border-accent bg-accent-soft"
            >
              <Download size={28} className="text-accent" />
              <div className="mt-2 text-[14px] font-medium text-fg">Drop to download</div>
            </motion.div>
          )}
        </AnimatePresence>
      </main>

      <StatusBar
        speed={totalSpeed}
        history={history}
        downloading={downloadingCount}
        queued={queuedCount}
        settings={settings}
        onChangeSettings={changeSettings}
        onOpenSettings={(section) => setSettingsView({ open: true, section: section as SettingsSection })}
        queues={queues}
        browsers={browserList}
        onSetupBrowsers={() => setBrowserSetupOpen(true)}
      />

      {/* Overlays */}
      <Menu
        position={menu && menuTask ? { x: menu.x, y: menu.y } : null}
        onClose={() => setMenu(null)}
        items={menuTask ? buildTaskMenu(menuTask, handlers, expandedId === menuTask.id, queues) : []}
        minWidth={210}
      />
      <Menu position={appMenu ? { x: appMenu.x, y: appMenu.y } : null} onClose={() => setAppMenu(null)} items={appMenuItems} minWidth={190} />
      <AddDownloadDialog
        open={addState.open}
        initialText={addState.text}
        settings={settings}
        onClose={() => setAddState((s) => ({ ...s, open: false }))}
        onSubmit={addOne}
        queues={queues}
        defaultQueue={queueFilter === 'all' ? undefined : queueFilter}
        initialRequest={addState.request}
      />
      <AddTorrentDialog
        open={torrentState.open}
        initialSource={torrentState.source}
        initialFile={torrentState.file}
        settings={settings}
        onClose={() => setTorrentState((s) => ({ ...s, open: false }))}
        onSubmit={addTorrent}
      />
      <AddMultipleDialog
        open={multiState.open}
        initialTab={multiState.tab}
        initialText={multiState.text}
        initialLinks={multiState.links}
        preselect={multiState.preselect}
        initialRequest={multiState.request}
        settings={settings}
        onClose={() => setMultiState((s) => ({ ...s, open: false }))}
        onSubmit={addMany}
      />
      <RemoveDialog
        tasks={downloads.filter((d) => removeIds.includes(d.id))}
        onClose={() => setRemoveIds([])}
        onConfirm={async (deleteFiles) => {
          await actions.remove(removeIds, deleteFiles);
          setSelection(new Set());
        }}
      />
      <BrowserSetupDialog open={browserSetupOpen} onClose={() => setBrowserSetupOpen(false)} integration={browserData} />
      <StatsDialog open={statsOpen} onClose={() => setStatsOpen(false)} downloads={downloads} history={history} peak={peak} speed={totalSpeed} />
      </div>
    </QueuesContext.Provider>
  );
}
