// Keyboard focus helpers for modal dialogs (command palette, share card). [FRONTEND]

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Tabbable descendants of `container`, in DOM order. */
export function focusableIn(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => !el.hasAttribute('hidden') && el.getAttribute('aria-hidden') !== 'true',
  );
}

/**
 * Where Tab should go from `index` among `count` stops; `null` = let the
 * browser decide (somewhere in the middle). Pure, so it can be tested.
 */
export function nextTabStop(index: number, count: number, backwards: boolean): number | null {
  if (count <= 0) return null;
  if (backwards) return index <= 0 ? count - 1 : null;
  return index < 0 || index >= count - 1 ? 0 : null;
}

/** Keep Tab / Shift+Tab inside `container`. Call from the dialog's keydown. */
export function trapTab(event: KeyboardEvent, container: HTMLElement): void {
  if (event.key !== 'Tab') return;
  const stops = focusableIn(container);
  if (stops.length === 0) {
    event.preventDefault();
    container.focus();
    return;
  }
  const active = document.activeElement as HTMLElement | null;
  const target = nextTabStop(active ? stops.indexOf(active) : -1, stops.length, event.shiftKey);
  if (target !== null) {
    event.preventDefault();
    stops[target].focus();
  }
}
