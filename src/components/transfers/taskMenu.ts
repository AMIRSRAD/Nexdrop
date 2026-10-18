import { Copy, ExternalLink, FolderOpen, Info, Pause, Play, RotateCw, Terminal, Trash } from 'lucide-react';
import type { DownloadTask } from '../../types/download';
import type { MenuItem } from '../ui/Menu';

export interface TaskHandlers {
  pause: (id: string) => void;
  resume: (id: string) => void;
  retry: (id: string) => void;
  open: (id: string) => void;
  reveal: (id: string) => void;
  copyLink: (task: DownloadTask) => void;
  copyCurl: (task: DownloadTask) => void;
  requestRemove: (ids: string[]) => void;
  toggleExpand: (id: string) => void;
  toggleSelect: (id: string, range?: boolean) => void;
  focus: (id: string) => void;
  contextMenu: (task: DownloadTask, x: number, y: number) => void;
}

export function canPause(t: DownloadTask) {
  return t.status === 'downloading' || t.status === 'queued';
}
export function canResume(t: DownloadTask) {
  return t.status === 'paused' || t.status === 'scheduled';
}
export function canRetry(t: DownloadTask) {
  return t.status === 'failed' || t.status === 'canceled';
}

export function buildTaskMenu(task: DownloadTask, h: TaskHandlers, expanded: boolean): MenuItem[] {
  const items: MenuItem[] = [];
  if (task.status === 'completed') {
    items.push(
      { label: 'Open', icon: ExternalLink, shortcut: 'Enter', onSelect: () => h.open(task.id) },
      { label: 'Show in folder', icon: FolderOpen, onSelect: () => h.reveal(task.id) },
    );
  } else {
    if (canPause(task)) items.push({ label: 'Pause', icon: Pause, shortcut: 'Space', onSelect: () => h.pause(task.id) });
    if (canResume(task))
      items.push({ label: task.status === 'scheduled' ? 'Start now' : 'Resume', icon: Play, shortcut: 'Space', onSelect: () => h.resume(task.id) });
    if (canRetry(task)) items.push({ label: 'Retry', icon: RotateCw, onSelect: () => h.retry(task.id) });
    items.push({ label: 'Show in folder', icon: FolderOpen, onSelect: () => h.reveal(task.id) });
  }
  items.push(
    { label: expanded ? 'Hide details' : 'Details', icon: Info, onSelect: () => h.toggleExpand(task.id) },
    { separator: true },
    { label: 'Copy link', icon: Copy, onSelect: () => h.copyLink(task) },
    { label: 'Copy as cURL', icon: Terminal, onSelect: () => h.copyCurl(task) },
    { separator: true },
    { label: 'Remove…', icon: Trash, shortcut: 'Del', danger: true, onSelect: () => h.requestRemove([task.id]) },
  );
  return items;
}
