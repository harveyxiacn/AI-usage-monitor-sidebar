// Provider-agnostic helpers. [FRONTEND]
//
// The provider set is decided by the *backend* (`get_providers` / the snapshot),
// not by this file: everything here has to render a provider the frontend has
// never heard of without a special case. Two providers ship hand-tuned accent
// tokens in theme.css; every other one falls back to the neutral ramp, which is
// why the accents are emitted as `var(--accent-<id>-1, var(--accent-fallback-1))`
// — for claude/codex the first variable is defined and the fallback is inert, so
// their colours are byte-for-byte what they were before.
import type { ColorSettings, ProviderId } from './types';

/** Colour keys of `Settings.colors` that are *not* a provider accent. */
const RESERVED_COLOR_KEYS: ReadonlySet<string> = new Set(['warn', 'critical', 'surface', 'text']);

/** Provider ids appear in CSS variable names; keep them to a safe alphabet. */
function cssId(provider: ProviderId): string {
  return provider.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'unknown';
}

/**
 * `--accent-<provider><suffix>` with the neutral ramp as the CSS-level
 * fallback, so an unknown provider is drawn in the theme's generic accent
 * instead of vanishing.
 */
export function accentVar(provider: ProviderId, suffix = ''): string {
  return `var(--accent-${cssId(provider)}${suffix}, var(--accent-fallback${suffix}))`;
}

/** Accent for depth `depth` (0 = outermost) of a concentric ring group. */
export function rampVar(provider: ProviderId, depth: number): string {
  return accentVar(provider, `-${Math.min(Math.max(depth, 0), 2) + 1}`);
}

/** Accent for slot `slot` of a provider in ringMode "primary" / "all". */
export function altVar(provider: ProviderId, slot: number): string {
  return accentVar(provider, slot === 0 ? '' : '-alt');
}

/** Provider ids that have a user-tunable accent in `Settings.colors`. */
export function providerColorKeys(colors: ColorSettings): ProviderId[] {
  return Object.keys(colors).filter((key) => !RESERVED_COLOR_KEYS.has(key));
}

/** The user's accent for `provider`, or null when it has no colour setting. */
export function providerColor(colors: ColorSettings, provider: ProviderId): string | null {
  if (RESERVED_COLOR_KEYS.has(provider)) return null;
  const value = (colors as unknown as Record<string, unknown>)[provider];
  return typeof value === 'string' ? value : null;
}

/**
 * Last-resort label for a provider the snapshot does not name: "github-copilot"
 * → "Github Copilot". The backend's `displayName` always wins when present.
 */
export function providerDisplayName(provider: ProviderId): string {
  return (
    provider
      .split(/[-_\s]+/)
      .filter(Boolean)
      .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ') || provider
  );
}
