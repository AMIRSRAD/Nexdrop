import { useEffect, useState } from 'react';
import { isTauri } from '../services/api';

export type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'none'; checkedAt: number }
  | { kind: 'available'; version: string; notes?: string }
  | { kind: 'installing'; version: string; percent: number | null }
  | { kind: 'error'; message: string };

// One shared state for the whole app (the toast, the About page and the startup check).
let state: UpdateState = { kind: 'idle' };
const listeners = new Set<(s: UpdateState) => void>();
let pending: Awaited<ReturnType<typeof import('@tauri-apps/plugin-updater').check>> = null;

function set(next: UpdateState) {
  state = next;
  listeners.forEach((l) => l(state));
}

const LAST_CHECK_KEY = 'nexdrop.updateCheckedAt';

/** Asks GitHub whether a newer signed version exists. */
export async function checkForUpdate(): Promise<UpdateState> {
  if (!isTauri()) {
    set({ kind: 'none', checkedAt: Date.now() });
    return state;
  }
  if (state.kind === 'checking' || state.kind === 'installing') return state;
  set({ kind: 'checking' });
  try {
    const { check } = await import('@tauri-apps/plugin-updater');
    pending = await check({ timeout: 20000 });
    try {
      localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
    } catch {
      /* private mode */
    }
    set(pending ? { kind: 'available', version: pending.version, notes: pending.body ?? undefined } : { kind: 'none', checkedAt: Date.now() });
  } catch (err) {
    set({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
  }
  return state;
}

/** Downloads the update (its signature is checked), installs it and restarts Nexdrop. */
export async function installUpdate(): Promise<void> {
  if (!pending) return;
  const version = pending.version;
  let total = 0;
  let got = 0;
  set({ kind: 'installing', version, percent: null });
  try {
    await pending.downloadAndInstall((ev) => {
      if (ev.event === 'Started') total = ev.data.contentLength ?? 0;
      if (ev.event === 'Progress') {
        got += ev.data.chunkLength;
        set({ kind: 'installing', version, percent: total ? Math.min(100, Math.round((got / total) * 100)) : null });
      }
    });
    const { relaunch } = await import('@tauri-apps/plugin-process');
    await relaunch();
  } catch (err) {
    set({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
  }
}

/** True when the last automatic check is older than `hours`. */
export function checkIsDue(hours: number): boolean {
  try {
    const last = Number(localStorage.getItem(LAST_CHECK_KEY) || 0);
    return Date.now() - last > hours * 3600 * 1000;
  } catch {
    return true;
  }
}

export function useUpdateState(): UpdateState {
  const [s, setS] = useState(state);
  useEffect(() => {
    listeners.add(setS);
    return () => {
      listeners.delete(setS);
    };
  }, []);
  return s;
}
