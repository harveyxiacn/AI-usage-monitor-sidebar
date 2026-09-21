// Locale-aware formatting helpers. [FRONTEND]
// Everything user-visible goes through t(), so these functions are reactive to
// the language rune just like the templates that call them.
import { forecastLine } from '$lib/forecast';
import { intlLocale, t } from '$lib/i18n/i18n.svelte';
import type { PercentMode, QuotaWindow, Thresholds, WindowKind } from '$lib/types';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const pad2 = (n: number) => String(n).padStart(2, '0');

/** 1.2K / 34.5K / 123K / 3.4M / 1.2B */
export function formatTokens(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—';
  const abs = Math.abs(n);
  const unit = (v: number, suffix: string) =>
    `${Math.abs(v) >= 100 ? Math.round(v) : v.toFixed(1)}${suffix}`;
  if (abs < 1_000) return String(Math.round(n));
  if (abs < 1_000_000) return unit(n / 1_000, 'K');
  if (abs < 1_000_000_000) return unit(n / 1_000_000, 'M');
  return unit(n / 1_000_000_000, 'B');
}

/** $12.34 — null means "model not in the pricing table". */
export function formatCost(usd: number | null | undefined): string {
  if (usd == null || !Number.isFinite(usd)) return '—';
  return `$${usd.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

export function formatInt(n: number): string {
  return n.toLocaleString(intlLocale());
}

/** Clamped 0..100 used percent. */
export function clampPercent(p: number | null | undefined): number {
  if (p == null || !Number.isFinite(p)) return 0;
  return Math.min(100, Math.max(0, p));
}

/** "73% Used" / "已用 73%" — or the remaining variant. */
export function formatPercent(usedPercent: number | null, mode: PercentMode): string {
  if (usedPercent == null) return t('percent.unknown');
  const used = clampPercent(usedPercent);
  return mode === 'remaining'
    ? t('percent.left', { p: Math.round(100 - used) })
    : t('percent.used', { p: Math.round(used) });
}

/** Bare number for the small label under a ring ("73%"). */
export function shortPercent(usedPercent: number | null, mode: PercentMode): string {
  if (usedPercent == null) return '—';
  const used = clampPercent(usedPercent);
  return `${Math.round(mode === 'remaining' ? 100 - used : used)}%`;
}

/**
 * "Resets in 51 min" / "Resets in 2 h 05 m" within 24 h, otherwise an absolute
 * "Resets Thu 12:00 AM" / "9月18日 03:59 重置". null → "—".
 */
export function formatReset(resetsAt: string | null, now: number = Date.now()): string {
  if (!resetsAt) return t('reset.unknown');
  const ts = Date.parse(resetsAt);
  if (!Number.isFinite(ts)) return t('reset.unknown');
  const diff = ts - now;
  if (diff < DAY) {
    const totalMin = Math.max(0, Math.ceil(diff / MINUTE));
    if (totalMin < 60) return t('reset.inMin', { m: totalMin });
    return t('reset.inHourMin', { h: Math.floor(totalMin / 60), m: pad2(totalMin % 60) });
  }
  return t('reset.at', { when: absoluteReset(new Date(ts)) });
}

/** "Thu 12:00 AM" (en) / "9月18日 03:59" (zh) — no comma, matching the design. */
function absoluteReset(d: Date): string {
  const loc = intlLocale();
  if (loc === 'zh-CN') {
    const md = new Intl.DateTimeFormat(loc, { month: 'numeric', day: 'numeric' }).format(d);
    const hm = new Intl.DateTimeFormat(loc, {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(d);
    return `${md} ${hm}`;
  }
  const wd = new Intl.DateTimeFormat(loc, { weekday: 'short' }).format(d);
  const hm = new Intl.DateTimeFormat(loc, {
    hour: 'numeric',
    minute: '2-digit',
    hour12: true,
  }).format(d);
  return `${wd} ${hm}`;
}

/**
 * "Runs out in ~40 min" / "On pace for 82% at reset" — the burn-rate line under
 * a window, or null when the backend sent no forecast. The wording rules live
 * in `forecast.ts`; this only localizes them.
 */
export function formatForecast(
  w: QuotaWindow,
  mode: PercentMode = 'used',
  now: number = Date.now()
): { text: string; tone: 'warn' | 'muted' } | null {
  const line = forecastLine(w, mode, now);
  return line && { text: t(line.key, line.params), tone: line.tone };
}

/** "12 s ago" / "12 秒前" */
export function formatAgo(iso: string | null, now: number = Date.now()): string {
  if (!iso) return t('reset.unknown');
  const ts = Date.parse(iso);
  if (!Number.isFinite(ts)) return t('reset.unknown');
  const diff = Math.max(0, now - ts);
  if (diff < 5_000) return t('ago.now');
  if (diff < MINUTE) return t('ago.seconds', { n: Math.floor(diff / 1000) });
  if (diff < HOUR) return t('ago.minutes', { n: Math.floor(diff / MINUTE) });
  if (diff < DAY) return t('ago.hours', { n: Math.floor(diff / HOUR) });
  return t('ago.days', { n: Math.floor(diff / DAY) });
}

/** Short date/time label for a history bucket, tuned per bucket size. */
export function formatBucket(iso: string, bucket: 'hour' | 'day' | 'week' | 'month'): string {
  const d = new Date(iso);
  if (!Number.isFinite(d.getTime())) return iso;
  const loc = intlLocale();
  switch (bucket) {
    case 'hour':
      return new Intl.DateTimeFormat(loc, { month: 'numeric', day: 'numeric', hour: '2-digit', hour12: false }).format(d);
    case 'month':
      return new Intl.DateTimeFormat(loc, { year: 'numeric', month: 'short' }).format(d);
    default:
      return new Intl.DateTimeFormat(loc, { month: 'numeric', day: 'numeric' }).format(d);
  }
}

// ---------------------------------------------------------------- labels ----

/**
 * "5-hour" / "Weekly"; in the popover the primary session window is called
 * "Current session" like the reference design. Scoped windows always use the
 * plain kind label so "5-hour · Spark" reads correctly.
 */
export type LabelContext = 'bar' | 'popover' | 'dashboard';

export function kindLabel(kind: WindowKind, context: LabelContext, scoped: boolean): string {
  switch (kind) {
    case 'five_hour':
      return context === 'popover' && !scoped ? t('window.fiveHourSession') : t('window.fiveHour');
    case 'seven_day':
      return t('window.weekly');
    default:
      return '';
  }
}

/** Full label for a quota window; `other` keeps whatever the backend sent. */
export function windowLabel(w: QuotaWindow, context: LabelContext = 'popover'): string {
  if (w.kind === 'other') return w.label;
  const base = kindLabel(w.kind, context, w.scope != null);
  return w.scope ? t('window.scoped', { kind: base, scope: w.scope }) : base;
}

// ------------------------------------------------------------ thresholds ----

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
