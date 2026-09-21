// Derived ring list for the sidebar. [FRONTEND]
//
// One `RingItem` is one *rendered unit* in the pill — a ring group. In
// `ringMode = "concentric"` a group stacks up to three arcs for a single
// provider (outer weekly, inner 5-hour, innermost per-model scope); in the
// other two modes a group is a plain single-arc ring. Everything downstream
// (hover targeting, popover requests, the collapsed handle colour) works on the
// group, so the sidebar loop is identical in all three modes.
import { severityOf, worstSeverity, type Severity } from '$lib/format';
import { altVar, rampVar } from '$lib/providers';
import { settings } from './settings.svelte';
import { snapshot } from './snapshot.svelte';
import type { AppSnapshot, ProviderId, ProviderQuota, QuotaWindow, Settings } from '$lib/types';

/** One arc of a ring group, outer → inner. */
export interface RingArc {
  window: QuotaWindow;
  /** css colour expression, e.g. "var(--accent-claude-2)" */
  accent: string;
  severity: Severity;
}

export interface RingItem {
  /** stable key for keyed each-blocks */
  key: string;
  provider: ProviderId;
  quota: ProviderQuota;
  /** outer → inner; empty when the provider has no usable window */
  arcs: RingArc[];
  /** window the percent label under the group refers to (the primary one) */
  labelWindow: QuotaWindow | null;
  /** position in the sidebar list — sent to the platform as PopoverRequest.ringIndex */
  index: number;
  /** accent of the outermost arc — collapsed handle, hover affordances */
  accent: string;
  /** worst severity across every arc of the group */
  severity: Severity;
}

/**
 * Concentric ramp (outer → inner) and the contrasting "alt" hue used by
 * ringMode="all", where two rings of one provider must be told apart rather
 * than read as one material. Both are built from the provider id instead of a
 * hand-listed table, so a provider the frontend has never heard of still gets
 * a ring — theme.css supplies `--accent-fallback*` behind every lookup.
 */

/** Accent for slot `slot` of a provider in ringMode "primary" / "all". */
export function accentFor(provider: ProviderId, slot: number): string {
  return altVar(provider, slot);
}

/** Accent for depth `depth` (0 = outermost) of a concentric group. */
export function rampFor(provider: ProviderId, depth: number): string {
  return rampVar(provider, depth);
}

/** primary window first, then the remaining account-wide windows in backend order. */
function nonScopedWindows(quota: ProviderQuota): QuotaWindow[] {
  return quota.windows
    .filter((w) => w.scope == null)
    .sort((a, b) => Number(b.isPrimary) - Number(a.isPrimary));
}

function primaryOf(quota: ProviderQuota): QuotaWindow | null {
  const nonScoped = nonScopedWindows(quota);
  return nonScoped.find((w) => w.isPrimary) ?? nonScoped[0] ?? null;
}

/**
 * Arcs of a concentric group, outer → inner:
 *   outer  = the account-wide weekly window
 *   inner  = the account-wide 5-hour window, omitted when the plan has none
 *            (Codex Pro / prolite only get the weekly window)
 *   third  = the first *per-model* scoped weekly window, Claude only and only
 *            when showScopedRing is on. Codex' scoped windows are per-feature
 *            additional limits, not a slice of the account limit, so stacking
 *            them inside the same group would be misleading — they stay in the
 *            popover's "More limits" section.
 * If the provider reports neither a weekly nor a 5-hour window (e.g. only
 * `other`-kind windows) the group falls back to the account-wide windows with
 * the primary one outermost, so something sensible is still drawn.
 */
function concentricWindows(quota: ProviderQuota, s: Settings): QuotaWindow[] {
  const nonScoped = nonScopedWindows(quota);
  const weekly = nonScoped.find((w) => w.kind === 'seven_day');
  const fiveHour = nonScoped.find((w) => w.kind === 'five_hour');

  const out: QuotaWindow[] = [];
  if (weekly) out.push(weekly);
  if (fiveHour) out.push(fiveHour);
  if (out.length === 0) out.push(...nonScoped.slice(0, 2));

  if (s.showScopedRing && quota.provider !== 'codex') {
    const scoped = quota.windows.find((w) => w.scope != null && w.kind === 'seven_day');
    if (scoped) out.push(scoped);
  }
  return out.slice(0, 3);
}

/** Windows that each get their own plain ring in "primary" / "all". */
function plainWindows(quota: ProviderQuota, mode: Settings['ringMode']): QuotaWindow[] {
  const ordered = nonScopedWindows(quota);
  return mode === 'all' ? ordered : ordered.slice(0, 1);
}

export function buildRingItems(snap: AppSnapshot | null, s: Settings): RingItem[] {
  if (!snap) return [];
  const visible = snap.providers
    .filter((p) => {
      const cfg = s.providers[p.provider];
      // explicitly switched off, or the backend reports the provider disabled
      return (cfg?.enabled ?? true) && p.status !== 'disabled';
    })
    .sort((a, b) => (s.providers[a.provider]?.order ?? 0) - (s.providers[b.provider]?.order ?? 0));

  const items: RingItem[] = [];

  const push = (
    quota: ProviderQuota,
    key: string,
    windows: QuotaWindow[],
    accentOf: (depth: number) => string,
    labelWindow: QuotaWindow | null
  ) => {
    const arcs: RingArc[] = windows.map((w, depth) => ({
      window: w,
      accent: accentOf(depth),
      severity: severityOf(w.usedPercent, s.thresholds),
    }));
    items.push({
      key,
      provider: quota.provider,
      quota,
      arcs,
      labelWindow,
      index: items.length,
      accent: arcs[0]?.accent ?? accentOf(0),
      severity: worstSeverity(arcs.map((a) => a.severity)),
    });
  };

  for (const quota of visible) {
    if (s.ringMode === 'concentric') {
      const windows = concentricWindows(quota, s);
      push(
        quota,
        `${quota.provider}:group`,
        windows,
        (depth) => rampFor(quota.provider, depth),
        primaryOf(quota)
      );
      continue;
    }

    const windows = plainWindows(quota, s.ringMode);
    if (windows.length === 0) {
      // not_logged_in / error with no cached windows — still show a dimmed ring
      push(quota, `${quota.provider}:none`, [], (d) => accentFor(quota.provider, d), null);
      continue;
    }
    windows.forEach((w, slot) => {
      push(quota, `${quota.provider}:${w.kind}:${slot}`, [w], () => accentFor(quota.provider, slot), w);
    });
  }
  return items;
}

class RingStore {
  items = $derived.by(() => buildRingItems(snapshot.value, settings.value));
}

export const rings = new RingStore();
