import { useEffect, useRef } from 'react';

/**
 * Overlay stack (dialogs, menus, popovers). Escape always dismisses the
 * top-most layer only, and the app's global shortcuts are suspended while
 * any layer is open.
 */
type Layer = { id: number; onEscape: () => void };

const stack: Layer[] = [];
let nextId = 1;
let installed = false;

function install() {
  if (installed) return;
  installed = true;
  window.addEventListener(
    'keydown',
    (e) => {
      if (e.key !== 'Escape' || stack.length === 0) return;
      e.preventDefault();
      e.stopPropagation();
      stack[stack.length - 1].onEscape();
    },
    true,
  );
}

export function hasOpenLayer(): boolean {
  return stack.length > 0;
}

export function useLayer(active: boolean, onEscape: () => void) {
  const cb = useRef(onEscape);
  cb.current = onEscape;

  useEffect(() => {
    if (!active) return;
    install();
    const layer: Layer = { id: nextId++, onEscape: () => cb.current() };
    stack.push(layer);
    return () => {
      const i = stack.findIndex((l) => l.id === layer.id);
      if (i >= 0) stack.splice(i, 1);
    };
  }, [active]);
}
