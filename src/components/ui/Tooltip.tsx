import React, { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '../../lib/cn';

interface TooltipProps {
  content: React.ReactNode;
  shortcut?: string;
  side?: 'top' | 'bottom';
  children: React.ReactElement<any>;
  delay?: number;
}

/**
 * Lightweight hover tooltip. Rendered in a portal so it never gets
 * clipped by scroll containers, and flips/clamps to stay on screen.
 */
export const Tooltip: React.FC<TooltipProps> = ({ content, shortcut, side = 'bottom', children, delay = 450 }) => {
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const timer = useRef<number | undefined>(undefined);

  const show = useCallback(
    (e: React.SyntheticEvent<HTMLElement>) => {
      const el = e.currentTarget;
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setAnchor(el.getBoundingClientRect()), delay);
    },
    [delay],
  );
  const hide = useCallback(() => {
    window.clearTimeout(timer.current);
    setAnchor(null);
  }, []);

  if (!content) return children;

  const child = React.Children.only(children);
  const props = child.props as Record<string, any>;

  return (
    <>
      {React.cloneElement(child, {
        onMouseEnter: (e: React.MouseEvent<HTMLElement>) => {
          props.onMouseEnter?.(e);
          show(e);
        },
        onMouseLeave: (e: React.MouseEvent<HTMLElement>) => {
          props.onMouseLeave?.(e);
          hide();
        },
        onMouseDown: (e: React.MouseEvent<HTMLElement>) => {
          props.onMouseDown?.(e);
          hide();
        },
      })}
      {anchor && <TooltipBubble anchor={anchor} side={side} content={content} shortcut={shortcut} />}
    </>
  );
};

const TooltipBubble: React.FC<{ anchor: DOMRect; side: 'top' | 'bottom'; content: React.ReactNode; shortcut?: string }> = ({
  anchor,
  side,
  content,
  shortcut,
}) => {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    let y = side === 'bottom' ? anchor.bottom + 6 : anchor.top - height - 6;
    if (y + height > window.innerHeight - 4) y = anchor.top - height - 6;
    if (y < 4) y = anchor.bottom + 6;
    const x = Math.min(Math.max(4, anchor.left + anchor.width / 2 - width / 2), window.innerWidth - width - 4);
    setPos({ x, y });
  }, [anchor, side]);

  return createPortal(
    <div
      ref={ref}
      role="tooltip"
      style={{ left: pos?.x ?? -9999, top: pos?.y ?? -9999 }}
      className={cn(
        'pointer-events-none fixed z-[100] flex items-center gap-2 rounded-md bg-elevated px-2 py-1 text-[11.5px] font-medium whitespace-nowrap text-fg shadow-pop',
      )}
    >
      {content}
      {shortcut && <span className="text-fg-3">{shortcut}</span>}
    </div>,
    document.body,
  );
};
