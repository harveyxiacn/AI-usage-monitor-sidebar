import { test, expect } from '@playwright/test';
import { nextTickDelay, resetParts } from '../src/lib/countdown';

const NOW = Date.parse('2026-10-02T10:00:00Z');
const at = (ms: number) => new Date(NOW + ms).toISOString();
const MIN = 60_000;
const HOUR = 60 * MIN;

test('resetParts counts minutes, rounding up, inside the first hour', () => {
  expect(resetParts(at(51 * MIN), NOW)).toEqual({ kind: 'min', m: 51 });
  expect(resetParts(at(30_000), NOW)).toEqual({ kind: 'min', m: 1 });
  expect(resetParts(at(-5_000), NOW)).toEqual({ kind: 'min', m: 0 });
});

test('resetParts switches to hours and minutes up to 24 h', () => {
  expect(resetParts(at(2 * HOUR + 5 * MIN), NOW)).toEqual({ kind: 'hourMin', h: 2, m: 5 });
  expect(resetParts(at(23 * HOUR + 59 * MIN), NOW)).toEqual({ kind: 'hourMin', h: 23, m: 59 });
});

test('resetParts keeps the absolute moment and adds a floored d/h relative past 24 h', () => {
  const ms = (3 * 24 + 4) * HOUR + 59 * MIN;
  expect(resetParts(at(ms), NOW)).toEqual({ kind: 'at', ts: NOW + ms, d: 3, h: 4 });
  expect(resetParts(at(24 * HOUR), NOW)).toEqual({ kind: 'at', ts: NOW + 24 * HOUR, d: 1, h: 0 });
});

test('resetParts is unknown for missing or unparsable input', () => {
  expect(resetParts(null, NOW)).toEqual({ kind: 'unknown' });
  expect(resetParts('', NOW)).toEqual({ kind: 'unknown' });
  expect(resetParts('soon', NOW)).toEqual({ kind: 'unknown' });
});

test('nextTickDelay ticks every second while a fetch is under a minute old', () => {
  expect(nextTickDelay(NOW, [NOW + 5 * HOUR], NOW - 20_000)).toBe(1000);
});

test('nextTickDelay waits for the next minute boundary of the nearest anchor', () => {
  // reset in 10 min 20 s: the label next changes 20 s from now
  expect(nextTickDelay(NOW, [NOW + 10 * MIN + 20_000], NOW - 10 * MIN)).toBe(20_050);
  // fetched 90 s ago: "1 min ago" becomes "2 min ago" in 30 s
  expect(nextTickDelay(NOW, [], NOW - 90_000)).toBe(30_050);
  // the nearest of several anchors wins
  expect(nextTickDelay(NOW, [NOW + 45_000 + 5 * MIN, NOW + 12_000 + MIN], NOW - 5 * MIN)).toBe(12_050);
});

test('nextTickDelay stays within 250 ms .. 1 min', () => {
  expect(nextTickDelay(NOW, [], null)).toBe(MIN);
  expect(nextTickDelay(NOW, [NOW + MIN], NOW - 2 * MIN)).toBe(MIN);
  expect(nextTickDelay(NOW, [NOW + 1], NOW - 2 * MIN)).toBeGreaterThanOrEqual(250);
  expect(nextTickDelay(NOW, [Number.NaN, undefined], NaN)).toBe(MIN);
});
