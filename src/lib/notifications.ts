// Pure helpers for the notification settings panel. [FRONTEND]
import type { Thresholds } from './types';

/** Valid range of a threshold, in whole percent. */
export const THRESHOLD_MIN = 1;
export const THRESHOLD_MAX = 100;

const clampWhole = (value: number, fallback: number) =>
  Number.isFinite(value) ? Math.min(THRESHOLD_MAX, Math.max(THRESHOLD_MIN, Math.round(value))) : fallback;

/**
 * Move one handle of the warn/critical pair and keep `warn < critical`.
 * The handle that was *not* moved gives way only when it has to, so dragging
 * one slider never silently rewrites the other unless they would cross.
 */
export function moveThreshold(current: Thresholds, which: keyof Thresholds, value: number): Thresholds {
  let warn = clampWhole(current.warn, 70);
  let critical = clampWhole(current.critical, 90);
  if (which === 'warn') {
    warn = clampWhole(value, warn);
    if (warn >= critical) {
      critical = Math.min(THRESHOLD_MAX, warn + 1);
      warn = Math.min(warn, critical - 1);
    }
  } else {
    critical = clampWhole(value, critical);
    if (critical <= warn) {
      warn = Math.max(THRESHOLD_MIN, critical - 1);
      critical = Math.max(critical, warn + 1);
    }
  }
  return { warn, critical };
}

/** Widths (percent of the track) of the green / amber / red zones of a ring. */
export function thresholdZones(t: Thresholds): { ok: number; warn: number; critical: number } {
  const warn = Math.min(THRESHOLD_MAX, Math.max(0, t.warn));
  const critical = Math.min(THRESHOLD_MAX, Math.max(warn, t.critical));
  return { ok: warn, warn: critical - warn, critical: THRESHOLD_MAX - critical };
}

export type WebhookUrlProblem = 'empty' | 'not-a-url' | 'not-https';

/** `null` when the backend would accept the URL (https with a host). */
export function webhookUrlProblem(value: string): WebhookUrlProblem | null {
  const text = value.trim();
  if (!text) return 'empty';
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return 'not-a-url';
  }
  if (url.protocol !== 'https:') return 'not-https';
  return url.hostname ? null : 'not-a-url';
}

/** Only the host of a webhook URL is ever shown: the rest may be a secret. */
export function webhookHost(value: string): string {
  try {
    return new URL(value.trim()).hostname;
  } catch {
    return '';
  }
}

export type FocusStatus = { kind: 'off' } | { kind: 'forever' } | { kind: 'until'; until: number };

/** Mirrors `focus::is_active`: 0 = off, -1 = until turned off, else a deadline. */
export function focusStatus(focusUntil: number, now: number): FocusStatus {
  if (focusUntil === -1) return { kind: 'forever' };
  if (focusUntil > now) return { kind: 'until', until: focusUntil };
  return { kind: 'off' };
}
