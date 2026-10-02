import { expect, test } from '@playwright/test';
import { adviceInvolves, durationParts, isSwitch, roundMinutes } from '../src/lib/advice';
import type { AdvisorWindow, RoutingAdvice } from '../src/lib/types';

const win = (key: string): AdvisorWindow => ({
  key, provider: key.split('@')[0], displayName: key, kind: 'five_hour', label: '5-hour', usedPercent: 50, remainingPercent: 50,
  resetsInMin: 60, resetsAtMs: 0, safeMinutes: null, projectedPercentAtReset: null, confidence: null,
});
const advice = (over: Partial<RoutingAdvice> = {}): RoutingAdvice => ({
  kind: 'switch', from: win('claude'), to: win('codex'), useTargetMinutes: 120, confidence: 'high', basis: [], ...over,
});

test('only a complete switch recommendation counts as a switch', () => {
  expect(isSwitch(advice())).toBe(true);
  expect(isSwitch(advice({ kind: 'no_switch', from: null, to: null }))).toBe(false);
  expect(isSwitch(advice({ to: null }))).toBe(false);
  expect(isSwitch(null)).toBe(false);
  expect(isSwitch(undefined)).toBe(false);
});

test('the popover hint is for the two providers involved, not for a bystander', () => {
  const a = advice();
  expect(adviceInvolves(a, 'claude')).toBe(true);
  expect(adviceInvolves(a, 'codex')).toBe(true);
  expect(adviceInvolves(a, 'claude@work')).toBe(false);
  expect(adviceInvolves(null, 'claude')).toBe(false);
  expect(adviceInvolves(advice({ kind: 'no_switch', from: null, to: null }), 'claude')).toBe(false);
});

test('durations read as "about", not as exact minutes', () => {
  expect(roundMinutes(0)).toBe(0);
  expect(roundMinutes(2)).toBe(5);
  expect(roundMinutes(27)).toBe(25);
  expect(roundMinutes(117)).toBe(120);
  expect(roundMinutes(125)).toBe(120);
  expect(roundMinutes(250)).toBe(240);
  expect(roundMinutes(3000)).toBe(2880);
  expect(roundMinutes(Number.NaN)).toBe(0);
  expect(durationParts(135)).toEqual({ days: 0, hours: 2, minutes: 15 });
  expect(durationParts(1440)).toEqual({ days: 1, hours: 0, minutes: 0 });
  expect(durationParts(20)).toEqual({ days: 0, hours: 0, minutes: 20 });
});
