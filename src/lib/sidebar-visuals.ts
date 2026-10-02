// Pure presentation rules of the floating bar that do not belong to a single
// component: label content, mini-bar / forecast-arc geometry, the collapsed
// handle's segments and the popover's two-column split. Rune-free and
// i18n-free so `tests/sidebar-visuals.unit.ts` can import it in plain Node.
// [FRONTEND]
import { clampPercent, severityOf, type Severity } from './severity';
import { providerPolled } from './sidebar-items';
import { altVar, quotaKey, rampVar } from './providers';
import type {
  AppSnapshot,
  LabelContent,
  PercentMode,
  PercentPosition,
  ProviderId,
  Settings,
} from './types';

// ---------------------------------------------------------------- label ----

export interface LabelLayout {
  /** text for the ring's centre (always the percentage), or null */
  center: string | null;
  /** first text line: under the ring, or to its right on a top/bottom edge */
  main: string | null;
  /** second line under the ring (vertical edges, `both` only) */
  sub: string | null;
}

/**
 * What to write where.
 *  - `below` + percent: the percentage; `reset`: the countdown; `both`: on a
 *    vertical edge two stacked lines, on a horizontal one "73% · 1h12".
 *  - `center` can only fit the percentage, so it always gets that and the
 *    countdown (when asked for) goes to the usual label slot instead.
 *  - A top/bottom bar has spare width, so it writes percent + countdown to the
 *    right of the ring unless the user asked for the countdown alone.
 *  - An unknown reset never leaves an empty label: it degrades to the percent.
 */
export function labelLayout(opts: {
  content: LabelContent;
  position: PercentPosition;
  horizontal: boolean;
  percent: string;
  reset: string;
}): LabelLayout {
  const { position, horizontal, percent, reset } = opts;
  const content: LabelContent = horizontal && opts.content === 'percent' ? 'both' : opts.content;
  const wantsReset = content !== 'percent' && reset !== '';

  if (position === 'center') {
    return { center: percent, main: wantsReset ? reset : null, sub: null };
  }
  if (content === 'reset') {
    return { center: null, main: wantsReset ? reset : percent, sub: null };
  }
  if (content === 'both' && wantsReset) {
    return horizontal
      ? { center: null, main: `${percent} · ${reset}`, sub: null }
      : { center: null, main: percent, sub: reset };
  }
  return { center: null, main: percent, sub: null };
}

/**
 * Mini-bar text: a bar has no centre, so the percentage always leads. On a
 * vertical edge the countdown goes on its own line; on a horizontal one
 * everything joins into one "73% · 1h12" run.
 */
export function miniText(layout: LabelLayout, horizontal: boolean): { primary: string | null; secondary: string | null } {
  const lead = layout.center ?? layout.main;
  const rest = layout.center != null ? layout.main : layout.sub;
  if (horizontal) {
    const parts = [layout.center, layout.main, layout.sub].filter((v): v is string => !!v);
    return { primary: parts.length ? parts.join(' · ') : null, secondary: null };
  }
  return { primary: lead, secondary: rest };
}

// ------------------------------------------------------------- geometry ----

/** Dashed forecast arc range in "shown" percent (after the used/remaining flip). */
export function forecastArcRange(
  used: number | null,
  projected: number | null,
  mode: PercentMode
): { from: number; to: number } | null {
  if (used == null || projected == null) return null;
  const u = clampPercent(used);
  const p = clampPercent(projected);
  if (p <= u) return null;
  const a = mode === 'remaining' ? 100 - u : u;
  const b = mode === 'remaining' ? 100 - p : p;
  return { from: Math.min(a, b), to: Math.max(a, b) };
}

/**
 * SVG path of a circular arc of radius `r` about (c, c) from `fromPct` to
 * `toPct` (0 = 12 o'clock, clockwise). A full circle is nudged below 100 so
 * the endpoints stay distinct; an empty range gives an empty string.
 */
export function arcPath(c: number, r: number, fromPct: number, toPct: number): string {
  const from = clampPercent(fromPct);
  const to = Math.min(clampPercent(toPct), from + 99.99);
  if (to - from <= 0.01) return '';
  const pt = (pct: number) => {
    const a = ((pct / 100) * 360 - 90) * (Math.PI / 180);
    return [c + r * Math.cos(a), c + r * Math.sin(a)];
  };
  const [x1, y1] = pt(from);
  const [x2, y2] = pt(to);
  const large = to - from > 50 ? 1 : 0;
  const f = (v: number) => v.toFixed(3);
  return `M${f(x1)} ${f(y1)}A${f(r)} ${f(r)} 0 ${large} 1 ${f(x2)} ${f(y2)}`;
}

/** The projection reaches the limit: draw the "will run out" indicator. */
export function willRunOut(projected: number | null | undefined): boolean {
  return projected != null && projected >= 99.5;
}

// --------------------------------------------------------------- handle ----

export interface HandleSegment {
  provider: ProviderId;
  /** `quotaKey()` of the quota: unique even when two accounts share a provider */
  key: string;
  /** css colour: provider accent, or amber / red once it crossed a threshold */
  accent: string;
  severity: Severity;
  /** highest used percent of the provider's windows, null when unknown */
  usedPercent: number | null;
}

/**
 * One segment per polled provider for the collapsed auto-hide handle, in the
 * configured order. Like `barSeverity` it looks at ALL windows, so hiding a
 * ring never hides a warning. Segments are equal in size: a proportional
 * split would make a quiet provider vanish, the opposite of what a glanceable
 * handle is for.
 */
export function handleSegments(snap: AppSnapshot | null, s: Settings): HandleSegment[] {
  const polled = (snap?.providers ?? [])
    .filter((p) => providerPolled(p, s))
    .sort((a, b) => (s.providers[a.provider]?.order ?? 0) - (s.providers[b.provider]?.order ?? 0));
  return polled.map((q) => {
    const peak = q.windows.reduce<number | null>(
      (m, w) => (w.usedPercent == null ? m : Math.max(m ?? -1, w.usedPercent)),
      null
    );
    const severity = peak == null ? 'normal' : severityOf(peak, s.thresholds);
    const base = s.ringMode === 'concentric' ? rampVar(q.provider, 0) : altVar(q.provider, 0);
    const accent = severity === 'critical' ? 'var(--critical)' : severity === 'warn' ? 'var(--warn)' : base;
    return { provider: q.provider, key: quotaKey(q), accent, severity, usedPercent: peak };
  });
}

/** Highest used percent across everything polled, null when nothing is known. */
export function handlePeak(snap: AppSnapshot | null, s: Settings): { provider: ProviderId; key: string; used: number } | null {
  // the first segment with the highest percent, which is the provider/account `barSeverity` calls the leader
  let best: HandleSegment | null = null;
  for (const seg of handleSegments(snap, s)) {
    if (seg.usedPercent != null && (best === null || seg.usedPercent > (best.usedPercent ?? -1))) best = seg;
  }
  return best ? { provider: best.provider, key: best.key, used: best.usedPercent! } : null;
}

// -------------------------------------------------------------- popover ----

/**
 * Two-column popover on a top/bottom bar: only with at least three rows, so a
 * short card stays compact. Account-wide windows go left, per-model / feature
 * limits right; with no scoped windows the account-wide ones are halved.
 * Returns null for the normal single-column layout.
 */
export function splitColumns<T>(main: T[], scoped: T[]): [T[], T[]] | null {
  if (main.length + scoped.length < 3) return null;
  if (scoped.length > 0) return [main, scoped];
  const half = Math.ceil(main.length / 2);
  return [main.slice(0, half), main.slice(half)];
}
