import { test as base, expect, type Page } from '@playwright/test';
import { mockSettings, mockSnapshot } from '../src/lib/mock';
import type { AppSnapshot } from '../src/lib/types';

function quotaSnapshot(claudeUsed = 77, codexUsed = 29): AppSnapshot {
  const value = structuredClone(mockSnapshot);
  const claude = value.providers.find((provider) => provider.provider === 'claude')!;
  claude.windows = claude.windows.slice(0, 3);
  claude.windows[0].usedPercent = claudeUsed;
  claude.windows[1].usedPercent = 49;
  claude.windows[2].usedPercent = 59;
  const codex = value.providers.find((provider) => provider.provider === 'codex')!;
  codex.windows = codex.windows.filter((window) => window.kind === 'seven_day');
  codex.windows[0].isPrimary = true;
  codex.windows[0].usedPercent = codexUsed;
  return value;
}

interface PendingSnapshot {
  resolve: (value: AppSnapshot) => void;
  reject: (error: Error) => void;
}

interface Backend {
  value: AppSnapshot;
  commands: string[];
  reads: PendingSnapshot[];
  refreshes: PendingSnapshot[];
  deferReads: boolean;
  failedSubscriptions: number;
  subscriptionAttempts: number;
}

const test = base.extend<{ backend: Backend }>({
  backend: async ({ page }, use) => {
    const backend: Backend = {
      value: quotaSnapshot(), commands: [], reads: [], refreshes: [],
      deferReads: false, failedSubscriptions: 0, subscriptionAttempts: 0,
    };
    const runtimeErrors: string[] = [];
    page.on('pageerror', (error) => runtimeErrors.push(error.message));
    const time = new Date('2026-09-22T10:00:00Z');
    await page.clock.install({ time });
    await page.clock.pauseAt(time);
    let listenerId = 0;
    await page.exposeBinding('__controlledSnapshotInvoke', (_source, command: string, args: { event?: string }) => {
      backend.commands.push(command);
      switch (command) {
        case 'get_settings': return { ...mockSettings, percentMode: 'remaining', surfaceStyle: 'cyber' };
        case 'get_snapshot':
          if (!backend.deferReads) return structuredClone(backend.value);
          return new Promise<AppSnapshot>((resolve, reject) => backend.reads.push({ resolve, reject }));
        case 'refresh_now':
          return new Promise<AppSnapshot>((resolve, reject) => backend.refreshes.push({ resolve, reject }));
        case 'plugin:event|listen':
          if (args.event === 'snapshot-updated') {
            backend.subscriptionAttempts++;
            if (backend.failedSubscriptions > 0) {
              backend.failedSubscriptions--;
              throw new Error('snapshot listener temporarily unavailable');
            }
          }
          return ++listenerId;
        case 'plugin:event|unlisten':
        case 'sidebar_relayout':
          return;
        default: throw new Error(`Unexpected Tauri command in snapshot test: ${command}`);
      }
    });
    // Exercise the same IPC/event boundary as the native windows. All hooks
    // belong to this fixture; production code has no special test interface.
    await page.addInitScript(() => {
      let callbackId = 0;
      let settledReads = 0;
      let settledRefreshes = 0;
      const callbacks = new Map<number, (event: unknown) => void>();
      const listeners = new Map<number, { event: string; handler: number }>();
      Reflect.set(window, '__emitSnapshot', (payload: unknown) => {
        let delivered = 0;
        for (const [id, listener] of listeners) {
          if (listener.event !== 'snapshot-updated') continue;
          callbacks.get(listener.handler)?.({ event: listener.event, id, payload });
          delivered++;
        }
        return delivered;
      });
      const invoke = Reflect.get(window, '__controlledSnapshotInvoke') as (command: string, args: unknown) => Promise<unknown>;
      Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
        transformCallback: (callback: (event: unknown) => void) => {
          callbacks.set(++callbackId, callback);
          return callbackId;
        },
        invoke: async (command: string, args: unknown) => {
          try {
            const result = await invoke(command, args);
            // An unsuccessful listen must not silently install a callback.
            if (command === 'plugin:event|listen') {
              listeners.set(result as number, args as { event: string; handler: number });
            } else if (command === 'plugin:event|unlisten') {
              listeners.delete((args as { eventId: number }).eventId);
            }
            return result;
          } finally {
            if (command === 'get_snapshot') document.documentElement.dataset.settledSnapshots = String(++settledReads);
            if (command === 'refresh_now') document.documentElement.dataset.settledRefreshes = String(++settledRefreshes);
          }
        },
      } });
      Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } });
    });
    await use(backend);
    expect(runtimeErrors).toEqual([]);
  },
});

async function expectRings(page: Page, claudeRemaining = 23, codexRemaining = 71) {
  const slots = page.locator('.slot[role="button"]');
  await expect(slots).toHaveCount(2);
  await expect(slots.locator('.pct')).toHaveText([`${claudeRemaining}%`, `${codexRemaining}%`]);
  await expect(slots.nth(0)).toHaveAttribute('aria-label', `Claude: Weekly 51%, 5-hour ${claudeRemaining}%, Weekly · Fable 41%`);
  await expect(slots.nth(1)).toHaveAttribute('aria-label', `Codex: Weekly ${codexRemaining}%`);
  // Read SVG attributes, not animated computed styles. Each entry is the
  // actual fraction of the ring's circumference painted by its value arc.
  await expect.poll(() => slots.locator('circle.arc').evaluateAll((arcs) => arcs.map((arc) => {
    const circumference = Number(arc.getAttribute('stroke-dasharray'));
    const offset = Number(arc.getAttribute('stroke-dashoffset'));
    return Math.round(100 * (1 - offset / circumference));
  }))).toEqual([51, claudeRemaining, 41, codexRemaining]);
}

async function emitSnapshot(page: Page, value: AppSnapshot) {
  return page.evaluate((payload) => Reflect.get(window, '__emitSnapshot')(payload) as number, value);
}

async function storeError(page: Page) {
  return page.evaluate(async () => {
    const path = '/src/lib/stores/snapshot.svelte.ts';
    const { snapshot } = await import(/* @vite-ignore */ path);
    return snapshot.error as string | null;
  });
}

async function openSidebar(page: Page) {
  await page.goto('/');
  await expectRings(page);
}

test('snapshot events update the labels and SVG arcs of existing keyed sidebar rings', async ({ page, backend }) => {
  await openSidebar(page);
  await page.locator('.slot[role="button"]').evaluateAll((slots) => slots.forEach((slot, i) => {
    slot.setAttribute('data-original-slot', String(i));
  }));
  expect(await emitSnapshot(page, quotaSnapshot(100, 34))).toBe(1);
  await expectRings(page, 0, 66);
  await expect(page.locator('[data-original-slot="0"] .pct')).toHaveText('0%');
  await expect(page.locator('[data-original-slot="1"] .pct')).toHaveText('66%');
  expect(backend.commands).not.toContain('refresh_now');
});

test('a missed event is repaired within 30 seconds using only the cached snapshot', async ({ page, backend }) => {
  await openSidebar(page);
  backend.value = quotaSnapshot(100, 34);
  await expectRings(page);
  const initialReads = backend.commands.filter((command) => command === 'get_snapshot').length;
  await page.clock.fastForward(30_000);
  await expectRings(page, 0, 66);
  const additionalReads = backend.commands.filter((command) => command === 'get_snapshot').length - initialReads;
  expect(additionalReads).toBeGreaterThanOrEqual(1);
  expect(additionalReads).toBeLessThanOrEqual(2);
  expect(backend.commands).not.toContain('refresh_now');
});

test('a failed snapshot subscription retries and subsequent events update immediately', async ({ page, backend }) => {
  backend.failedSubscriptions = 1;
  await openSidebar(page);
  expect(await emitSnapshot(page, quotaSnapshot(100, 34))).toBe(0);
  await expectRings(page);
  await page.clock.fastForward(30_000);
  await expect.poll(() => backend.subscriptionAttempts).toBe(2);
  await expect.poll(() => emitSnapshot(page, quotaSnapshot(100, 34))).toBe(1);
  await expectRings(page, 0, 66);
  expect(await storeError(page)).toBeNull();
  expect(backend.commands).not.toContain('refresh_now');
});

for (const phase of ['initial', 'periodic'] as const) {
  for (const outcome of ['response', 'rejection'] as const) {
    test(`a delayed ${phase} snapshot ${outcome} cannot overwrite a newer event or expose an old error`, async ({ page, backend }) => {
      if (phase === 'initial') {
        backend.deferReads = true;
        await page.goto('/');
      } else {
        await openSidebar(page);
        backend.deferReads = true;
        await page.clock.fastForward(30_000);
      }
      await expect.poll(() => backend.reads.length).toBe(1);
      expect(await emitSnapshot(page, quotaSnapshot(100, 34))).toBe(1);
      await expectRings(page, 0, 66);
      if (outcome === 'response') backend.reads[0].resolve(quotaSnapshot());
      else backend.reads[0].reject(new Error('stale snapshot request failed'));
      await expect(page.locator('html')).toHaveAttribute('data-settled-snapshots', phase === 'initial' ? '1' : '2');
      await expectRings(page, 0, 66);
      expect(await storeError(page)).toBeNull();
    });
  }
}

for (const lifecycle of ['focus', 'pageshow', 'visible'] as const) {
  test(`${lifecycle} reconciles the cached snapshot promptly`, async ({ page, backend }) => {
    await openSidebar(page);
    backend.value = quotaSnapshot(100, 34);
    await page.evaluate((event) => {
      if (event === 'visible') {
        Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
        document.dispatchEvent(new Event('visibilitychange'));
        Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
        document.dispatchEvent(new Event('visibilitychange'));
      } else {
        window.dispatchEvent(new Event(event));
      }
    }, lifecycle);
    // The clock stays frozen: this update must not wait for the 30s timer.
    await expectRings(page, 0, 66);
    expect(backend.commands).not.toContain('refresh_now');
  });
}

test('a periodic cached read cannot suppress a newer refresh response when its event is lost', async ({ page, backend }) => {
  await openSidebar(page);
  backend.deferReads = true;
  await page.clock.fastForward(30_000);
  await expect.poll(() => backend.reads.length).toBe(1);
  await page.evaluate(async () => {
    const path = '/src/lib/stores/snapshot.svelte.ts';
    const { snapshot } = await import(/* @vite-ignore */ path);
    void snapshot.refresh();
  });
  await expect.poll(() => backend.refreshes.length).toBe(1);
  backend.reads[0].resolve(quotaSnapshot());
  await expect(page.locator('html')).toHaveAttribute('data-settled-snapshots', '2');
  await expectRings(page);
  // No event accompanies this completion; refresh_now's own return value is
  // the only path by which the fresh quota can reach the sidebar.
  backend.refreshes[0].resolve(quotaSnapshot(100, 34));
  await expect(page.locator('html')).toHaveAttribute('data-settled-refreshes', '1');
  await expectRings(page, 0, 66);
  expect(await storeError(page)).toBeNull();
});

test('the last store consumer removes listeners and timers and ignores its pending read', async ({ page, backend }) => {
  // An empty document lets this test own all consumers of the real store,
  // without adding a product-only route or reaching into private fields.
  await page.route('**/snapshot-store-lifecycle', (route) => route.fulfill({
    contentType: 'text/html', body: '<!doctype html><html><body></body></html>',
  }));
  await page.goto('/snapshot-store-lifecycle');
  backend.deferReads = true;
  await page.evaluate(async () => {
    const path = '/src/lib/stores/snapshot.svelte.ts';
    const { snapshot } = await import(/* @vite-ignore */ path);
    Reflect.set(window, '__snapshotDisposers', [snapshot.init(), snapshot.init()]);
  });
  await expect.poll(() => backend.reads.length).toBe(1);
  expect(backend.subscriptionAttempts).toBe(1);
  await page.evaluate(() => Reflect.get(window, '__snapshotDisposers')[0]());
  expect(backend.commands).not.toContain('plugin:event|unlisten');
  // An outstanding read is deduplicated even while the remaining consumer
  // stays subscribed through another periodic reconciliation.
  await page.clock.fastForward(30_000);
  expect(backend.reads).toHaveLength(1);
  await page.evaluate(() => Reflect.get(window, '__snapshotDisposers')[1]());
  await expect.poll(() => backend.commands.filter((command) => command === 'plugin:event|unlisten').length).toBe(1);
  backend.reads[0].resolve(quotaSnapshot(100, 34));
  await expect(page.locator('html')).toHaveAttribute('data-settled-snapshots', '1');
  await page.clock.fastForward(60_000);
  await page.evaluate(() => {
    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('pageshow'));
    document.dispatchEvent(new Event('visibilitychange'));
  });
  expect(await emitSnapshot(page, quotaSnapshot(100, 34))).toBe(0);
  expect(backend.reads).toHaveLength(1);
  expect(backend.subscriptionAttempts).toBe(1);
  const state = await page.evaluate(async () => {
    const path = '/src/lib/stores/snapshot.svelte.ts';
    const { snapshot } = await import(/* @vite-ignore */ path);
    return { value: snapshot.value, error: snapshot.error };
  });
  expect(state).toEqual({ value: null, error: null });
});
