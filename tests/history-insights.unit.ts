import { test, expect } from '@playwright/test';
import {
  budgetProgress,
  calendarWeeks,
  heatLevel,
  heatThresholds,
  historyRange,
  metricOf,
  sessionsCsv,
} from '../src/lib/history';
import type { CalendarDay, SessionRow } from '../src/lib/types';

/** Run `body` with a fixed local time zone, whatever the host is set to. */
function inTimeZone(tz: string, body: () => void) {
  const original = process.env.TZ;
  process.env.TZ = tz;
  try {
    body();
  } finally {
    if (original === undefined) delete process.env.TZ;
    else process.env.TZ = original;
  }
}

function day(date: string, totalTokens: number, estimatedCostUsd: number | null = 0): CalendarDay {
  return {
    date,
    inputTokens: totalTokens, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0,
    reasoningTokens: 0, totalTokens, requests: 1, estimatedCostUsd,
  };
}

test('the calendar grid is Monday-first and marks days outside the range', () => {
  inTimeZone('Asia/Singapore', () => {
    // 2026-09-16 is a Wednesday, 2026-09-21 the Monday after
    const weeks = calendarWeeks(
      new Date(2026, 8, 16).getTime(),
      new Date(2026, 8, 22).getTime()
    );
    expect(weeks).toHaveLength(2);
    expect(weeks[0]).toEqual([null, null, '2026-09-16', '2026-09-17', '2026-09-18', '2026-09-19', '2026-09-20']);
    expect(weeks[1]).toEqual(['2026-09-21', null, null, null, null, null, null]);
    expect(calendarWeeks(0, 0)).toEqual([]);
    expect(calendarWeeks(10, 5)).toEqual([]);
  });
});

test('calendar days survive both daylight saving transitions', () => {
  inTimeZone('America/New_York', () => {
    // March 2026: the clocks jump forward on Sunday the 8th
    const spring = historyRange('custom', '2026-03-01', '2026-03-31')!;
    const weeks = calendarWeeks(spring.from, spring.to);
    const dates = weeks.flat().filter((date): date is string => date !== null);
    expect(weeks.every((week) => week.length === 7)).toBe(true);
    expect(weeks).toHaveLength(6);
    expect(dates).toHaveLength(31);
    expect(new Set(dates).size).toBe(31);
    expect(dates[0]).toBe('2026-03-01');
    expect(dates.at(-1)).toBe('2026-03-31');
    // the 23-hour day keeps its own cell, on the right weekday
    expect(weeks[1]).toEqual(['2026-03-02', '2026-03-03', '2026-03-04', '2026-03-05', '2026-03-06', '2026-03-07', '2026-03-08']);

    // November 2026: the clocks fall back on Sunday the 1st
    const fall = historyRange('custom', '2026-10-26', '2026-11-08')!;
    const fallWeeks = calendarWeeks(fall.from, fall.to);
    expect(fallWeeks).toHaveLength(2);
    expect(fallWeeks.flat().filter(Boolean)).toHaveLength(14);
    expect(fallWeeks[0][6]).toBe('2026-11-01');
  });
});

test('a 26-week window is always 26 columns of 7 days across a DST change', () => {
  inTimeZone('Europe/Berlin', () => {
    // ends the day after 2026-04-01, so the window spans the March transition
    const to = new Date(2026, 3, 2).getTime();
    const from = new Date(2026, 3, 2);
    from.setDate(from.getDate() - 26 * 7);
    const weeks = calendarWeeks(from.getTime(), to);
    const dates = weeks.flat().filter(Boolean);
    expect(dates).toHaveLength(26 * 7);
    expect(new Set(dates).size).toBe(26 * 7);
    expect(weeks.every((week) => week.length === 7)).toBe(true);
  });
});

test('heat levels are quartiles of the non-zero days, with the peak always darkest', () => {
  const thresholds = heatThresholds([0, 1, 2, 3, 4, 5, 6, 7, 8, 0]);
  expect(thresholds).toEqual([3, 5, 7]);
  expect([1, 2, 3, 4, 5, 6, 7, 8].map((v) => heatLevel(v, thresholds))).toEqual([1, 1, 2, 2, 3, 3, 4, 4]);
  expect(heatLevel(0, thresholds)).toBe(0);
  // an unknown cost is not zero; the caller renders it separately
  expect(heatLevel(null, thresholds)).toBe(0);

  // one outlier must not flatten the rest into a single shade
  const skewed = heatThresholds([1, 1, 2, 1_000_000]);
  expect(heatLevel(1, skewed)).toBeLessThan(heatLevel(1_000_000, skewed));
  expect(heatLevel(1_000_000, skewed)).toBe(4);
  // a single day of activity still reaches the darkest step
  expect(heatLevel(42, heatThresholds([42]))).toBe(4);
  expect(heatThresholds([])).toEqual([0, 0, 0]);

  expect(metricOf(day('2026-09-20', 500, 1.25), 'tokens')).toBe(500);
  expect(metricOf(day('2026-09-20', 500, 1.25), 'cost')).toBe(1.25);
  expect(metricOf(day('2026-09-20', 500, null), 'cost')).toBeNull();
});

test('the monthly budget measures a DST month and extrapolates from the elapsed share', () => {
  inTimeZone('America/New_York', () => {
    const now = new Date(2026, 2, 15, 12, 0).getTime();
    const days = [
      day('2026-02-28', 10, 99), // previous month — ignored
      day('2026-03-02', 10, 10),
      day('2026-03-10', 10, 5),
      day('2026-03-20', 10, 42), // still in the future — not counted yet
    ];
    const p = budgetProgress(days, 100, now)!;
    expect(p.monthEnd - p.monthStart).toBe(743 * 3_600_000); // 31 days minus the lost hour
    expect(p.series).toHaveLength(15);
    expect(p.series[0]).toEqual({ date: '2026-03-01', cumulativeUsd: 0 });
    expect(p.series[1].cumulativeUsd).toBe(10);
    expect(p.series.at(-1)).toEqual({ date: '2026-03-15', cumulativeUsd: 15 });
    expect(p.spentUsd).toBe(15);
    expect(p.incomplete).toBe(false);
    expect(p.percentUsed).toBeCloseTo(15, 6);
    // 14 days 12 h elapsed, one of those hours skipped by the spring forward
    expect(p.elapsed).toBeCloseTo(347 / 743, 6);
    expect(p.paceUsd).toBeCloseTo(15 / (347 / 743), 6);

    // an unpriced day makes the month a lower bound rather than a wrong number
    const unknown = budgetProgress([...days, day('2026-03-05', 10, null)], 100, now)!;
    expect(unknown.incomplete).toBe(true);
    expect(unknown.spentUsd).toBe(15);

    // no budget, no burn-up
    for (const budget of [0, -5, Number.NaN]) expect(budgetProgress(days, budget, now)).toBeNull();
    // an empty month is 0 %, never a division by zero
    const quiet = budgetProgress([], 50, new Date(2026, 2, 1, 0, 0).getTime())!;
    expect(quiet.spentUsd).toBe(0);
    expect(quiet.percentUsed).toBe(0);
    expect(quiet.paceUsd).toBe(0);
    expect(quiet.series).toEqual([{ date: '2026-03-01', cumulativeUsd: 0 }]);
  });
});

test('over-budget months report more than 100 % without clamping', () => {
  inTimeZone('Asia/Singapore', () => {
    const now = new Date(2026, 8, 10, 0, 0).getTime();
    const p = budgetProgress([day('2026-09-02', 1, 120)], 100, now)!;
    expect(p.percentUsed).toBeCloseTo(120, 6);
    expect(p.paceUsd).toBeGreaterThan(p.spentUsd);
  });
});

test('the session CSV exports counters and identifiers only', () => {
  const row: SessionRow = {
    sessionId: 'thread-1,alpha',
    provider: 'codex',
    project: 'C:\\团队\\comma, quote" project',
    firstTs: '2026-09-20T09:00:00+08:00',
    lastTs: '2026-09-20T11:30:00+08:00',
    durationMs: 9_000_000,
    models: ['gpt-5.3-codex', 'gpt-5.3-codex-spark'],
    inputTokens: 100, cacheWriteTokens: 20, cacheReadTokens: 30, outputTokens: 40,
    reasoningTokens: 10, totalTokens: 190, requests: 7, estimatedCostUsd: null,
  };
  const csv = sessionsCsv([row]);
  const [header, first] = csv.split('\r\n');
  expect(header).toBe(
    'session_id,provider,project,first_activity,last_activity,duration_ms,models,input_tokens,cache_write_tokens,cache_read_tokens,output_tokens,reasoning_tokens,total_tokens,requests,estimated_cost_usd,known_cost_usd,unpriced_requests'
  );
  expect(first).toContain('"thread-1,alpha",codex,');
  // the exact path survives, quoted; the cost stays empty rather than 0
  expect(first).toContain('"C:\\团队\\comma, quote"" project"');
  expect(first).toContain('gpt-5.3-codex gpt-5.3-codex-spark');
  expect(first.endsWith(',190,7,,,')).toBe(true);
  expect(csv.endsWith('\r\n')).toBe(true);
  // a priced session renders four decimals, like the bucket export
  expect(sessionsCsv([{ ...row, estimatedCostUsd: 1.5 }])).toContain(',190,7,1.5000');
  expect(sessionsCsv([])).toBe(`${header}\r\n`);
});
