import { useCallback, useEffect, useState } from 'react';
import type { BrowserIntegration } from '../types/download';
import { api } from '../services/api';
import { usePageVisible } from './usePageVisible';

/**
 * Which browsers are on this computer and whether Nexdrop's extension has checked in from them.
 * Polled quickly while `watching` (a setup window or settings page is open), slowly otherwise.
 */
export function useBrowsers(watching: boolean) {
  const [data, setData] = useState<BrowserIntegration | null>(null);

  const refresh = useCallback(() => {
    api
      .getBrowsers()
      .then(setData)
      .catch(() => {});
  }, []);

  const visible = usePageVisible();
  useEffect(() => {
    // Looking for browsers reads the registry and disk: never while hidden in the tray.
    if (!visible) return;
    refresh();
    const timer = window.setInterval(refresh, watching ? 3000 : 60000);
    return () => window.clearInterval(timer);
  }, [refresh, watching, visible]);

  return { data, refresh };
}
