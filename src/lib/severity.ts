// Threshold arithmetic, split out of `format.ts` so it can be imported (and
// unit-tested) without pulling in the i18n catalogues. [FRONTEND]
// `format.ts` re-exports everything here, so existing imports keep working.
import type { Thresholds } from './types';

/** Clamped 0..100 used percent. */
export function clampPercent(p: number | null | undefined): number {
  if (p == null || !Number.isFinite(p)) return 0;
  return Math.min(100, Math.max(0, p));
}

export type Severity = 'normal' | 'warn' | 'critical';

export function severityOf(usedPercent: number | null, th: Thresholds): Severity {
  if (usedPercent == null) return 'normal';
  const p = clampPercent(usedPercent);
  if (p >= th.critical) return 'critical';
  if (p >= th.warn) return 'warn';
  return 'normal';
}

/** Accent unless the window crossed a threshold, then amber / red. */
export function severityColor(accent: string, severity: Severity): string {
  if (severity === 'critical') return 'var(--critical)';
  if (severity === 'warn') return 'var(--warn)';
  return accent;
}

const SEVERITY_RANK: Record<Severity, number> = { normal: 0, warn: 1, critical: 2 };

export function worstSeverity(list: Severity[]): Severity {
  return list.reduce<Severity>((acc, s) => (SEVERITY_RANK[s] > SEVERITY_RANK[acc] ? s : acc), 'normal');
}
