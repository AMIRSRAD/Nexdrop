import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { motion } from 'motion/react';
import { Check, type LucideIcon } from 'lucide-react';
import { cn } from '../../lib/cn';
import { useLayer } from '../../lib/layers';

export type MenuItem =
  | {
      label: string;
      icon?: LucideIcon;
      shortcut?: string;
      onSelect: () => void;
      danger?: boolean;
      disabled?: boolean;
      checked?: boolean;
      hint?: string;
    }
  | { separator: true }
  | { heading: string };

export interface MenuPosition {
  /** Pointer position (context menu) */
  x?: number;
  y?: number;
  /** Or anchor element rect (dropdown) */
  anchor?: DOMRect;
  align?: 'start' | 'end';
}

interface MenuProps {
  position: MenuPosition | null;
  items: MenuItem[];
  onClose: () => void;
  minWidth?: number;
}

export const Menu: React.FC<MenuProps> = ({ position, items, onClose, minWidth = 200 }) => {
  useLayer(!!position, onClose);
  if (!position) return null;
  return createPortal(<MenuSurface position={position} items={items} onClose={onClose} minWidth={minWidth} />, document.body);
};

const MenuSurface: React.FC<Required<Omit<MenuProps, 'position'>> & { position: MenuPosition }> = ({
  position,
  items,
  onClose,
  minWidth,
}) => {
  const ref = useRef<HTMLDivElement>(null);
  const [coords, setCoords] = useState<{ x: number; y: number } | null>(null);
  const [activeIndex, setActiveIndex] = useState(-1);

  const selectable = items
    .map((item, i) => ({ item, i }))
    .filter(({ item }) => 'onSelect' in item && !item.disabled)
    .map(({ i }) => i);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    let x: number;
    let y: number;
    if (position.anchor) {
      const a = position.anchor;
      x = position.align === 'end' ? a.right - width : a.left;
      y = a.bottom + 4;
      if (y + height > window.innerHeight - 8) y = a.top - height - 4;
    } else {
      x = position.x ?? 0;
      y = position.y ?? 0;
      if (x + width > window.innerWidth - 8) x = x - width;
      if (y + height > window.innerHeight - 8) y = Math.max(8, y - height);
    }
    x = Math.max(8, Math.min(x, window.innerWidth - width - 8));
    setCoords({ x, y: Math.max(8, y) });
    el.focus();
  }, [position]);

  useEffect(() => {
    const onPointer = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onBlur = () => onClose();
    window.addEventListener('mousedown', onPointer, true);
    window.addEventListener('blur', onBlur);
    window.addEventListener('resize', onBlur);
    return () => {
      window.removeEventListener('mousedown', onPointer, true);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('resize', onBlur);
    };
  }, [onClose]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (selectable.length === 0) return;
      const pos = selectable.indexOf(activeIndex);
      const next =
        e.key === 'ArrowDown'
          ? selectable[(pos + 1) % selectable.length]
          : selectable[(pos - 1 + selectable.length) % selectable.length];
      setActiveIndex(next);
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const item = items[activeIndex];
      if (item && 'onSelect' in item && !item.disabled) {
        onClose();
        item.onSelect();
      }
    } else if (e.key === 'Tab') {
      e.preventDefault();
      onClose();
    }
  };

  return (
    <motion.div
      ref={ref}
      role="menu"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.09 }}
      style={{ left: coords?.x ?? -9999, top: coords?.y ?? -9999, minWidth }}
      className="fixed z-[90] rounded-lg bg-elevated p-1 shadow-pop outline-none"
    >
      {items.map((item, i) => {
        if ('separator' in item) return <div key={i} className="mx-1 my-1 h-px bg-line" />;
        if ('heading' in item)
          return (
            <div key={i} className="px-2 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-fg-3 uppercase">
              {item.heading}
            </div>
          );
        const Icon = item.icon;
        return (
          <button
            key={i}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onMouseEnter={() => setActiveIndex(i)}
            onClick={() => {
              onClose();
              item.onSelect();
            }}
            className={cn(
              'flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-[13px] text-fg disabled:opacity-40',
              activeIndex === i && !item.disabled && (item.danger ? 'bg-danger/12 text-danger' : 'bg-hover'),
              item.danger && 'text-danger',
            )}
          >
            <span className="flex w-4 justify-center text-fg-2">
              {item.checked !== undefined ? (
                item.checked ? <Check size={14} className="text-accent" /> : null
              ) : Icon ? (
                <Icon size={15} strokeWidth={1.9} className={item.danger ? 'text-danger' : undefined} />
              ) : null}
            </span>
            <span className="flex-1 truncate">{item.label}</span>
            {item.hint && <span className="text-[12px] text-fg-3 tabular">{item.hint}</span>}
            {item.shortcut && <span className="text-[11.5px] text-fg-3">{item.shortcut}</span>}
          </button>
        );
      })}
    </motion.div>
  );
};

/** Convenience hook for a button that opens a dropdown menu. */
export function useDropdown(align: 'start' | 'end' = 'start') {
  const [position, setPosition] = useState<MenuPosition | null>(null);
  // The outside-mousedown that closes the menu fires before the trigger's
  // click; without this the trigger would immediately reopen it.
  const closedAt = useRef(0);
  return {
    position,
    open: (e: React.MouseEvent<HTMLElement>) => {
      if (Date.now() - closedAt.current < 200) return;
      setPosition({ anchor: e.currentTarget.getBoundingClientRect(), align });
    },
    close: () => {
      closedAt.current = Date.now();
      setPosition(null);
    },
    isOpen: !!position,
  };
}
