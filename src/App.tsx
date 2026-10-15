import React, { useState, useEffect, useMemo, useCallback } from 'react';
import { api } from './services/api';
import {
  AddDownloadRequest,
  AppSettings,
  DownloadProgressEvent,
  DownloadTask,
  FileCategory,
} from './types/download';
import { Sidebar } from './components/layout/Sidebar';
import { Header } from './components/layout/Header';
import { DownloadCard } from './components/downloads/DownloadCard';
import { DownloadRow } from './components/downloads/DownloadRow';
import { EmptyState } from './components/downloads/EmptyState';
import { NewDownloadModal } from './components/modals/NewDownloadModal';
import { BatchDownloadModal } from './components/modals/BatchDownloadModal';
import { LinkGrabberModal } from './components/modals/LinkGrabberModal';
import { SettingsModal } from './components/modals/SettingsModal';
import { DownloadInspector } from './components/downloads/DownloadInspector';
import { AnalyticsView } from './components/analytics/AnalyticsView';
import { Zap, X } from 'lucide-react';
import { isTauri } from './services/api';
import { listen } from '@tauri-apps/api/event';

export default function App() {
  const [downloads, setDownloads] = useState<DownloadTask[]>([]);
  const [settings, setSettings] = useState<AppSettings>({
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
    auto_categorize: false,
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
  });

  const [currentTab, setCurrentTab] = useState<string>('all');
  const [selectedCategory, setSelectedCategory] = useState<FileCategory>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [viewMode, setViewMode] = useState<'grid' | 'list'>('grid');
  const [sortBy, setSortBy] = useState<'date' | 'size' | 'name' | 'speed'>('date');

  // Modal and drawer states
  const [isNewModalOpen, setIsNewModalOpen] = useState(false);
  const [isBatchModalOpen, setIsBatchModalOpen] = useState(false);
  const [isLinkGrabberModalOpen, setIsLinkGrabberModalOpen] = useState(false);
  const [isSettingsModalOpen, setIsSettingsModalOpen] = useState(false);
  const [newModalInitialUrl, setNewModalInitialUrl] = useState('');
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [clipboardDetectedUrl, setClipboardDetectedUrl] = useState<string | null>(null);

  // Initial load
  const loadInitialData = useCallback(async () => {
    try {
      const [allDownloads, currentSettings] = await Promise.all([
        api.getAllDownloads(),
        api.getSettings(),
      ]);
      setDownloads(allDownloads);
      setSettings(currentSettings);
    } catch (err) {
      console.error('Failed to load initial data:', err);
    }
  }, []);

  useEffect(() => {
    loadInitialData();
  }, [loadInitialData]);

  // Global Keyboard Shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
        if (e.key === 'Escape') {
          target.blur();
        }
        return;
      }

      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        setNewModalInitialUrl('');
        setIsNewModalOpen(true);
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'b') {
        e.preventDefault();
        setIsBatchModalOpen(true);
      } else if ((e.ctrlKey || e.metaKey) && e.key === ',') {
        e.preventDefault();
        setIsSettingsModalOpen(true);
      } else if (e.key === 'Escape') {
        setIsNewModalOpen(false);
        setIsBatchModalOpen(false);
        setIsSettingsModalOpen(false);
        setSelectedTaskId(null);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Clipboard URL Detection
  useEffect(() => {
    if (!settings.clipboard_watch) return;

    const checkClipboard = async () => {
      try {
        const text = await navigator.clipboard.readText();
        const trimmed = text.trim();
        if (
          (trimmed.startsWith('http://') || trimmed.startsWith('https://')) &&
          trimmed !== clipboardDetectedUrl &&
          !downloads.some((d) => d.url === trimmed)
        ) {
          const match = /\.(zip|rar|7z|tar|gz|iso|exe|msi|dmg|mp4|mkv|mp3|pdf|bin)(\?|$)/i.test(trimmed);
          if (match) {
            setClipboardDetectedUrl(trimmed);
          }
        }
      } catch {
        // clipboard access not granted
      }
    };

    window.addEventListener('focus', checkClipboard);
    return () => window.removeEventListener('focus', checkClipboard);
  }, [settings.clipboard_watch, clipboardDetectedUrl, downloads]);

  // Event listeners for Tauri backend
  useEffect(() => {
    let unlistenProgress: (() => void) | undefined;
    let unlistenCompleted: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;

    api.onProgress((event: DownloadProgressEvent) => {
      setDownloads((prev) =>
        prev.map((item) => {
          if (item.id === event.id) {
            return {
              ...item,
              downloaded_bytes: event.downloaded_bytes,
              speed_bps: event.speed_bps,
              eta_seconds: event.eta_seconds,
              status: event.status,
              segments: event.segments?.length ? event.segments : item.segments,
              updated_at: Date.now(),
            };
          }
          return item;
        })
      );
    }).then((unlisten) => {
      unlistenProgress = unlisten;
    });

    api.onCompleted((completedTask: DownloadTask) => {
      setDownloads((prev) =>
        prev.map((item) => (item.id === completedTask.id ? completedTask : item))
      );
    }).then((unlisten) => {
      unlistenCompleted = unlisten;
    });

    api.onStatusChanged(() => {
      api.getAllDownloads().then((tasks) => setDownloads(tasks));
    }).then((unlisten) => {
      unlistenStatus = unlisten;
    });

    let unlistenOpenNew: (() => void) | undefined;
    let unlistenIntercepted: (() => void) | undefined;

    if (isTauri()) {
      listen('open-new-download', () => {
        setIsNewModalOpen(true);
      }).then((unlisten) => {
        unlistenOpenNew = unlisten;
      });

      listen('download-intercepted', () => {
        api.getAllDownloads().then((tasks) => setDownloads(tasks));
      }).then((unlisten) => {
        unlistenIntercepted = unlisten;
      });
    }

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenCompleted) unlistenCompleted();
      if (unlistenStatus) unlistenStatus();
      if (unlistenOpenNew) unlistenOpenNew();
      if (unlistenIntercepted) unlistenIntercepted();
    };
  }, []);

  // Action Handlers
  const handleAddDownload = async (req: AddDownloadRequest) => {
    try {
      const created = await api.addDownload(req);
      setDownloads((prev) => [created, ...prev]);
    } catch (err) {
      console.error('Failed to add download:', err);
      alert(`Download failed: ${err}`);
    }
  };

  const handleBatchAdd = async (reqs: AddDownloadRequest[]) => {
    try {
      const createdList = await api.batchAdd(reqs);
      setDownloads((prev) => [...createdList, ...prev]);
    } catch (err) {
      console.error('Failed to batch add downloads:', err);
    }
  };

  const handlePause = async (id: string) => {
    try {
      await api.pauseDownload(id);
      setDownloads((prev) =>
        prev.map((d) => (d.id === id ? { ...d, status: 'paused', speed_bps: 0 } : d))
      );
    } catch (err) {
      console.error(err);
    }
  };

  const handleResume = async (id: string) => {
    try {
      await api.resumeDownload(id);
      setDownloads((prev) =>
        prev.map((d) => (d.id === id ? { ...d, status: 'downloading' } : d))
      );
    } catch (err) {
      console.error(err);
    }
  };

  const handleRetry = async (id: string) => {
    try {
      await api.retryDownload(id);
      setDownloads((prev) =>
        prev.map((d) => (d.id === id ? { ...d, status: 'downloading', error_message: undefined } : d))
      );
    } catch (err) {
      console.error(err);
    }
  };

  const handleDelete = async (id: string, deleteFile = false) => {
    try {
      await api.deleteDownload(id, deleteFile);
      setDownloads((prev) => prev.filter((d) => d.id !== id));
      if (selectedTaskId === id) {
        setSelectedTaskId(null);
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handlePauseAll = async () => {
    const downloading = downloads.filter((d) => d.status === 'downloading');
    for (const d of downloading) {
      await handlePause(d.id);
    }
  };

  const handleResumeAll = async () => {
    const paused = downloads.filter((d) => d.status === 'paused');
    for (const d of paused) {
      await handleResume(d.id);
    }
  };

  const handleOpen = async (id: string) => {
    try {
      await api.openFile(id);
    } catch (err) {
      console.error(err);
    }
  };

  const handleReveal = async (id: string) => {
    try {
      await api.revealFile(id);
    } catch (err) {
      console.error(err);
    }
  };

  const handleSaveSettings = async (newSettings: AppSettings) => {
    try {
      await api.updateSettings(newSettings);
      setSettings(newSettings);
    } catch (err) {
      console.error('Failed to update settings:', err);
    }
  };

  // Filtered & Sorted downloads
  const filteredDownloads = useMemo(() => {
    return downloads
      .filter((task) => {
        // Tab filter
        if (currentTab !== 'all' && task.status !== currentTab) {
          return false;
        }

        // Category filter
        if (selectedCategory !== 'all' && task.category !== selectedCategory) {
          return false;
        }

        // Search query
        if (searchQuery.trim()) {
          const query = searchQuery.toLowerCase();
          const matchName = task.filename.toLowerCase().includes(query);
          const matchUrl = task.url.toLowerCase().includes(query);
          if (!matchName && !matchUrl) return false;
        }

        return true;
      })
      .sort((a, b) => {
        if (sortBy === 'date') {
          return b.created_at - a.created_at;
        }
        if (sortBy === 'size') {
          return b.total_bytes - a.total_bytes;
        }
        if (sortBy === 'name') {
          return a.filename.localeCompare(b.filename);
        }
        if (sortBy === 'speed') {
          return (b.speed_bps || 0) - (a.speed_bps || 0);
        }
        return 0;
      });
  }, [downloads, currentTab, selectedCategory, searchQuery, sortBy]);

  // Aggregate download speed across all downloading tasks
  const aggregateSpeed = useMemo(() => {
    return downloads
      .filter((d) => d.status === 'downloading')
      .reduce((acc, curr) => acc + (curr.speed_bps || 0), 0);
  }, [downloads]);

  // Sidebar count indicators
  const counts = useMemo(() => {
    return {
      all: downloads.length,
      downloading: downloads.filter((d) => d.status === 'downloading').length,
      completed: downloads.filter((d) => d.status === 'completed').length,
      paused: downloads.filter((d) => d.status === 'paused').length,
      scheduled: downloads.filter((d) => d.status === 'scheduled').length,
      failed: downloads.filter((d) => d.status === 'failed').length,
    };
  }, [downloads]);

  const selectedTask = downloads.find((d) => d.id === selectedTaskId) || null;

  return (
    <div className="flex h-screen w-screen bg-[#0c0d12] text-slate-100 font-sans antialiased overflow-hidden">
      {/* Sidebar Navigation */}
      <Sidebar
        currentTab={currentTab}
        onSelectTab={setCurrentTab}
        selectedCategory={selectedCategory}
        onSelectCategory={setSelectedCategory}
        counts={counts}
        aggregateSpeed={aggregateSpeed}
        onOpenSettings={() => setIsSettingsModalOpen(true)}
      />

      {/* Main Content Area */}
      <main className="flex-1 flex flex-col h-full min-w-0 overflow-hidden bg-slate-950/40">
        {/* Top Header */}
        <Header
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          viewMode={viewMode}
          onViewModeChange={setViewMode}
          sortBy={sortBy}
          onSortByChange={setSortBy}
          onOpenNewDownload={() => {
            setNewModalInitialUrl('');
            setIsNewModalOpen(true);
          }}
          onOpenBatchDownload={() => setIsBatchModalOpen(true)}
          onOpenLinkGrabber={() => setIsLinkGrabberModalOpen(true)}
          onPauseAll={handlePauseAll}
          onResumeAll={handleResumeAll}
          hasActiveDownloads={counts.downloading > 0}
          hasPausedDownloads={counts.paused > 0}
        />

        {/* Main Content Area: Analytics or Downloads List */}
        {currentTab === 'analytics' ? (
          <AnalyticsView
            downloads={downloads}
            onOpenFile={handleOpen}
            onRevealFile={handleReveal}
          />
        ) : (
          <div className="flex-1 overflow-y-auto px-6 py-5">
            {filteredDownloads.length === 0 ? (
              <EmptyState
                tabName={currentTab}
                onOpenNewDownload={(initialUrl) => {
                  setNewModalInitialUrl(initialUrl || '');
                  setIsNewModalOpen(true);
                }}
              />
            ) : viewMode === 'grid' ? (
              <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
                {filteredDownloads.map((task) => (
                  <DownloadCard
                    key={task.id}
                    task={task}
                    onPause={handlePause}
                    onResume={handleResume}
                    onRetry={handleRetry}
                    onDelete={handleDelete}
                    onOpen={handleOpen}
                    onReveal={handleReveal}
                    onSelect={(t) => setSelectedTaskId(t.id)}
                  />
                ))}
              </div>
            ) : (
              <div className="rounded-2xl border border-white/5 bg-slate-900/30 overflow-hidden divide-y divide-white/5">
                {filteredDownloads.map((task) => (
                  <DownloadRow
                    key={task.id}
                    task={task}
                    onPause={handlePause}
                    onResume={handleResume}
                    onRetry={handleRetry}
                    onDelete={handleDelete}
                    onOpen={handleOpen}
                    onReveal={handleReveal}
                    onSelect={(t) => setSelectedTaskId(t.id)}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </main>

      {/* Download Inspector Drawer */}
      {selectedTask && (
        <DownloadInspector
          task={selectedTask}
          onClose={() => setSelectedTaskId(null)}
          onPause={handlePause}
          onResume={handleResume}
          onRetry={handleRetry}
          onOpenFile={handleOpen}
          onRevealFile={handleReveal}
        />
      )}

      {/* Floating Clipboard Notification Toast */}
      {clipboardDetectedUrl && (
        <div className="fixed bottom-6 right-6 z-50 p-4 bg-slate-900/95 border border-indigo-500/30 rounded-2xl shadow-2xl backdrop-blur-xl flex items-center gap-3 animate-in fade-in slide-in-from-bottom duration-200 max-w-md">
          <div className="flex h-8 w-8 items-center justify-center rounded-xl bg-indigo-500/20 text-indigo-400 shrink-0">
            <Zap size={16} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-xs font-semibold text-slate-200">Download Link Detected</div>
            <div className="text-[11px] text-slate-400 truncate font-mono">{clipboardDetectedUrl}</div>
          </div>
          <button
            onClick={() => {
              setNewModalInitialUrl(clipboardDetectedUrl);
              setClipboardDetectedUrl(null);
              setIsNewModalOpen(true);
            }}
            className="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white font-semibold text-xs rounded-lg transition-colors shrink-0"
          >
            Download
          </button>
          <button
            onClick={() => setClipboardDetectedUrl(null)}
            className="p-1 text-slate-500 hover:text-slate-300 rounded-lg hover:bg-white/5 transition-colors"
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* Modals */}
      <NewDownloadModal
        isOpen={isNewModalOpen}
        onClose={() => setIsNewModalOpen(false)}
        onAdd={handleAddDownload}
        initialUrl={newModalInitialUrl}
        defaultDestination={settings.default_destination}
      />

      <BatchDownloadModal
        isOpen={isBatchModalOpen}
        onClose={() => setIsBatchModalOpen(false)}
        onBatchAdd={handleBatchAdd}
        defaultDestination={settings.default_destination}
      />

      <LinkGrabberModal
        isOpen={isLinkGrabberModalOpen}
        onClose={() => setIsLinkGrabberModalOpen(false)}
        onBatchAdd={handleBatchAdd}
        defaultDestination={settings.default_destination}
      />

      <SettingsModal
        isOpen={isSettingsModalOpen}
        onClose={() => setIsSettingsModalOpen(false)}
        settings={settings}
        onSave={handleSaveSettings}
      />
    </div>
  );
}
