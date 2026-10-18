import {
  AppWindow,
  File,
  FileArchive,
  FileText,
  Film,
  Music,
  type LucideIcon,
} from 'lucide-react';
import type { DownloadStatus, DownloadTask, FileCategory } from '../types/download';

export type ConcreteCategory = Exclude<FileCategory, 'all'>;

export const CATEGORIES: { id: ConcreteCategory; label: string; icon: LucideIcon; color: string }[] = [
  { id: 'compressed', label: 'Archives', icon: FileArchive, color: 'var(--cat-compressed)' },
  { id: 'videos', label: 'Video', icon: Film, color: 'var(--cat-videos)' },
  { id: 'music', label: 'Audio', icon: Music, color: 'var(--cat-music)' },
  { id: 'documents', label: 'Documents', icon: FileText, color: 'var(--cat-documents)' },
  { id: 'programs', label: 'Programs', icon: AppWindow, color: 'var(--cat-programs)' },
  { id: 'other', label: 'Other', icon: File, color: 'var(--cat-other)' },
];

export function categoryMeta(category: string | undefined) {
  const normalized = category === 'video' ? 'videos' : category === 'audio' ? 'music' : category;
  return CATEGORIES.find((c) => c.id === normalized) ?? CATEGORIES[CATEGORIES.length - 1];
}

/** The status filters shown above the transfer list. */
export type StatusFilter = 'all' | 'active' | 'paused' | 'completed' | 'failed' | 'scheduled';

export const STATUS_FILTERS: { id: StatusFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'active', label: 'Active' },
  { id: 'paused', label: 'Paused' },
  { id: 'completed', label: 'Completed' },
  { id: 'failed', label: 'Failed' },
  { id: 'scheduled', label: 'Scheduled' },
];

export function matchesStatus(status: DownloadStatus, filter: StatusFilter): boolean {
  switch (filter) {
    case 'all':
      return true;
    case 'active':
      return status === 'downloading' || status === 'queued';
    case 'failed':
      return status === 'failed' || status === 'canceled';
    default:
      return status === filter;
  }
}

export const STATUS_LABEL: Record<DownloadStatus, string> = {
  queued: 'Queued',
  downloading: 'Downloading',
  paused: 'Paused',
  completed: 'Completed',
  failed: 'Failed',
  scheduled: 'Scheduled',
  canceled: 'Canceled',
};

export const STATUS_COLOR: Record<DownloadStatus, string> = {
  queued: 'var(--fg-3)',
  downloading: 'var(--accent)',
  paused: 'var(--warning)',
  completed: 'var(--success)',
  failed: 'var(--danger)',
  scheduled: 'var(--violet)',
  canceled: 'var(--fg-3)',
};

export function progressOf(task: Pick<DownloadTask, 'downloaded_bytes' | 'total_bytes'>): number {
  if (!task.total_bytes || task.total_bytes <= 0) return 0;
  return Math.min(1, Math.max(0, task.downloaded_bytes / task.total_bytes));
}

export const PRIORITIES = [
  { value: 0, label: 'Normal' },
  { value: 1, label: 'High' },
  { value: 2, label: 'Urgent' },
];

export const CHUNK_SIZES = [
  { value: 262144, label: '256 KB' },
  { value: 524288, label: '512 KB' },
  { value: 1048576, label: '1 MB' },
  { value: 2097152, label: '2 MB' },
  { value: 5242880, label: '5 MB' },
  { value: 10485760, label: '10 MB' },
];

export const USER_AGENTS = [
  { value: '', label: 'Nexdrop (default)' },
  {
    value:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36',
    label: 'Chrome on Windows',
  },
  {
    value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:135.0) Gecko/20100101 Firefox/135.0',
    label: 'Firefox on Windows',
  },
  {
    value:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15',
    label: 'Safari on macOS',
  },
  { value: 'curl/8.10.1', label: 'curl' },
];

export const EXTENSION_DIR = 'd:\\Projects\\Nexdrop\\extensions\\nexdrop-browser-extension';
export const GRABBER_PORT = 18888;

const URL_RE = /https?:\/\/[^\s"'<>]+/gi;

/** Pull every distinct http(s) URL out of arbitrary text. */
export function extractUrls(text: string): string[] {
  const found = text.match(URL_RE) ?? [];
  return Array.from(new Set(found.map((u) => u.replace(/[),.;]+$/, ''))));
}

export function isHttpUrl(text: string): boolean {
  return /^https?:\/\/\S+$/i.test(text.trim());
}

export function looksLikeCurl(text: string): boolean {
  return /^\s*curl(\.exe)?\s/i.test(text);
}
