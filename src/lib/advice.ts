// Presentation rules for the routing advice (`get_routing_advice`), i18n-free so
// `tests/advice.unit.ts` can import them in Node. The advice itself is computed
// by src-tauri/src/advisor/routing.rs. [FRONTEND]
import type { AdvisorWindow, RoutingAdvice } from './types';

export type SwitchAdvice = RoutingAdvice & { from: AdvisorWindow; to: AdvisorWindow };

/** A switch recommendation, the only kind the popover shows. */
export function isSwitch(advice: RoutingAdvice | null | undefined): advice is SwitchAdvice {
  return !!advice && advice.kind === 'switch' && advice.from !== null && advice.to !== null;
}

/** Does this advice concern the quota source `key` (`claude`, `claude@work`)? */
export function adviceInvolves(advice: RoutingAdvice | null | undefined, key: string): boolean {
  return isSwitch(advice) && (advice.from.key === key || advice.to.key === key);
}

/**
 * Coarse "about" figure for a span in minutes: the backend's exact numbers
 * would read as precision the estimate does not have.
 * < 1 h: nearest 5 min; < 3 h: nearest 15 min; otherwise nearest hour
 * (nearest 6 h from two days on).
 */
export function roundMinutes(minutes: number): number {
  if (!Number.isFinite(minutes) || minutes <= 0) return 0;
  if (minutes < 60) return Math.max(5, Math.round(minutes / 5) * 5);
  if (minutes < 180) return Math.round(minutes / 15) * 15;
  if (minutes < 2880) return Math.round(minutes / 60) * 60;
  return Math.round(minutes / 360) * 360;
}

export interface DurationParts { days: number; hours: number; minutes: number }

export function durationParts(minutes: number): DurationParts {
  const m = roundMinutes(minutes);
  return { days: Math.floor(m / 1440), hours: Math.floor((m % 1440) / 60), minutes: m % 60 };
}
