import React, { createContext, useCallback, useContext, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { CircleAlert, CircleCheck, Info, X } from 'lucide-react';
import { cn } from '../../lib/cn';

export type ToastTone = 'info' | 'success' | 'error';

export interface ToastOptions {
  title: string;
  description?: string;
  tone?: ToastTone;
  icon?: React.ReactNode;
  action?: { label: string; onClick: () => void };
  /** ms; 0 keeps it until dismissed */
  duration?: number;
  /** Replaces an existing toast with the same id */
  id?: string;
}

interface ToastItem extends ToastOptions {
  id: string;
}

interface ToastApi {
  show: (t: ToastOptions) => string;
  dismiss: (id: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used inside <ToastProvider>');
  return ctx;
}

export const ToastProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const timers = useRef(new Map<string, number>());

  const dismiss = useCallback((id: string) => {
    window.clearTimeout(timers.current.get(id));
    timers.current.delete(id);
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const show = useCallback(
    (t: ToastOptions) => {
      const id = t.id ?? `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
      setToasts((prev) => [...prev.filter((p) => p.id !== id), { ...t, id }].slice(-4));
      window.clearTimeout(timers.current.get(id));
      const duration = t.duration ?? (t.tone === 'error' ? 7000 : 4000);
      if (duration > 0) timers.current.set(id, window.setTimeout(() => dismiss(id), duration));
      return id;
    },
    [dismiss],
  );

  const api = useMemo(() => ({ show, dismiss }), [show, dismiss]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="pointer-events-none fixed right-4 bottom-10 z-[80] flex w-[360px] flex-col items-end gap-2">
        <AnimatePresence initial={false}>
          {toasts.map((t) => (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, y: 12, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24, transition: { duration: 0.14 } }}
              transition={{ duration: 0.18, ease: [0.2, 0.9, 0.3, 1] }}
              className="pointer-events-auto flex w-full items-start gap-3 rounded-xl bg-elevated p-3 pr-2 shadow-pop"
            >
              <div className="mt-px shrink-0">
                {t.icon ??
                  (t.tone === 'success' ? (
                    <CircleCheck size={17} className="text-success" />
                  ) : t.tone === 'error' ? (
                    <CircleAlert size={17} className="text-danger" />
                  ) : (
                    <Info size={17} className="text-accent" />
                  ))}
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium text-fg">{t.title}</div>
                {t.description && (
                  <div className="mt-0.5 line-clamp-2 text-[12px] break-all text-fg-2">{t.description}</div>
                )}
              </div>
              {t.action && (
                <button
                  type="button"
                  onClick={() => {
                    t.action!.onClick();
                    dismiss(t.id);
                  }}
                  className={cn(
                    'h-7 shrink-0 rounded-md px-2.5 text-[12px] font-semibold text-accent hover:bg-accent-soft',
                  )}
                >
                  {t.action.label}
                </button>
              )}
              <button
                type="button"
                aria-label="Dismiss"
                onClick={() => dismiss(t.id)}
                className="flex h-7 w-6 shrink-0 items-center justify-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
              >
                <X size={14} />
              </button>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  );
};
