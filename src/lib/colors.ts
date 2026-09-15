// Colour helpers. [FRONTEND]
// The design system lives in CSS custom properties; chart.js needs concrete
// colour strings, so anything handed to a canvas goes through resolveColor().
import type { ProviderId } from './types';

const VAR_RE = /^var\(\s*(--[\w-]+)\s*(?:,\s*(.*?))?\)$/;

export function cssVar(name: string, fallback = '#8a8a93'): string {
  if (typeof document === 'undefined') return fallback;
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

/** "var(--accent-claude)" → "#ff5c1a"; plain colours pass through untouched. */
export function resolveColor(color: string, fallback = '#8a8a93'): string {
  const m = VAR_RE.exec(color.trim());
  if (!m) return color;
  return cssVar(m[1], m[2]?.trim() || fallback);
}

/** Same hue pair as the sidebar rings, but as resolvable css variables. */
export const PROVIDER_ACCENT: Record<ProviderId, string> = {
  claude: 'var(--accent-claude)',
  codex: 'var(--accent-codex)',
};

/** Categorical palette for "group by model" series (stable by index). */
export const SERIES_COLORS = [
  'var(--accent-claude)',
  'var(--accent-claude-alt)',
  '#e0542f',
  'var(--accent-codex)',
  'var(--accent-codex-alt)',
  '#3fa9d6',
  '#d6b23f',
  '#a56fd6',
];

export function seriesColor(index: number): string {
  return SERIES_COLORS[index % SERIES_COLORS.length];
}

/** rgba() version of a resolved colour, for chart fills. */
export function withAlpha(color: string, alpha: number): string {
  const hex = resolveColor(color);
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  return `rgb(${(n >> 16) & 255} ${(n >> 8) & 255} ${n & 255} / ${alpha})`;
}
