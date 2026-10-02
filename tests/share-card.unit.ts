import { test, expect } from '@playwright/test';
import {
  CARD_HEIGHT,
  CARD_WIDTH,
  buildShareModel,
  fitFontSize,
  layoutShareCard,
  monthBounds,
  shareFileName,
  topModel,
  type ShareInput,
} from '../src/lib/share-card';
import { defaultSettings } from '../src/lib/settings-defaults';
import type { HistoryRow, ProviderQuota, QuotaWindow } from '../src/lib/types';

const win = (label: string, used: number, primary = false): QuotaWindow => ({
  kind: 'five_hour',
  label,
  windowSeconds: null,
  usedPercent: used,
  resetsAt: null,
  scope: null,
  isPrimary: primary,
});

const quota = (provider: string, name: string, windows: QuotaWindow[], extra: Partial<ProviderQuota> = {}): ProviderQuota => ({
  provider,
  displayName: name,
  plan: null,
  planLabel: null,
  account: { email: 'private.person@example.com', name: 'Private Person' },
  windows,
  fetchedAt: '2026-10-02T00:00:00Z',
  source: 'api',
  status: 'ok',
  error: null,
  credits: null,
  extras: [],
  nextAttemptAt: null,
  ...extra,
});

const row = (model: string | null, tokens: number): HistoryRow => ({
  bucketStart: '2026-10-01T00:00:00',
  provider: 'claude',
  model,
  project: '/home/secret/project',
  inputTokens: 0,
  cacheWriteTokens: 0,
  cacheReadTokens: 0,
  outputTokens: 0,
  reasoningTokens: 0,
  totalTokens: tokens,
  requests: 1,
  estimatedCostUsd: null,
});

const totals = { inputTokens: 1, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 1, reasoningTokens: 0, totalTokens: 1_234_000, requests: 5, estimatedCostUsd: 12.5 };

function input(overrides: Partial<ShareInput> = {}): ShareInput {
  return {
    providers: [
      quota('claude', 'Claude', [win('Weekly', 40), win('5-hour', 73, true)]),
      quota('codex', 'Codex', [win('5-hour', 95, true)]),
    ],
    providerSettings: defaultSettings.providers,
    thresholds: { warn: 70, critical: 90 },
    percentMode: 'used',
    accentOf: (id) => (id === 'claude' ? '#ff5c1a' : '#10a37f'),
    monthLabel: 'October 2026',
    totals,
    rows: [row('opus', 700), row('sonnet', 300), row('opus', 100)],
    hideCost: false,
    theme: 'dark',
    labels: {
      title: 'AI usage', thisMonth: 'This month', tokens: 'Tokens', cost: 'Est. cost', estimate: 'estimate',
      topModel: 'Top model', appName: 'AI Usage Sidebar', costNote: 'estimate only', noData: 'No data',
    },
    format: { tokens: (n) => `${n ?? '-'}t`, cost: (n) => (n == null ? '-' : `$${n}`), percent: (u) => `${Math.round(u)}%` },
    ...overrides,
  };
}

test('one ring per provider with its primary window, severity and accent', () => {
  const model = buildShareModel(input());
  expect(model.rings.map((r) => [r.provider, r.windowLabel, r.fill, r.text, r.severity, r.color])).toEqual([
    ['claude', '5-hour', 73, '73%', 'warn', '#ff5c1a'],
    ['codex', '5-hour', 95, '95%', 'critical', '#10a37f'],
  ]);
});

test('providers that are disabled, signed out or without windows get no ring', () => {
  const model = buildShareModel(
    input({
      providers: [
        quota('claude', 'Claude', [win('5-hour', 10, true)]),
        quota('codex', 'Codex', [win('5-hour', 10, true)], { status: 'not_logged_in' }),
        quota('copilot', 'Copilot', []),
      ],
    }),
  );
  expect(model.rings.map((r) => r.provider)).toEqual(['claude']);
  const off = buildShareModel(input({ providerSettings: { ...defaultSettings.providers, claude: { ...defaultSettings.providers.claude, enabled: false } } }));
  expect(off.rings.map((r) => r.provider)).toEqual(['codex']);
});

test('no e-mail, account name, project path or plan ever reaches the model', () => {
  const text = JSON.stringify(buildShareModel(input()));
  expect(text).not.toContain('example.com');
  expect(text).not.toContain('Private Person');
  expect(text).not.toContain('/home/secret');
});

test('the cost is shown as an estimate, and hiding it removes it entirely', () => {
  const shown = buildShareModel(input());
  expect(shown.stats.find((s) => s.id === 'cost')).toMatchObject({ value: '$12.5', note: 'estimate' });
  const hidden = buildShareModel(input({ hideCost: true }));
  expect(hidden.stats.map((s) => s.id)).toEqual(['tokens', 'topModel']);
  expect(JSON.stringify(hidden)).not.toContain('$12.5');
});

test('an unknown total cost falls back to the known subtotal, or a dash', () => {
  const known = buildShareModel(input({ totals: { ...totals, estimatedCostUsd: null, knownCostUsd: 3 } }));
  expect(known.stats.find((s) => s.id === 'cost')?.value).toBe('$3');
  const none = buildShareModel(input({ totals: null }));
  expect(none.stats.find((s) => s.id === 'cost')?.value).toBe('-');
  expect(none.stats.find((s) => s.id === 'tokens')?.value).toBe('-t');
});

test('the top model sums its rows across buckets and reports its share', () => {
  const top = topModel([row('opus', 700), row('sonnet', 300), row('opus', 100)]);
  expect(top).toMatchObject({ name: 'opus', tokens: 800 });
  expect(top?.share).toBeCloseTo(800 / 1100);
  expect(topModel([row(null, 50), row('', 10)])).toBeNull();
  expect(topModel([])).toBeNull();
  const model = buildShareModel(input());
  expect(model.stats.find((s) => s.id === 'topModel')).toMatchObject({ value: 'opus', note: '73% · 800t' });
});

test('the ring text follows the formatter while the arc always shows the used share', () => {
  const base = input();
  const model = buildShareModel({ ...base, format: { ...base.format, percent: (u) => `${100 - Math.round(u)}% left` } });
  expect(model.rings[0].text).toBe('27% left');
  expect(model.rings[0].fill).toBe(73);
});

// ---------- layout ----------

function layoutFor(rings: number, stats: number) {
  const model = {
    rings: Array.from({ length: rings }, (_, i) => ({ provider: `p${i}`, name: '', windowLabel: '', fill: 0, text: '', color: '', severity: 'normal' as const })),
    stats: Array.from({ length: stats }, (_, i) => ({ id: (['tokens', 'cost', 'topModel'] as const)[i], label: '', value: '', note: null })),
  };
  return layoutShareCard(model);
}

test('the canvas is 1200 x 630', () => {
  const layout = layoutFor(2, 3);
  expect([layout.width, layout.height, CARD_WIDTH, CARD_HEIGHT]).toEqual([1200, 630, 1200, 630]);
});

for (const rings of [1, 2, 3, 4, 5, 6, 8]) {
  test(`${rings} ring(s) stay inside their area and never overlap`, () => {
    const layout = layoutFor(rings, 3);
    expect(layout.rings).toHaveLength(rings);
    const area = layout.ringsArea;
    for (const [i, slot] of layout.rings.entries()) {
      const outer = slot.r + slot.stroke / 2;
      expect(slot.cx - outer, `left ${i}`).toBeGreaterThanOrEqual(area.x - 0.5);
      expect(slot.cx + outer, `right ${i}`).toBeLessThanOrEqual(area.x + area.w + 0.5);
      expect(slot.cy - outer, `top ${i}`).toBeGreaterThanOrEqual(area.y - 0.5);
      expect(slot.windowY, `labels ${i}`).toBeLessThanOrEqual(area.y + area.h + 8);
      for (const v of Object.values(slot)) expect(Number.isFinite(v)).toBe(true);
      for (const other of layout.rings.slice(i + 1)) {
        const dist = Math.hypot(slot.cx - other.cx, slot.cy - other.cy);
        expect(dist, `overlap ${i}`).toBeGreaterThanOrEqual(2 * outer - 0.5);
      }
    }
    expect(area.x + area.w).toBeLessThan(layout.statsArea.x);
    expect(layout.statsArea.x + layout.statsArea.w).toBeLessThanOrEqual(layout.width - layout.pad + 0.5);
  });
}

test('rings are centred in their area', () => {
  const layout = layoutFor(3, 3);
  const area = layout.ringsArea;
  const first = layout.rings[0];
  const last = layout.rings[2];
  const left = first.cx - first.r - first.stroke / 2;
  const right = last.cx + last.r + last.stroke / 2;
  expect(left - area.x).toBeCloseTo(area.x + area.w - right, 0);
});

test('stats stack in equal slots down the right column', () => {
  const three = layoutFor(2, 3);
  expect(three.stats).toHaveLength(3);
  expect(three.stats[1].y - three.stats[0].y).toBeCloseTo(three.statsArea.h / 3);
  const two = layoutFor(2, 2);
  expect(two.stats[1].y - two.stats[0].y).toBeCloseTo(two.statsArea.h / 2);
  for (const s of three.stats) {
    expect(s.labelY).toBeLessThan(s.valueY);
    expect(s.valueY).toBeLessThan(s.noteY);
    expect(s.noteY).toBeLessThanOrEqual(s.y + s.h);
  }
});

test('without stats the rings use the whole width; without rings nothing is laid out', () => {
  const wide = layoutFor(2, 0);
  expect(wide.stats).toEqual([]);
  expect(wide.ringsArea.w).toBeGreaterThan(layoutFor(2, 3).ringsArea.w);
  expect(layoutFor(0, 3).rings).toEqual([]);
});

test('long model names shrink until they fit, but not below the minimum', () => {
  const width = (size: number) => size * 20;
  expect(fitFontSize(width, 1000, 52, 24)).toBe(50);
  expect(fitFontSize(width, 100, 52, 24)).toBe(24);
  expect(fitFontSize(width, 5000, 52, 24)).toBe(52);
});

test('file names and month bounds use the local calendar', () => {
  const now = new Date(2026, 9, 17, 15).getTime();
  expect(shareFileName(now)).toBe('ai-usage-2026-10.png');
  expect(monthBounds(now)).toEqual({ from: new Date(2026, 9, 1).getTime(), to: new Date(2026, 10, 1).getTime() });
  const dec = new Date(2026, 11, 31, 23).getTime();
  expect(monthBounds(dec).to).toBe(new Date(2027, 0, 1).getTime());
});
