import { test as base, expect, type Page } from '@playwright/test';
import { mockSettings, mockSnapshot } from '../src/lib/mock';
import type { HistoryResult, QuotaHistoryQuery, QuotaSample, TokenTotals } from '../src/lib/types';

const START = Date.parse('2026-09-22T00:00:00Z');
const RANGE = { from: START, to: START + 2 * 3_600_000 };

function samples(provider = 'codex'): QuotaSample[] {
  return [[0, 20], [30, 30], [61, 2], [65, 7], [70, 7]].map(([minute, usedPercent]) => ({
    provider, kind: 'five_hour', scope: null, usedPercent, plan: 'Pro',
    ts: new Date(START + minute * 60_000).toISOString(),
    resetsAt: minute < 60 ? '2026-09-22T01:00:00Z' : '2026-09-22T06:00:00Z',
  }));
}

const total = (tokens: number): TokenTotals => ({
  inputTokens: tokens, outputTokens: 0, reasoningTokens: 0,
  cacheReadTokens: 0, cacheWriteTokens: 0, totalTokens: tokens,
  requests: 1, estimatedCostUsd: 0, knownCostUsd: 0, unpricedRequests: 0,
});

function historyResult(): HistoryResult {
  const variants = [
    ['codex', 'gpt-6-astra', 'medium'], ['codex', 'gpt-6-astra', 'ultra'],
    ['codex', 'gpt-5.6-sol', 'xhigh'], ['claude', 'claude-fable', 'high'],
    ['claude', 'claude-opus', null],
  ];
  return {
    rows: variants.map(([provider, model, reasoningEffort], i) => ({
      ...total((i + 1) * 100), bucketStart: new Date(START).toISOString(),
      provider: provider!, model, reasoningEffort, project: null,
    })),
    totals: { ...total(1500), requests: 5 },
    byProvider: { codex: { ...total(600), requests: 3 }, claude: { ...total(900), requests: 2 } },
    projects: [], costApproximate: false,
  };
}

interface Pending {
  query: QuotaHistoryQuery;
  resolve: (value: QuotaSample[]) => void;
  reject: (error: Error) => void;
}
interface Backend {
  value: QuotaSample[];
  defer: boolean;
  pending: Pending[];
  commands: string[];
  queries: QuotaHistoryQuery[];
  historyQueries: Array<{ groupByModel: boolean }>;
}

const test = base.extend<{ backend: Backend }>({
  backend: async ({ page }, use) => {
    const backend: Backend = { value: samples(), defer: false, pending: [], commands: [], queries: [], historyQueries: [] };
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    let chartModuleUrl = '';
    page.on('request', (request) => {
      if (request.url().includes('/chart__js.js?v=')) chartModuleUrl = request.url();
    });
    await page.exposeBinding('__quotaChartModuleUrl', () => chartModuleUrl);
    const time = new Date('2026-09-22T10:00:00Z');
    await page.clock.install({ time });
    await page.clock.pauseAt(time);
    let listenerId = 0;
    await page.exposeBinding('__quotaTestInvoke', (_source, command: string, args: { query: QuotaHistoryQuery & { groupByModel: boolean } }) => {
      backend.commands.push(command);
      switch (command) {
        case 'get_quota_history':
          backend.queries.push(args.query);
          if (!backend.defer) return structuredClone(backend.value);
          return new Promise<QuotaSample[]>((resolve, reject) => backend.pending.push({ query: args.query, resolve, reject }));
        case 'get_usage_history': backend.historyQueries.push(args.query); return historyResult();
        case 'get_usage_calendar': return { days: [], slots: [], totals: total(0) };
        case 'plugin:event|listen': return ++listenerId;
        case 'plugin:event|unlisten': return;
        default: throw new Error(`Unexpected Tauri command in quota history test: ${command}`);
      }
    });
    await page.addInitScript(() => {
      let callbackId = 0;
      let settled = 0;
      const callbacks = new Map<number, (value: unknown) => void>();
      const invoke = Reflect.get(window, '__quotaTestInvoke') as (command: string, args: unknown) => Promise<unknown>;
      Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
        transformCallback: (callback: (value: unknown) => void) => { callbacks.set(++callbackId, callback); return callbackId; },
        invoke: async (command: string, args: unknown) => {
          try { return await invoke(command, args); }
          finally { if (command === 'get_quota_history') document.documentElement.dataset.settledQuota = String(++settled); }
        },
      } });
      Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } });
    });
    await page.route('**/quota-history-fixture', (route) => route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><html data-win="dashboard" data-theme="dark"><body><main id="fixture" style="padding:24px"></main></body></html>',
    }));
    await use(backend);
    expect(errors).toEqual([]);
  },
});

async function mount(page: Page, fullHistory = false, live = false) {
  await page.goto('/quota-history-fixture');
  await page.evaluate(async ({ fullHistory, live, initialRange, settingsValue, snapshotValue }) => {
    const runtimePath = '/node_modules/.vite/deps/svelte.js';
    const storesPath = '/node_modules/.vite/deps/svelte_store.js';
    const settingsPath = '/src/lib/stores/settings.svelte.ts';
    const snapshotPath = '/src/lib/stores/snapshot.svelte.ts';
    const componentPath = fullHistory
      ? '/src/lib/components/dashboard/HistoryTab.svelte'
      : '/src/lib/components/dashboard/QuotaHistoryPanel.svelte';
    const cssPath = '/src/app.css';
    const [{ mount, unmount }, { writable, fromStore }, { settings }, { snapshot }, { default: Component }] = await Promise.all([
      import(/* @vite-ignore */ runtimePath), import(/* @vite-ignore */ storesPath),
      import(/* @vite-ignore */ settingsPath), import(/* @vite-ignore */ snapshotPath),
      import(/* @vite-ignore */ componentPath), import(/* @vite-ignore */ cssPath),
    ]);
    settings.value = settingsValue;
    snapshot.value = snapshotValue;
    const state = writable({ range: initialRange, provider: 'codex', live, themeKey: 'dark' });
    const reactive = fromStore(state);
    const props = Object.fromEntries(['range', 'provider', 'live', 'themeKey'].map((key) => [key, undefined]));
    for (const key of Object.keys(props)) Object.defineProperty(props, key, { get: () => reactive.current[key] });
    const instance = mount(Component, { target: document.querySelector('#fixture')!, props });
    Reflect.set(window, '__setQuotaProps', (patch: Record<string, unknown>) => state.update((value: Record<string, unknown>) => ({ ...value, ...patch })));
    Reflect.set(window, '__unmountQuotaFixture', () => unmount(instance));
  }, { fullHistory, live, initialRange: RANGE, settingsValue: { ...mockSettings, percentMode: 'used', language: 'en' }, snapshotValue: mockSnapshot });
}

async function setProps(page: Page, patch: Record<string, unknown>) {
  await page.evaluate((value) => Reflect.get(window, '__setQuotaProps')(value), patch);
}

async function chartState(page: Page, selector = '.quota-chart canvas') {
  return page.evaluate(async (selector) => {
    // Match the component's exact Vite dependency URL. Omitting its version
    // query would load a second Chart class with a separate instance registry.
    const path = await Reflect.get(window, '__quotaChartModuleUrl')();
    if (!path) throw new Error('Component has not loaded Chart.js');
    const { Chart } = await import(/* @vite-ignore */ path);
    const canvas = document.querySelector(selector);
    const chart = canvas ? Chart.getChart(canvas) : undefined;
    if (!chart) throw new Error('Chart missing: ' + JSON.stringify({ path, canvas: !!canvas, instances: Object.keys(Chart.instances) }));
    const dataset = chart.data.datasets[0];
    return {
      id: chart.id, type: chart.config.type,
      min: chart.options.scales.y.min, max: chart.options.scales.y.max,
      xMin: chart.scales.x.min, xMax: chart.scales.x.max,
      stepped: dataset.stepped, data: dataset.data,
      labels: chart.data.datasets.map((item: { label: string }) => item.label),
      values: chart.data.datasets.map((item: { data: unknown[] }) => item.data),
      resetDash: dataset.segment?.borderDash({ p1DataIndex: 2 }),
      ordinaryDash: dataset.segment?.borderDash({ p1DataIndex: 1 }) ?? null,
    };
  }, selector);
}

test('quota curve and change records show exact percentage points across a reset', async ({ page, backend }) => {
  await mount(page);
  const panel = page.getByRole('region', { name: 'Quota history', exact: true });
  await expect(panel.locator('dd')).toHaveText(['7%', '15 pp', '1', '5']);
  await expect(panel.locator('tbody tr')).toHaveCount(4);
  await expect(panel.locator('tbody tr').nth(0).locator('td').nth(1)).toHaveText('7%');
  await expect(panel.locator('tbody tr').nth(0).locator('td').nth(2)).toHaveText('93%');
  await expect(panel.locator('tbody tr').nth(0).locator('td').nth(3)).toHaveText('+5 pp');
  await expect(panel.locator('tbody tr').nth(1)).toContainText('-28 pp');
  await expect(panel.locator('tbody tr').nth(1)).toContainText('Reset / new cycle');
  await expect(panel.locator('tbody tr').nth(3)).toContainText('First sample in range');
  await expect(panel.getByText('Unchanged', { exact: true })).toHaveCount(0);
  await expect(panel.getByText('Showing 4 of 4 records', { exact: true })).toBeVisible();
  await expect.poll(() => chartState(page)).toMatchObject({
    type: 'line', min: 0, max: 100, stepped: 'before', resetDash: [4, 4], ordinaryDash: null,
    data: samples().map((sample) => ({ x: Date.parse(sample.ts), y: sample.usedPercent })),
  });
  expect(backend.queries).toEqual([{ from: new Date(RANGE.from).toISOString(), to: new Date(RANGE.to).toISOString(), provider: 'codex' }]);
});

test('remaining preference rebuilds the curve while keeping used-change semantics', async ({ page, backend }) => {
  await mount(page);
  await expect(page.locator('.quota-stats dd')).toHaveText(['7%', '15 pp', '1', '5']);
  const initialChart = await chartState(page);
  await page.evaluate(async () => {
    const path = '/src/lib/stores/settings.svelte.ts';
    const { settings } = await import(/* @vite-ignore */ path);
    settings.value = { ...settings.value, percentMode: 'remaining' };
  });
  await expect(page.locator('.quota-stats dd')).toHaveText(['93%', '15 pp', '1', '5']);
  await expect(page.locator('.quota-stats dt').first()).toHaveText('Latest remaining in range');
  await expect.poll(() => chartState(page)).toMatchObject({
    labels: ['Remaining'], data: samples().map((sample) => ({ x: Date.parse(sample.ts), y: 100 - sample.usedPercent })),
  });
  expect((await chartState(page))!.id).not.toBe(initialChart!.id);
  expect(backend.queries).toHaveLength(1);
});

test('a single observation uses a short timestamp range around its actual date', async ({ page, backend }) => {
  backend.value = [samples()[0]];
  await mount(page);
  await expect(page.locator('.quota-stats dd')).toHaveText(['20%', '0 pp', '0', '1']);
  const chart = await chartState(page);
  expect(chart!.xMin).toBeLessThan(START);
  expect(chart!.xMax).toBeGreaterThan(START);
  expect(chart!.xMax - chart!.xMin).toBeLessThan(86_400_000);
  expect(chart!.data).toEqual([{ x: START, y: 20 }]);
});

test('a deadline update before the old reset remains visible without counting a reset', async ({ page, backend }) => {
  backend.value = [samples()[0], {
    ...samples()[0], ts: '2026-09-22T00:30:00Z', resetsAt: '2026-09-22T06:00:00Z',
  }];
  await mount(page);
  await expect(page.locator('.quota-stats dd')).toHaveText(['20%', '0 pp', '0', '2']);
  await expect(page.locator('.quota-table-wrap tbody tr')).toHaveCount(2);
  await expect(page.locator('.quota-table-wrap tbody tr').first()).toContainText('Reset deadline updated');
  await expect(page.locator('.quota-table-wrap tbody tr').first().locator('td').nth(3)).toHaveText('0 pp');
});

for (const outcome of ['success', 'failure'] as const) {
  test(`a stale quota ${outcome} cannot replace another provider or show its old error`, async ({ page, backend }) => {
    backend.defer = true;
    await mount(page);
    await expect.poll(() => backend.pending.length).toBe(1);
    await setProps(page, { provider: 'claude' });
    await expect.poll(() => backend.pending.length).toBe(2);
    expect(backend.pending.map((pending) => pending.query.provider)).toEqual(['codex', 'claude']);
    backend.pending[1].resolve([{ ...samples('claude')[0], usedPercent: 66 }]);
    await expect(page.locator('.quota-stats dd')).toHaveText(['66%', '0 pp', '0', '1']);
    if (outcome === 'success') backend.pending[0].resolve(samples());
    else backend.pending[0].reject(new Error('obsolete codex quota failure'));
    await expect(page.locator('html')).toHaveAttribute('data-settled-quota', '2');
    await expect(page.locator('.quota-stats dd')).toHaveText(['66%', '0 pp', '0', '1']);
    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.locator('.window-picker option:checked')).toContainText('Claude');
  });
}

test('a snapshot timestamp refreshes live history from stored samples without provider polling', async ({ page, backend }) => {
  await mount(page, false, true);
  await expect(page.locator('.quota-stats dd')).toHaveText(['7%', '15 pp', '1', '5']);
  backend.defer = true;
  await page.evaluate(async () => {
    const path = '/src/lib/stores/snapshot.svelte.ts';
    const { snapshot } = await import(/* @vite-ignore */ path);
    snapshot.value = { ...snapshot.value, generatedAt: '2026-09-22T10:00:01Z' };
  });
  await expect.poll(() => backend.pending.length).toBe(1);
  await expect(page.locator('.quota-panel')).toHaveAttribute('aria-busy', 'true');
  await expect(page.locator('.quota-stats dd')).toHaveText(['7%', '15 pp', '1', '5']);
  backend.pending[0].resolve([...samples(), { ...samples().at(-1)!, usedPercent: 9, ts: '2026-09-22T10:00:00Z' }]);
  await expect(page.locator('.quota-stats dd')).toHaveText(['9%', '17 pp', '1', '6']);
  await expect(page.locator('.quota-panel')).toHaveAttribute('aria-busy', 'false');
  expect(backend.queries).toHaveLength(2);
  expect(backend.queries[1].to).toBe('2026-09-22T10:00:00.001Z');
  expect(backend.commands).not.toContain('refresh_now');
});

test('invalid ranges discard pending results and unmount destroys the chart and ignores a pending rejection', async ({ page, backend }) => {
  backend.defer = true;
  await mount(page);
  await expect.poll(() => backend.pending.length).toBe(1);
  await setProps(page, { range: null });
  backend.pending[0].resolve(samples());
  await expect(page.locator('html')).toHaveAttribute('data-settled-quota', '1');
  await expect(page.locator('.quota-stats')).toHaveCount(0);
  await expect(page.locator('.quota-panel')).toHaveAttribute('aria-busy', 'false');
  await expect(page.getByText(/No quota records in this range/)).toBeVisible();
  backend.defer = false;
  await setProps(page, { range: RANGE });
  await expect(page.locator('.quota-stats dd')).toHaveText(['7%', '15 pp', '1', '5']);
  const chart = await chartState(page);
  backend.defer = true;
  await page.evaluate(async () => {
    const path = '/src/lib/stores/snapshot.svelte.ts';
    const { snapshot } = await import(/* @vite-ignore */ path);
    snapshot.value = { ...snapshot.value, generatedAt: '2026-09-22T10:01:00Z' };
  });
  await expect.poll(() => backend.pending.length).toBe(2);
  await page.evaluate(() => Reflect.get(window, '__unmountQuotaFixture')());
  backend.pending[1].reject(new Error('disposed request failure'));
  await expect(page.locator('html')).toHaveAttribute('data-settled-quota', '3');
  await expect(page.locator('.quota-panel')).toHaveCount(0);
  expect(await page.evaluate(async (id) => {
    const path = await Reflect.get(window, '__quotaChartModuleUrl')();
    const { Chart } = await import(/* @vite-ignore */ path);
    return Chart.instances[id] === undefined;
  }, chart!.id)).toBe(true);
  await page.clock.fastForward(60_000);
  expect(backend.queries).toHaveLength(3);
});

test('History displays model and effort variants alongside distinct token and quota charts', async ({ page, backend }) => {
  await page.setViewportSize({ width: 1280, height: 2100 });
  await mount(page, true);
  const expectedModels = ['gpt-6-astra · medium', 'gpt-6-astra · ultra', 'gpt-5.6-sol · xhigh', 'claude-fable · high', 'claude-opus · Effort not recorded'];
  await expect(page.locator('td.model')).toHaveText(expectedModels);
  await expect(page.locator('.quota-stats dd')).toHaveText(['7%', '15 pp', '1', '5']);
  await expect(page.locator('canvas')).toHaveCount(2);
  await expect.poll(() => chartState(page, '.chart canvas')).toMatchObject({
    type: 'bar', labels: [
      'Claude · claude-fable · high', 'Claude · claude-opus · Effort not recorded',
      'Codex · gpt-5.6-sol · xhigh', 'Codex · gpt-6-astra · medium', 'Codex · gpt-6-astra · ultra',
    ], values: [[400], [500], [300], [100], [200]],
  });
  expect(backend.historyQueries).toHaveLength(1);
  expect(backend.historyQueries[0].groupByModel).toBe(true);
  expect(backend.queries[0].provider).toBeNull();
  expect(backend.queries[0].from).toBe('2026-09-15T16:00:00.000Z');
  expect(backend.commands).not.toContain('refresh_now');
  await page.screenshot({ path: '/tmp/history-feature-preview.png', fullPage: true });
});
