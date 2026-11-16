import { useCallback, useMemo, useState } from 'react';

/** The list's resizable columns (Name takes whatever is left). */
export type ColumnKey = 'size' | 'progress' | 'speed' | 'eta' | 'added';

export const COLUMNS: Record<ColumnKey, { min: number; max: number; def: number }> = {
  size: { min: 56, max: 200, def: 84 },
  progress: { min: 110, max: 520, def: 190 },
  speed: { min: 64, max: 200, def: 92 },
  eta: { min: 56, max: 160, def: 76 },
  added: { min: 60, max: 200, def: 84 },
};

const KEY = 'nexdrop.columns';
type Widths = Record<ColumnKey, number>;

const defaults = (): Widths => ({ size: COLUMNS.size.def, progress: COLUMNS.progress.def, speed: COLUMNS.speed.def, eta: COLUMNS.eta.def, added: COLUMNS.added.def });
const clamp = (k: ColumnKey, w: number) => Math.round(Math.min(COLUMNS[k].max, Math.max(COLUMNS[k].min, w)));

function load(): Widths {
  const w = defaults();
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) || '{}');
    (Object.keys(COLUMNS) as ColumnKey[]).forEach((k) => {
      if (typeof saved[k] === 'number') w[k] = clamp(k, saved[k]);
    });
  } catch {
    /* no saved widths, or storage is unavailable */
  }
  return w;
}

/** Column widths the user can drag, remembered between runs. */
export function useColumnWidths() {
  const [widths, setWidths] = useState<Widths>(load);

  const persist = useCallback((w: Widths) => {
    try {
      localStorage.setItem(KEY, JSON.stringify(w));
    } catch {
      /* private mode */
    }
  }, []);

  const setWidth = useCallback((k: ColumnKey, w: number, save = false) => {
    setWidths((prev) => {
      const next = { ...prev, [k]: clamp(k, w) };
      if (save) persist(next);
      return next;
    });
  }, [persist]);

  const reset = useCallback((k: ColumnKey) => setWidth(k, COLUMNS[k].def, true), [setWidth]);

  // The grid reads these (see `.nx-grid`). Narrow windows drop the "Added" column.
  const vars = useMemo(
    () =>
      ({
        '--nx-cols': `36px minmax(220px, 1fr) ${widths.size}px ${widths.progress}px ${widths.speed}px ${widths.eta}px ${widths.added}px 124px`,
        '--nx-cols-narrow': `36px minmax(200px, 1fr) ${widths.size}px ${widths.progress}px ${widths.speed}px ${widths.eta}px 124px`,
      }) as React.CSSProperties,
    [widths],
  );

  return { widths, setWidth, reset, vars };
}
