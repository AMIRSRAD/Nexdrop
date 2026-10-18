import React, { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion, useIsPresent } from 'motion/react';
import { X } from 'lucide-react';
import { cn } from '../../lib/cn';
import { useLayer } from '../../lib/layers';

interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  icon?: React.ReactNode;
  width?: number;
  children: React.ReactNode;
  footer?: React.ReactNode;
  /** Content area handles its own scrolling/padding. */
  bare?: boolean;
  className?: string;
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export const Dialog: React.FC<DialogProps> = ({
  open,
  onClose,
  title,
  description,
  icon,
  width = 520,
  children,
  footer,
  bare,
  className,
}) => {
  useLayer(open, onClose);

  return createPortal(
    <AnimatePresence>
      {open && (
        <DialogFrame width={width} className={className} onClose={onClose}>
          <header className="flex items-start gap-3 px-5 pt-4 pb-3">
            {icon && <div className="mt-0.5 shrink-0">{icon}</div>}
            <div className="min-w-0 flex-1">
              <h2 className="text-[15px] leading-6 font-semibold text-fg">{title}</h2>
              {description && <p className="mt-0.5 text-[12.5px] text-fg-2">{description}</p>}
            </div>
            <button
              type="button"
              onClick={onClose}
              aria-label="Close"
              className="-mt-0.5 -mr-1.5 flex h-7 w-7 items-center justify-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
            >
              <X size={16} />
            </button>
          </header>
          <div className={cn('min-h-0 flex-1', bare ? 'flex flex-col' : 'overflow-y-auto px-5 pb-5')}>{children}</div>
          {footer && (
            <footer className="flex items-center gap-2 border-t border-line bg-surface px-5 py-3">{footer}</footer>
          )}
        </DialogFrame>
      )}
    </AnimatePresence>,
    document.body,
  );
};

const DialogFrame: React.FC<{ width: number; className?: string; onClose: () => void; children: React.ReactNode }> = ({
  width,
  className,
  onClose,
  children,
}) => {
  const ref = useRef<HTMLDivElement>(null);
  // While animating out, let clicks fall through to the app underneath
  const present = useIsPresent();

  useEffect(() => {
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const el = ref.current;
    if (el) {
      const target = el.querySelector<HTMLElement>('[data-autofocus]') ?? el.querySelector<HTMLElement>(FOCUSABLE);
      // Wait a frame so the element is mounted & visible
      requestAnimationFrame(() => target?.focus());
    }
    return () => previouslyFocused?.focus?.();
  }, []);

  const trapTab = (e: React.KeyboardEvent) => {
    if (e.key !== 'Tab' || !ref.current) return;
    const nodes = Array.from(ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((n) => n.offsetParent !== null);
    if (nodes.length === 0) return;
    const first = nodes[0];
    const last = nodes[nodes.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  return (
    <div className={cn('fixed inset-0 z-50 flex items-center justify-center p-6', !present && 'pointer-events-none')} onKeyDown={trapTab}>
      <motion.div
        className="absolute inset-0 bg-[var(--scrim)]"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.15 }}
        onMouseDown={onClose}
      />
      <motion.div
        ref={ref}
        role="dialog"
        aria-modal="true"
        initial={{ opacity: 0, scale: 0.97, y: 6 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.98, y: 4 }}
        transition={{ duration: 0.16, ease: [0.2, 0.9, 0.3, 1] }}
        style={{ width }}
        className={cn(
          'relative flex max-h-full max-w-full flex-col overflow-hidden rounded-xl bg-surface shadow-dialog',
          className,
        )}
      >
        {children}
      </motion.div>
    </div>
  );
};
