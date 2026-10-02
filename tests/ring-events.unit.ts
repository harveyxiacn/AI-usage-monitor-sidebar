import { test, expect } from '@playwright/test';
import { detectRingEvents, levelsOf, windowKey } from '../src/lib/ring-events';
import { SparkCache, sparkValues } from '../src/lib/quota-spark';
import type { AppSnapshot, QuotaSample, QuotaWindow } from '../src/lib/types';

const th = { warn: 70, critical: 90 };
const win = (kind: QuotaWindow['kind'], usedPercent: number, scope: string | null = null): QuotaWindow => ({
  kind,
  label: kind,
  windowSeconds: null,
  usedPercent,
  resetsAt: null,
  scope,
  isPrimary: false,
});
const snap = (...ws: QuotaWindow[]) =>
  ({
    providers: [{ provider: 'claude', windows: ws }],
  }) as unknown as AppSnapshot;
const levels = (...ws: QuotaWindow[]) => levelsOf(snap(...ws), th);
const key = windowKey('claude', { kind: 'five_hour', scope: null });

test('the first snapshot never animates', () => {
  expect(detectRingEvents(null, levels(win('five_hour', 95)))).toEqual([]);
});

test('crossing into warn and critical fires once, staying put does not', () => {
  const a = levels(win('five_hour', 60));
  const b = levels(win('five_hour', 75));
  const c = levels(win('five_hour', 92));
  expect(detectRingEvents(a, b)).toEqual([{ key, kind: 'warn' }]);
  expect(detectRingEvents(b, c)).toEqual([{ key, kind: 'critical' }]);
  expect(detectRingEvents(a, c)).toEqual([{ key, kind: 'critical' }]);
  expect(detectRingEvents(b, b)).toEqual([]);
  expect(detectRingEvents(b, levels(win('five_hour', 80)))).toEqual([]);
  // leaving a severity is not an alarm
  expect(detectRingEvents(c, levels(win('five_hour', 71)))).toEqual([]);
});

test('a sharp drop is a reset, a small one is not', () => {
  expect(detectRingEvents(levels(win('five_hour', 92)), levels(win('five_hour', 3)))).toEqual([{ key, kind: 'reset' }]);
  expect(detectRingEvents(levels(win('five_hour', 50)), levels(win('five_hour', 20)))).toEqual([{ key, kind: 'reset' }]);
  expect(detectRingEvents(levels(win('five_hour', 50)), levels(win('five_hour', 40)))).toEqual([]);
  // too little before the drop to call it a reset
  expect(detectRingEvents(levels(win('five_hour', 28)), levels(win('five_hour', 1)))).toEqual([]);
});

test('windows are matched per kind and scope; new windows are ignored', () => {
  const prev = levels(win('five_hour', 10), win('seven_day', 50, 'opus'));
  const next = levels(win('five_hour', 10), win('seven_day', 75, 'opus'), win('seven_day', 99));
  expect(detectRingEvents(prev, next)).toEqual([{ key: windowKey('claude', { kind: 'seven_day', scope: 'opus' }), kind: 'warn' }]);
});

const sample = (minAgo: number, used: number, kind: QuotaSample['kind'] = 'five_hour', scope: string | null = null): QuotaSample => ({
  provider: 'claude',
  kind,
  scope,
  usedPercent: used,
  resetsAt: null,
  plan: null,
  ts: new Date(NOW - minAgo * 60_000).toISOString(),
});
const NOW = Date.parse('2026-09-15T12:00:00Z');

test('sparkValues picks one window, last 24 h, oldest first', () => {
  const samples = [
    sample(10, 30),
    sample(120, 20),
    sample(30 * 60, 99), // older than 24 h
    sample(60, 25, 'seven_day'),
    sample(60, 5, 'five_hour', 'opus'),
  ];
  expect(sparkValues(samples, 'claude', { kind: 'five_hour', scope: null }, NOW)).toEqual([20, 30]);
  expect(sparkValues(samples, 'claude', { kind: 'five_hour', scope: 'opus' }, NOW)).toEqual([5]);
  expect(sparkValues(samples, 'codex', { kind: 'five_hour', scope: null }, NOW)).toEqual([]);
});

test('sparkValues thins long histories and keeps first and newest', () => {
  const samples = Array.from({ length: 300 }, (_, i) => sample(1400 - i * 4, i));
  const v = sparkValues(samples, 'claude', { kind: 'five_hour', scope: null }, NOW, 48);
  expect(v).toHaveLength(48);
  expect(v[0]).toBe(0);
  expect(v[47]).toBe(299);
});

test('SparkCache serves from cache within the TTL and dedupes in-flight loads', async () => {
  const cache = new SparkCache(1000);
  let calls = 0;
  const load = async () => {
    calls++;
    return [sample(1, 1)];
  };
  const [a, b] = await Promise.all([cache.get('claude', load, 0), cache.get('claude', load, 0)]);
  expect(a).toBe(b);
  expect(calls).toBe(1);
  await cache.get('claude', load, 999);
  expect(calls).toBe(1);
  await cache.get('claude', load, 1000);
  expect(calls).toBe(2);
});
