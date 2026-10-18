export function formatBytes(bytes: number, decimals = 1): string {
  if (!bytes || bytes <= 0 || isNaN(bytes)) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  const value = bytes / Math.pow(k, i);
  const dm = i === 0 ? 0 : value >= 100 ? 0 : decimals;
  return `${value.toFixed(dm)} ${sizes[i]}`;
}

export function formatSpeed(bps: number): string {
  if (!bps || bps <= 0) return '0 B/s';
  return `${formatBytes(bps, 1)}/s`;
}

export function formatDuration(seconds?: number): string {
  if (seconds === undefined || seconds === null || seconds <= 0 || !isFinite(seconds)) {
    return '—';
  }
  const s = Math.floor(seconds);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const remS = s % 60;
  if (m < 60) return remS ? `${m}m ${remS}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const remM = m % 60;
  if (h < 24) return remM ? `${h}h ${remM}m` : `${h}h`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

/** Backend timestamps are seconds, browser mocks use milliseconds. */
export function toMs(timestamp?: number): number {
  if (!timestamp) return 0;
  return timestamp > 10_000_000_000 ? timestamp : timestamp * 1000;
}

export function formatDate(timestamp?: number): string {
  if (!timestamp) return '';
  return new Date(toMs(timestamp)).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function formatRelative(timestamp?: number): string {
  if (!timestamp) return '—';
  const diff = Date.now() - toMs(timestamp);
  const abs = Math.abs(diff);
  const future = diff < 0;
  const units: [number, string][] = [
    [86_400_000 * 7, 'w'],
    [86_400_000, 'd'],
    [3_600_000, 'h'],
    [60_000, 'm'],
  ];
  if (abs < 45_000) return future ? 'in a moment' : 'just now';
  for (const [ms, label] of units) {
    if (abs >= ms) {
      const n = Math.floor(abs / ms);
      if (label === 'w' && n > 4) return formatDate(timestamp);
      return future ? `in ${n}${label}` : `${n}${label} ago`;
    }
  }
  return formatDate(timestamp);
}

export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

export function extOf(filename: string): string {
  const m = /\.([a-z0-9]{1,6})$/i.exec(filename);
  return m ? m[1].toLowerCase() : '';
}

export function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  const sep = dir.includes('\\') ? '\\' : '/';
  return dir.endsWith('/') || dir.endsWith('\\') ? `${dir}${name}` : `${dir}${sep}${name}`;
}

export function plural(n: number, word: string, pluralWord = `${word}s`): string {
  return `${n} ${n === 1 ? word : pluralWord}`;
}
