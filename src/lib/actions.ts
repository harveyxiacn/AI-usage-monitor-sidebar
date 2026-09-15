// Small Svelte actions / utilities. [FRONTEND]
import type { Action } from 'svelte/action';

export function debounce<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const wrapped = (...args: A) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
  wrapped.cancel = () => {
    if (timer) clearTimeout(timer);
    timer = undefined;
  };
  return wrapped;
}

export interface SizeReport {
  width: number;
  height: number;
}

/**
 * ResizeObserver action. Reports the element's border-box size in CSS px,
 * debounced (default 50 ms) so a burst of layout changes — ring count, scale,
 * font load — collapses into a single sidebar_relayout / popover_relayout call.
 */
export const observeSize: Action<HTMLElement, (size: SizeReport) => void> = (node, callback) => {
  let cb = callback;
  let last = { width: -1, height: -1 };

  const emit = debounce((size: SizeReport) => {
    if (size.width === last.width && size.height === last.height) return;
    last = size;
    cb(size);
  }, 50);

  const ro = new ResizeObserver(() => {
    // getBoundingClientRect gives CSS px including borders, which is exactly
    // what the platform layer needs to size the window.
    const r = node.getBoundingClientRect();
    emit({ width: Math.ceil(r.width), height: Math.ceil(r.height) });
  });
  ro.observe(node);

  return {
    update(next: (size: SizeReport) => void) {
      cb = next;
    },
    destroy() {
      emit.cancel();
      ro.disconnect();
    },
  };
};
