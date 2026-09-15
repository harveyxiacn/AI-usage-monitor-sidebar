// Applies Settings.theme / scale / opacity / language to <html>. [FRONTEND]
import { setLanguage } from '$lib/i18n/i18n.svelte';
import type { Settings } from '$lib/types';

let media: MediaQueryList | null = null;
let mediaHandler: (() => void) | null = null;
let lastSettings: Settings | null = null;

function resolveTheme(theme: Settings['theme']): 'dark' | 'light' {
  if (theme === 'dark' || theme === 'light') return theme;
  if (typeof window === 'undefined' || !window.matchMedia) return 'dark';
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

/**
 * Idempotent; call it from an $effect so it re-runs whenever settings change.
 * For theme 'auto' a prefers-color-scheme listener is installed once and
 * removed again as soon as the user picks an explicit theme.
 */
export function applyTheme(s: Settings): void {
  if (typeof document === 'undefined') return;
  lastSettings = s;
  const root = document.documentElement;

  root.dataset.theme = resolveTheme(s.theme);
  root.dataset.surface = s.surfaceStyle;
  root.style.setProperty('--ui-scale', String(clamp(s.scale, 0.75, 1.5)));
  root.style.setProperty('--surface-alpha', String(clamp(s.opacity, 0.3, 1)));
  setLanguage(s.language);

  if (s.theme === 'auto') {
    if (!media && typeof window !== 'undefined' && window.matchMedia) {
      media = window.matchMedia('(prefers-color-scheme: light)');
      mediaHandler = () => {
        if (lastSettings?.theme === 'auto') root.dataset.theme = resolveTheme('auto');
      };
      media.addEventListener('change', mediaHandler);
    }
  } else if (media && mediaHandler) {
    media.removeEventListener('change', mediaHandler);
    media = null;
    mediaHandler = null;
  }
}

/** Marks which Tauri window this document is, see base.css. */
export function markWindow(kind: 'sidebar' | 'popover' | 'dashboard'): void {
  if (typeof document !== 'undefined') document.documentElement.dataset.win = kind;
}

function clamp(v: number, lo: number, hi: number): number {
  return Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : lo;
}
