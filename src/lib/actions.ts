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

export type DragPhase = 'start' | 'move' | 'end' | 'cancel';

/** Pointer travel before a press becomes a drag instead of a click, CSS px. */
export const DRAG_THRESHOLD = 5;

/**
 * Drag-to-move for a window the platform layer positions. Reports the pointer
 * travel in *screen* coordinates, which stay meaningful while the window moves
 * under the pointer. Presses that never pass the threshold remain ordinary
 * clicks; the click that ends a real drag is swallowed so it cannot pin a ring.
 */
export const dragHandle: Action<HTMLElement, (phase: DragPhase, dx: number, dy: number) => void> = (node, callback) => {
  let cb = callback;
  let pointer: number | null = null;
  let origin = { x: 0, y: 0 };
  let last = { x: 0, y: 0 };
  let dragging = false;
  let frame = 0;

  const travel = (ev: PointerEvent) => ({ x: ev.screenX - origin.x, y: ev.screenY - origin.y });

  function finish(phase: 'end' | 'cancel') {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    if (dragging) {
      cb(phase, last.x, last.y);
      if (pointer !== null && node.hasPointerCapture(pointer)) node.releasePointerCapture(pointer);
      node.removeAttribute('data-dragging');
    }
    pointer = null;
    dragging = false;
  }

  function down(ev: PointerEvent) {
    if (ev.button !== 0 || pointer !== null) return;
    pointer = ev.pointerId;
    origin = { x: ev.screenX, y: ev.screenY };
    last = { x: 0, y: 0 };
  }

  function move(ev: PointerEvent) {
    if (ev.pointerId !== pointer) return;
    last = travel(ev);
    if (!dragging) {
      if (Math.hypot(last.x, last.y) < DRAG_THRESHOLD) return;
      dragging = true;
      // Capture only now: capturing on press would retarget ordinary clicks.
      node.setPointerCapture(ev.pointerId);
      node.setAttribute('data-dragging', '');
      cb('start', 0, 0);
    }
    // One window move per frame is as much as a compositor can show.
    frame ||= requestAnimationFrame(() => {
      frame = 0;
      if (dragging) cb('move', last.x, last.y);
    });
  }

  function up(ev: PointerEvent) {
    if (ev.pointerId !== pointer) return;
    if (dragging) {
      last = travel(ev);
      window.addEventListener('click', swallow, { capture: true, once: true });
      // No click follows a drag released outside the window; do not eat a later one.
      setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), 0);
    }
    finish('end');
  }

  function swallow(ev: MouseEvent) {
    ev.stopPropagation();
    ev.preventDefault();
  }

  function key(ev: KeyboardEvent) {
    if (ev.key === 'Escape' && dragging) finish('cancel');
  }

  const cancel = (ev: PointerEvent) => ev.pointerId === pointer && finish('cancel');

  node.addEventListener('pointerdown', down);
  node.addEventListener('pointermove', move);
  node.addEventListener('pointerup', up);
  node.addEventListener('pointercancel', cancel);
  window.addEventListener('keydown', key);

  return {
    update(next: (phase: DragPhase, dx: number, dy: number) => void) {
      cb = next;
    },
    destroy() {
      finish('cancel');
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointermove', move);
      node.removeEventListener('pointerup', up);
      node.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
    },
  };
};
