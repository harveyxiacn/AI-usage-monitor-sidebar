// Applies Settings.theme / surfaceStyle / scale / opacity / language and the
// user-tunable colours + sizes to <html>. [FRONTEND]
//
// Everything here writes *inline* custom properties on the root element, which
// therefore beat every rule in theme.css. A setting left at its contract
// default removes the inline property again so the stylesheet (and with it the
// per-theme value) takes over — that is why `setVar` accepts null.
import { haloChannels, hexToRgbChannels, lighten, parseHex } from '$lib/colors';
import { setLanguage } from '$lib/i18n/i18n.svelte';
import { clampSize, defaultColors } from './settings.svelte';
import type { ProviderId, Settings, SizeSettings } from '$lib/types';

type WindowKind = 'sidebar' | 'popover' | 'dashboard';

let media: MediaQueryList | null = null;
let mediaHandler: (() => void) | null = null;
let lastSettings: Settings | null = null;
let windowKind: WindowKind = 'sidebar';

function resolveTheme(theme: Settings['theme']): 'dark' | 'light' {
  if (theme === 'dark' || theme === 'light') return theme;
  if (typeof window === 'undefined' || !window.matchMedia) return 'dark';
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

function setVar(root: HTMLElement, name: string, value: string | null): void {
  if (value === null) root.style.removeProperty(name);
  else root.style.setProperty(name, value);
}

/**
 * Concentric ramp steps, in lightness delta from the chosen colour.
 * On a near-black pill the pale end still reads, so the dark theme uses the
 * full +18 / +34 %. On a near-white pill it would disappear, so the light
 * theme walks a shorter way and is capped well below white.
 */
const RAMP_DARK = { deltas: [0, 0.18, 0.34], maxL: 0.92 };
const RAMP_LIGHT = { deltas: [0, 0.11, 0.21], maxL: 0.8 };

function applyProviderRamp(
  root: HTMLElement,
  provider: ProviderId,
  color: string,
  theme: 'dark' | 'light'
): void {
  const isDefault = color.toLowerCase() === defaultColors[provider].toLowerCase();
  const valid = parseHex(color) !== null;
  // A colour left at the contract default keeps theme.css in charge, so the
  // hand-tuned light/dark ramps and the contrasting `-alt` hue survive.
  if (isDefault || !valid) {
    for (const suffix of ['', '-1', '-2', '-3', '-alt']) {
      setVar(root, `--accent-${provider}${suffix}`, null);
    }
    return;
  }
  const { deltas, maxL } = theme === 'light' ? RAMP_LIGHT : RAMP_DARK;
  setVar(root, `--accent-${provider}`, color);
  deltas.forEach((d, i) => setVar(root, `--accent-${provider}-${i + 1}`, lighten(color, d, maxL)));
  // ringMode="all" wants a *contrasting* second hue; the built-in one no longer
  // matches a custom accent, so derive a clearly lighter tint instead.
  setVar(root, `--accent-${provider}-alt`, lighten(color, theme === 'light' ? 0.16 : 0.26, maxL));
}

function applySizes(root: HTMLElement, sizes: SizeSettings): void {
  const rem = (px: number) => `${px / 16}rem`;
  // every size is written in rem so Settings.scale keeps scaling the whole UI
  setVar(root, '--ring-size', rem(clampSize('ringSize', sizes.ringSize)));
  setVar(root, '--ring-stroke', String(clampSize('ringStroke', sizes.ringStroke)));
  setVar(root, '--bar-gap', rem(clampSize('barGap', sizes.barGap)));
  setVar(root, '--bar-padding', rem(clampSize('barPadding', sizes.barPadding)));
  const radius = rem(clampSize('cornerRadius', sizes.cornerRadius));
  setVar(root, '--r-pill', radius);
  setVar(root, '--r-bubble', radius);
  setVar(root, '--label-size', rem(clampSize('labelSize', sizes.labelSize)));
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
  const theme = resolveTheme(s.theme);

  root.dataset.theme = theme;
  root.dataset.surface = s.surfaceStyle;
  const scale = clamp(s.scale, 0.75, 1.5);
  const alpha = clamp(s.opacity, 0.3, 1);
  root.style.setProperty('--ui-scale', String(scale));
  root.style.setProperty('--surface-alpha', String(alpha));
  setLanguage(s.language);

  applyProviderRamp(root, 'claude', s.colors.claude, theme);
  applyProviderRamp(root, 'codex', s.colors.codex, theme);
  setVar(root, '--warn', parseHex(s.colors.warn) ? s.colors.warn : null);
  setVar(root, '--critical', parseHex(s.colors.critical) ? s.colors.critical : null);

  // Surface tint: the alpha is the *material's* (theme.css glass fill, 1 for solid)
  // multiplied by Settings.opacity, exactly like the stylesheet does it.
  const tint = s.colors.surface ? hexToRgbChannels(s.colors.surface) : null;
  if (tint) {
    const base = s.surfaceStyle === 'cyber' ? 0.82 : s.surfaceStyle !== 'glass' ? 1 : theme === 'light' ? 0.6 : 0.66;
    setVar(root, '--surface-fill', `rgb(${tint} / calc(${base} * var(--surface-alpha)))`);
    // the ring/percent badge halo is keyed off the surface colour too
    setVar(root, '--bar-bg-rgb', tint);
  } else {
    setVar(root, '--surface-fill', null);
    setVar(root, '--bar-bg-rgb', null);
  }

  // Custom text colour applies to the two transparent widget windows only —
  // the dashboard is a normal app window and keeps its theme palette.
  const text = s.colors.text && parseHex(s.colors.text) ? s.colors.text : null;
  setVar(root, '--text', windowKind === 'dashboard' ? null : text);
  setVar(root, '--logo', windowKind === 'dashboard' ? null : text);
  // A custom text colour may be the opposite of the theme's (dark text on the
  // dark theme); the legibility halo has to follow the text, not the theme.
  setVar(root, '--text-halo-rgb', windowKind === 'dashboard' || !text ? null : haloChannels(text));

  applySizes(root, s.sizes);

  if (s.theme === 'auto') {
    if (!media && typeof window !== 'undefined' && window.matchMedia) {
      media = window.matchMedia('(prefers-color-scheme: light)');
      mediaHandler = () => {
        // re-run the whole thing: the derived ramps differ per theme
        if (lastSettings?.theme === 'auto') applyTheme(lastSettings);
      };
      media.addEventListener('change', mediaHandler);
    }
  } else if (media && mediaHandler) {
    media.removeEventListener('change', mediaHandler);
    media = null;
    mediaHandler = null;
  }
}

/**
 * Marks which Tauri window this document is, see base.css. Call it from the
 * route's module body (not onMount) so `applyTheme` already knows the kind the
 * first time it runs.
 */
export function markWindow(kind: WindowKind): void {
  windowKind = kind;
  if (typeof document !== 'undefined') document.documentElement.dataset.win = kind;
}

function clamp(v: number, lo: number, hi: number): number {
  return Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : lo;
}
