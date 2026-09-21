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

// ---------------------------------------------------------- hex / HSL ------
// The concentric ring ramp is *derived* from the user's provider colour, so we
// need a real colour space round-trip: hex → HSL, raise the lightness, back to
// hex. Hue and saturation are preserved so every ring of a group is obviously
// the same colour, just less intense.

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** "#f80" / "#ff8800" / "ff8800" → {r,g,b}; null when it isn't a hex colour. */
export function parseHex(hex: string): Rgb | null {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  let h = m[1];
  if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
  const n = parseInt(h, 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const hex2 = (v: number) => Math.round(clamp01(v) * 255).toString(16).padStart(2, '0');

export function rgbToHsl({ r, g, b }: Rgb): { h: number; s: number; l: number } {
  const rf = r / 255;
  const gf = g / 255;
  const bf = b / 255;
  const max = Math.max(rf, gf, bf);
  const min = Math.min(rf, gf, bf);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };
  // saturation of an HSL colour is the chroma normalised by how much room the
  // current lightness leaves for it
  const s = d / (1 - Math.abs(2 * l - 1));
  let h: number;
  if (max === rf) h = ((gf - bf) / d) % 6;
  else if (max === gf) h = (bf - rf) / d + 2;
  else h = (rf - gf) / d + 4;
  h *= 60;
  return { h: h < 0 ? h + 360 : h, s, l };
}

export function hslToHex(h: number, s: number, l: number): string {
  const c = (1 - Math.abs(2 * clamp01(l) - 1)) * clamp01(s);
  const hp = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const [r1, g1, b1] =
    hp < 1 ? [c, x, 0]
    : hp < 2 ? [x, c, 0]
    : hp < 3 ? [0, c, x]
    : hp < 4 ? [0, x, c]
    : hp < 5 ? [x, 0, c]
    : [c, 0, x];
  const m = clamp01(l) - c / 2;
  return `#${hex2(r1 + m)}${hex2(g1 + m)}${hex2(b1 + m)}`;
}

/**
 * How much saturation is given up per unit of added lightness. Pure HSL keeps
 * saturation constant, which turns a saturated teal or green neon as it
 * lightens (#10a37f + 18 % → #25eaba); bleeding a little saturation out keeps
 * the tint in the same family as the base, the way the hand-tuned ramps in
 * theme.css do.
 */
const SAT_FALLOFF = 0.55;

/**
 * Same hue, lightness raised by `delta` (0..1) and capped at `maxL` so a
 * light-theme ramp never washes out into the white surface; saturation eases
 * off slightly as it lightens (see SAT_FALLOFF).
 * Invalid input is returned untouched so a half-typed hex can't blank the UI.
 */
export function lighten(hex: string, delta: number, maxL = 0.92): string {
  const rgb = parseHex(hex);
  if (!rgb) return hex;
  const { h, s, l } = rgbToHsl(rgb);
  return hslToHex(h, s * Math.max(0, 1 - delta * SAT_FALLOFF), Math.min(maxL, l + delta));
}

/** "#ff5c1a" → "255 92 26", ready for `rgb(<x> / <alpha>)`. */
export function hexToRgbChannels(hex: string): string | null {
  const rgb = parseHex(hex);
  return rgb ? `${rgb.r} ${rgb.g} ${rgb.b}` : null;
}

/** WCAG relative luminance, 0 (black) .. 1 (white). */
export function relativeLuminance({ r, g, b }: Rgb): number {
  const lin = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG contrast ratio, 1 .. 21. */
export function contrastRatio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * The widget is translucent and cannot see what is behind it, so its text
 * carries its own backdrop: a halo in whichever of black / white contrasts
 * more with the text colour. Returns CSS rgb channels ("0 0 0").
 */
export function haloChannels(textHex: string): string | null {
  const rgb = parseHex(textHex);
  if (!rgb) return null;
  const black = { r: 0, g: 0, b: 0 };
  const white = { r: 255, g: 255, b: 255 };
  return contrastRatio(rgb, black) >= contrastRatio(rgb, white) ? '0 0 0' : '255 255 255';
}
