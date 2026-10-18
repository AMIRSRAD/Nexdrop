import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Power } from 'lucide-react';
import { api, isTauri } from '../services/api';
import type { AddDownloadRequest, AddTorrentRequest, DownloadProgressEvent, DownloadTask, TorrentLive } from '../types/download';
import { useToast } from '../components/ui/Toast';

export interface DownloadActions {
  add: (req: AddDownloadRequest) => Promise<DownloadTask>;
  addMany: (reqs: AddDownloadRequest[]) => Promise<DownloadTask[]>;
  addTorrent: (req: AddTorrentRequest) => Promise<DownloadTask>;
  stopSeeding: (id: string) => Promise<void>;
  startSeeding: (id: string) => Promise<void>;
  pause: (ids: string[]) => Promise<void>;
  resume: (ids: string[]) => Promise<void>;
  retry: (ids: string[]) => Promise<void>;
  remove: (ids: string[], deleteFile: boolean) => Promise<void>;
  open: (id: string) => Promise<void>;
  reveal: (id: string) => Promise<void>;
  refresh: () => Promise<void>;
}

const errText = (err: unknown) => (err instanceof Error ? err.message : String(err ?? 'Unknown error'));

/**
 * Owns the download list and keeps it in sync with the engine.
 * Progress events arrive per-download every ~250ms; they're coalesced and
 * applied once per animation frame so a dozen active transfers cost a
 * single React render instead of a dozen.
 */
export function useDownloads(onOpenNewDownload: () => void, onOpenTorrent: (source: string) => void) {
  const [downloads, setDownloads] = useState<DownloadTask[]>([]);
  const [loaded, setLoaded] = useState(false);
  const toast = useToast();
  const downloadsRef = useRef(downloads);
  downloadsRef.current = downloads;
  const openNewRef = useRef(onOpenNewDownload);
  openNewRef.current = onOpenNewDownload;
  const openTorrentRef = useRef(onOpenTorrent);
  openTorrentRef.current = onOpenTorrent;

  const refresh = useCallback(async () => {
    try {
      setDownloads(await api.getAllDownloads());
    } catch (err) {
      console.error('Failed to load downloads', err);
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Engine events
  useEffect(() => {
    const pending = new Map<string, DownloadProgressEvent>();
    const pendingLive = new Map<string, TorrentLive>();
    let frame = 0;

    const flush = () => {
      frame = 0;
      if (pending.size === 0 && pendingLive.size === 0) return;
      const batch = new Map(pending);
      const liveBatch = new Map(pendingLive);
      pending.clear();
      pendingLive.clear();
      setDownloads((prev) =>
        prev.map((base) => {
          const live = liveBatch.get(base.id);
          const item = live ? { ...base, uploaded_bytes: live.uploaded_bytes, peers: live.peers, upload_bps: live.upload_bps } : base;
          const ev = batch.get(item.id);
          if (!ev) return item;
          return {
            ...item,
            downloaded_bytes: ev.downloaded_bytes,
            total_bytes: ev.total_bytes > 0 ? ev.total_bytes : item.total_bytes,
            speed_bps: ev.speed_bps,
            eta_seconds: ev.eta_seconds,
            status: ev.status,
            segments: ev.segments?.length ? ev.segments : item.segments,
          };
        }),
      );
    };

    const unlisteners: Promise<() => void>[] = [
      api.onProgress((ev) => {
        pending.set(ev.id, ev);
        if (!frame) frame = requestAnimationFrame(flush);
      }),
      api.onTorrentLive((live) => {
        pendingLive.set(live.id, live);
        if (!frame) frame = requestAnimationFrame(flush);
      }),
      api.onCompleted((task) => {
        setDownloads((prev) => prev.map((d) => (d.id === task.id ? task : d)));
        toast.show({
          id: `done-${task.id}`,
          tone: 'success',
          title: 'Download complete',
          description: task.filename,
          action: { label: 'Open', onClick: () => api.openFile(task.id).catch(() => {}) },
        });
      }),
      api.onStatusChanged(() => {
        refresh();
      }),
    ];

    if (isTauri()) {
      unlisteners.push(
        listen<{ id: string; error?: string }>('download-failed', (e) => {
          const task = downloadsRef.current.find((d) => d.id === e.payload.id);
          toast.show({
            id: `fail-${e.payload.id}`,
            tone: 'error',
            title: task ? `Failed: ${task.filename}` : 'Download failed',
            description: e.payload.error,
          });
        }),
        listen('download-deleted', () => refresh()),
        listen<{ source: string }>('torrent-requested', (e) => openTorrentRef.current(e.payload.source)),
        listen<{ action: string; seconds: number }>('completion-action', (e) => {
          const { action, seconds } = e.payload;
          toast.show({
            id: 'power-countdown',
            tone: 'info',
            icon: <Power size={17} className="text-warning" />,
            title: action === 'shutdown' ? `Shutting down in ${seconds} seconds` : `Going to sleep in ${seconds} seconds`,
            description: 'Your downloads are finished.',
            duration: seconds * 1000,
            action: { label: 'Cancel', onClick: () => api.cancelCompletionAction().catch(() => {}) },
          });
        }),
        listen('completion-action-cancelled', () => {
          toast.dismiss('power-countdown');
          toast.show({ id: 'power-cancelled', tone: 'success', title: 'Cancelled', duration: 2500 });
        }),
        listen<{ event: string; paused?: number }>('scheduler-event', (e) => {
          refresh();
          if (e.payload.event === 'started') {
            toast.show({ id: 'scheduler', title: 'Scheduler window opened', description: 'Queued downloads are starting.' });
          } else if (e.payload.event === 'stopped') {
            const n = e.payload.paused ?? 0;
            toast.show({
              id: 'scheduler',
              title: 'Scheduler window closed',
              description: n > 0 ? `${n} download${n === 1 ? '' : 's'} paused until it reopens.` : undefined,
            });
          }
        }),
        listen('open-new-download', () => openNewRef.current()),
        listen('download-intercepted', () => {
          refresh();
          toast.show({ title: 'Download captured from browser', tone: 'info' });
        }),
      );
    }

    return () => {
      if (frame) cancelAnimationFrame(frame);
      unlisteners.forEach((p) => p.then((u) => u()).catch(() => {}));
    };
  }, [refresh, toast]);

  const patch = useCallback((ids: string[], fn: (d: DownloadTask) => DownloadTask) => {
    const set = new Set(ids);
    setDownloads((prev) => prev.map((d) => (set.has(d.id) ? fn(d) : d)));
  }, []);

  const runAll = useCallback(
    async (ids: string[], op: (id: string) => Promise<void>, verb: string) => {
      const results = await Promise.allSettled(ids.map(op));
      const failed = results.filter((r) => r.status === 'rejected') as PromiseRejectedResult[];
      if (failed.length) {
        toast.show({
          tone: 'error',
          title: failed.length === 1 ? `Couldn't ${verb} download` : `Couldn't ${verb} ${failed.length} downloads`,
          description: errText(failed[0].reason),
        });
      }
    },
    [toast],
  );

  const actions: DownloadActions = useMemo(
    () => ({
      refresh,
      add: async (req) => {
        const created = await api.addDownload(req);
        setDownloads((prev) => [created, ...prev.filter((d) => d.id !== created.id)]);
        return created;
      },
      addMany: async (reqs) => {
        const created = await api.batchAdd(reqs);
        const ids = new Set(created.map((c) => c.id));
        setDownloads((prev) => [...created, ...prev.filter((d) => !ids.has(d.id))]);
        return created;
      },
      addTorrent: async (req) => {
        const created = await api.addTorrent(req);
        setDownloads((prev) => [created, ...prev.filter((d) => d.id !== created.id)]);
        return created;
      },
      stopSeeding: async (id) => {
        await api.stopSeeding(id);
        patch([id], (d) => ({ ...d, seeding: false, upload_bps: 0 }));
      },
      startSeeding: async (id) => {
        await api.startSeeding(id);
        patch([id], (d) => ({ ...d, seeding: true }));
      },
      pause: async (ids) => {
        await runAll(ids, (id) => api.pauseDownload(id), 'pause');
        patch(ids, (d) => (d.status === 'downloading' || d.status === 'queued' ? { ...d, status: 'paused', speed_bps: 0 } : d));
      },
      resume: async (ids) => {
        await runAll(ids, (id) => api.resumeDownload(id), 'resume');
        patch(ids, (d) => (d.status === 'paused' || d.status === 'scheduled' ? { ...d, status: 'queued' } : d));
        refresh();
      },
      retry: async (ids) => {
        await runAll(ids, (id) => api.retryDownload(id), 'retry');
        patch(ids, (d) => ({ ...d, status: 'queued', error_message: undefined }));
        refresh();
      },
      remove: async (ids, deleteFile) => {
        await runAll(ids, (id) => api.deleteDownload(id, deleteFile), 'remove');
        const set = new Set(ids);
        setDownloads((prev) => prev.filter((d) => !set.has(d.id)));
      },
      open: async (id) => {
        try {
          await api.openFile(id);
        } catch (err) {
          toast.show({ tone: 'error', title: "Couldn't open file", description: errText(err) });
        }
      },
      reveal: async (id) => {
        try {
          await api.revealFile(id);
        } catch (err) {
          toast.show({ tone: 'error', title: "Couldn't show in folder", description: errText(err) });
        }
      },
    }),
    [patch, refresh, runAll, toast],
  );

  return { downloads, loaded, actions };
}

export { errText };
