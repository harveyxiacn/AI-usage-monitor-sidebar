import { test, expect } from '@playwright/test';
import { PRICING_STALE_DAYS, pricingStaleness } from '../src/lib/pricing-age';

const NOW = Date.parse('2026-10-02T10:00:00Z');
const daysAgo = (d: number) => new Date(NOW - d * 86_400_000).toISOString();

test('a recent table says nothing', () => {
  expect(pricingStaleness(daysAgo(3), null, NOW)).toBeNull();
  expect(pricingStaleness(daysAgo(PRICING_STALE_DAYS), null, NOW)).toBeNull();
});

test('a table older than 60 days quotes its date', () => {
  const r = pricingStaleness('2026-07-01T00:00:00Z', null, NOW);
  expect(r).toEqual({ date: '2026-07-01', ageDays: 93 });
  expect(pricingStaleness(daysAgo(PRICING_STALE_DAYS + 1), null, NOW)).not.toBeNull();
});

test('a recent successful check of the source refreshes an old revision', () => {
  expect(pricingStaleness(daysAgo(200), daysAgo(1), NOW)).toBeNull();
  expect(pricingStaleness(daysAgo(200), daysAgo(90), NOW)?.date).toBe(daysAgo(90).slice(0, 10));
});

test('unknown or unparsable dates stay silent', () => {
  expect(pricingStaleness(null, null, NOW)).toBeNull();
  expect(pricingStaleness(undefined, 'nope', NOW)).toBeNull();
});
