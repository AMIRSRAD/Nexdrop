import { useEffect } from 'react';

/** Applies `dark` / `light` / `system` to <html data-theme>. */
export function useTheme(theme: string) {
  useEffect(() => {
    const root = document.documentElement;
    if (theme === 'light' || theme === 'dark') {
      root.dataset.theme = theme;
      return;
    }
    const mq = window.matchMedia('(prefers-color-scheme: light)');
    const apply = () => (root.dataset.theme = mq.matches ? 'light' : 'dark');
    apply();
    mq.addEventListener('change', apply);
    return () => mq.removeEventListener('change', apply);
  }, [theme]);
}
