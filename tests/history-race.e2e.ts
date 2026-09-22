import { test as base, expect, type Page } from '@playwright/test';
import { mockSettings, mockSnapshot } from '../src/lib/mock';
import type { HistoryQuery, HistoryResult, ProviderId, TokenTotals } from '../src/lib/types';

function emptyTotals(): TokenTotals {
  return {
    inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0,
    reasoningTokens: 0, totalTokens: 0, requests: 0, estimatedCostUsd: 0,
  };
}

interface PendingHistory {
  provider: ProviderId | null;
  resolve: (result: HistoryResult) => void;
  reject: (error: Error) => void;
}

const test = base.extend<{ historyRequests: PendingHistory[] }>({
  historyRequests: async ({ page }, use) => {
    const requests: PendingHistory[] = [];
    const runtimeErrors: string[] = [];
    page.on('pageerror', (error) => runtimeErrors.push(error.message));
    let listenerId = 0;
    await page.exposeBinding('__controlledTauriInvoke', (_source, command: string, args: { query?: HistoryQuery }) => {
      switch (command) {
        case 'get_settings': return mockSettings;
        case 'get_snapshot': return mockSnapshot;
        case 'get_quota_history': return [];
        // the heatmap and the session view are separate commands on purpose,
        // so this test still counts exactly the table's history requests
        case 'get_usage_calendar': return { days: [], slots: [], totals: emptyTotals() };
        case 'get_usage_sessions': return { rows: [], totalSessions: 0, totals: emptyTotals(), truncated: false };
        case 'plugin:event|listen': return ++listenerId;
        case 'plugin:event|unlisten': return;
        case 'get_usage_history':
          return new Promise<HistoryResult>((resolve, reject) => {
            requests.push({ provider: args.query!.provider, resolve, reject });
          });
        default: throw new Error(`Unexpected Tauri command in history test: ${command}`);
      }
    });
    // Emulate the real IPC boundary without exposing hooks in product code.
    await page.addInitScript(() => {
      let callbackId = 0;
      let settledHistory = 0;
      const callbacks = new Map<number, (event: unknown) => void>();
      const listeners = new Map<string, number>();
      Reflect.set(window, '__emitIngest', (eventsAdded: number) => {
        const id = listeners.get('ingest-progress');
        if (id !== undefined) callbacks.get(id)?.({ event: 'ingest-progress', id, payload: {
          running: false, filesScanned: 1, filesUpdated: eventsAdded ? 1 : 0,
          eventsAdded, durationMs: 1, errors: [],
        } });
      });
      const invoke = Reflect.get(window, '__controlledTauriInvoke') as (command: string, args: unknown) => Promise<unknown>;
      Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
        transformCallback: (callback: (event: unknown) => void) => {
          callbacks.set(++callbackId, callback);
          return callbackId;
        },
        invoke: async (command: string, args: unknown) => {
          if (command === 'plugin:event|listen') {
            const { event, handler } = args as { event: string; handler: number };
            listeners.set(event, handler);
          }
          try { return await invoke(command, args); }
          finally {
            if (command === 'get_usage_history') {
              // A test-owned acknowledgement lets assertions wait for the
              // delayed response instead of relying on a wall-clock timeout.
              document.documentElement.dataset.settledHistory = String(++settledHistory);
            }
          }
        },
      } });
      Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } });
    });
    await use(requests);
    expect(runtimeErrors).toEqual([]);
  },
});

function result(provider: ProviderId, totalTokens: number): HistoryResult {
  const totals: TokenTotals = {
    inputTokens: totalTokens, cacheWriteTokens: 0, cacheReadTokens: 0,
    outputTokens: 0, reasoningTokens: 0, totalTokens, requests: 1, estimatedCostUsd: 0.01,
  };
  return {
    rows: [{ ...totals, bucketStart: '2026-09-20T00:00:00Z', provider, model: null, project: null }],
    totals,
    byProvider: { [provider]: totals },
    projects: [],
    costApproximate: false,
  };
}

async function openHistory(page: Page, requests: PendingHistory[]) {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await expect.poll(() => requests.length).toBe(1);
  expect(requests[0].provider).toBeNull();
  requests[0].resolve(result('claude', 111));
  await expect(page.locator('tbody tr')).toHaveCount(1);
  await expect(page.locator('tbody .strong')).toHaveText('111');
}

for (const outcome of ['success', 'error'] as const) {
  test(`a slow earlier ${outcome} cannot replace the latest provider selection`, async ({ page, historyRequests }) => {
    await openHistory(page, historyRequests);
    await page.locator('#provider').selectOption('claude');
    await expect.poll(() => historyRequests.length).toBe(2);
    expect(historyRequests[1].provider).toBe('claude');
    await page.locator('#provider').selectOption('codex');
    await expect.poll(() => historyRequests.length).toBe(3);
    expect(historyRequests[2].provider).toBe('codex');

    historyRequests[2].resolve(result('codex', 432));
    await expect(page.locator('tbody tr td:nth-child(2)')).toHaveText('Codex');
    await expect(page.locator('tbody .strong')).toHaveText('432');
    if (outcome === 'success') historyRequests[1].resolve(result('claude', 999));
    else historyRequests[1].reject(new Error('stale history failure'));

    await expect(page.locator('html')).toHaveAttribute('data-settled-history', '3');
    // Flush the render following the acknowledged IPC completion.
    await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())));
    await expect(page.locator('#provider')).toHaveValue('codex');
    await expect(page.locator('tbody tr td:nth-child(2)')).toHaveText('Codex');
    await expect(page.locator('tbody .strong')).toHaveText('432');
    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Export CSV', exact: true })).toBeEnabled();
  });
}

test('changing the provider recovers after a history request fails', async ({ page, historyRequests }) => {
  await openHistory(page, historyRequests);
  await page.locator('#provider').selectOption('claude');
  await expect.poll(() => historyRequests.length).toBe(2);
  historyRequests[1].reject(new Error('history storage unavailable'));
  await expect(page.getByRole('alert')).toContainText('history storage unavailable');
  await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Export CSV', exact: true })).toBeDisabled();

  await page.locator('#provider').selectOption('codex');
  await expect.poll(() => historyRequests.length).toBe(3);
  historyRequests[2].resolve(result('codex', 567));
  await expect(page.locator('tbody tr td:nth-child(2)')).toHaveText('Codex');
  await expect(page.locator('tbody .strong')).toHaveText('567');
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Export CSV', exact: true })).toBeEnabled();
});

test('a background time refresh preserves the chart and rows while fetching', async ({ page, historyRequests }) => {
  await page.clock.install();
  await openHistory(page, historyRequests);
  const canvas = page.locator('canvas').last();
  await expect(canvas).toBeVisible();
  await canvas.evaluate((node) => node.setAttribute('data-original-chart', 'yes'));
  await page.clock.fastForward(60_000);
  await expect.poll(() => historyRequests.length).toBe(2);
  await expect(page.locator('tbody .strong')).toHaveText('111');
  await expect(page.locator('canvas[data-original-chart="yes"]')).toBeVisible();
  historyRequests[1].resolve(result('claude', 222));
  await expect(page.locator('tbody .strong')).toHaveText('222');
  await expect(page.locator('canvas[data-original-chart="yes"]')).toBeVisible();
});


test('empty log scans do not reload history; new events update it in place', async ({ page, historyRequests }) => {
  await openHistory(page, historyRequests);
  await page.evaluate(async () => {
    Reflect.get(window, '__emitIngest')(0);
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  expect(historyRequests).toHaveLength(1);
  await page.evaluate(() => Reflect.get(window, '__emitIngest')(1));
  await expect.poll(() => historyRequests.length).toBe(2);
  await expect(page.locator('tbody .strong')).toHaveText('111');
  historyRequests[1].resolve(result('claude', 333));
  await expect(page.locator('tbody .strong')).toHaveText('333');
});

for (const scenario of [
  { name: 'mixed prices', full: null, known: 12.34, missing: 3, expected: '$12.34 (partial estimate)' },
  { name: 'free known records mixed with unpriced records', full: null, known: 0, missing: 2, expected: '$0.00 (partial estimate)' },
  { name: 'only unpriced records', full: null, known: null, missing: 4, expected: '—' },
  { name: 'fully priced records', full: 15, known: 15, missing: 0, expected: '$15.00' },
]) {
  test(`cost display distinguishes ${scenario.name}`, async ({ page, historyRequests }) => {
    await page.goto('/dashboard');
    await page.getByRole('button', { name: 'History', exact: true }).click();
    await expect.poll(() => historyRequests.length).toBe(1);
    const data = result('codex', 1000);
    const cost = { estimatedCostUsd: scenario.full, knownCostUsd: scenario.known, unpricedRequests: scenario.missing };
    Object.assign(data.totals, cost);
    Object.assign(data.rows[0], cost);
    historyRequests[0].resolve(data);
    const card = page.locator('article.cmp').filter({ hasText: 'Codex' });
    await expect(card.locator('dd').last()).toHaveText(scenario.expected);
    await expect(page.locator('tbody tr td').last()).toHaveText(scenario.expected);
    if (scenario.full == null && scenario.known != null) {
      await expect(card).toContainText(`${scenario.missing} unpriced requests are excluded`);
    } else {
      await expect(card).not.toContainText('partial estimate');
    }
  });
}
