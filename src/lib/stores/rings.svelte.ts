// Derived ring list for the sidebar. [FRONTEND]
//
// One `RingItem` is one *rendered unit* in the pill — a ring group. In
// `ringMode = "concentric"` a group stacks up to three arcs for a single
// provider (outer weekly, inner 5-hour, innermost per-model scope); in the
// other two modes a group is a plain single-arc ring. Everything downstream
// (hover targeting, popover requests, the collapsed handle colour) works on the
// group, so the sidebar loop is identical in all three modes.
//
// *Which* providers and windows reach this file is decided by
// `$lib/sidebar-items` (Settings.sidebarItems / ProviderSettings.showInSidebar)
// before any group is built; everything here is pure presentation.
import { severityColor, severityOf, worstSeverity, type Severity } from '$lib/format';
import { altVar, rampVar } from '$lib/providers';
import { barGroups, barProviders, barSeverity, labelWindowOf } from '$lib/sidebar-items';
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
  /**
   * Worst severity across every arc of the group — a *drawn* value. The
   * bar-wide alarm level is `barSeverity()`, which also counts the windows the
   * user hid from the bar.
   */
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

/**
 * Colour of the collapsed auto-hide handle: the worst threshold reached by
 * *any* window of *any* polled provider, in the accent of the provider closest
 * to its limit. It reads the snapshot rather than the ring items on purpose —
 * hiding a ring (or a whole provider) from the bar must never hide a warning.
 */
export function handleColorOf(snap: AppSnapshot | null, s: Settings): string {
  const { severity, leader } = barSeverity(snap, s);
  if (!leader) return 'var(--surface-track)';
  const accent = s.ringMode === 'concentric' ? rampFor(leader, 0) : accentFor(leader, 0);
  return severityColor(accent, severity);
}

export function buildRingItems(snap: AppSnapshot | null, s: Settings): RingItem[] {
  if (!snap) return [];
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

  for (const quota of barProviders(snap, s)) {
    // no group at all = every window of this provider is hidden on the bar
    const groups = barGroups(quota, s);
    if (s.ringMode === 'concentric') {
      for (const arcs of groups) {
        push(
          quota,
          `${quota.provider}:group`,
          arcs,
          (depth) => rampFor(quota.provider, depth),
          labelWindowOf(arcs)
        );
      }
      continue;
    }

    groups.forEach((group, slot) => {
      // an empty group is the not_logged_in / no-cached-windows placeholder
      const w = group[0] ?? null;
      push(
        quota,
        w ? `${quota.provider}:${w.kind}:${slot}` : `${quota.provider}:none`,
        group,
        () => accentFor(quota.provider, slot),
        w
      );
    });
  }
  return items;
}

class RingStore {
  items = $derived.by(() => buildRingItems(snapshot.value, settings.value));
}

export const rings = new RingStore();
