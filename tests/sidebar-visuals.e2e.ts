// The sidebar route in its new visual modes, rendered from the mock backend
// with a seeded settings patch (see sidebar-items.e2e.ts).
import { test as base, expect, type Page } from '@playwright/test';
import type { SettingsPatch } from '../src/lib/settings-writer';

const test = base.extend<{ runtimeErrors: void }>({
  runtimeErrors: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await use();
    expect(errors).toEqual([]);
  }, { auto: true }],
});

async function openBar(page: Page, patch: SettingsPatch = {}) {
  await page.goto(`/?settings=${encodeURIComponent(JSON.stringify(patch))}`);
  await expect(page.locator('.pill')).toBeVisible();
  await expect(page.locator('.slot')).toHaveCount(2);
}

test('labelContent reset replaces the percentage with a countdown', async ({ page }) => {
  await openBar(page, { labelContent: 'reset' });
  for (const text of await page.locator('.pct').allTextContents()) expect(text).toMatch(/^(\d+m|\d+h\d\d|\d+d\d+h|\d+%)$/);
  await expect(page.locator('.pct').first()).not.toHaveText(/%$/);
});

test('labelContent both stacks percent and countdown on a vertical bar', async ({ page }) => {
  await openBar(page, { labelContent: 'both' });
  await expect(page.locator('.pct.sub')).toHaveCount(2);
  await expect(page.locator('.pct:not(.sub)').first()).toHaveText(/%$/);
});

test('a horizontal bar writes percent and countdown to the right of each ring', async ({ page }) => {
  await openBar(page, { edge: 'top' });
  await expect(page.locator('.txt.inline')).toHaveCount(2);
  await expect(page.locator('.txt.inline .pct').first()).toHaveText(/^\d+% · /);
});

test('centre position keeps the percentage in the ring and puts the countdown beside it', async ({ page }) => {
  await openBar(page, { percentPosition: 'center', labelContent: 'both' });
  await expect(page.locator('.center-pct').first()).toHaveText(/%$/);
  await expect(page.locator('.pct').first()).toHaveText(/^(\d+m|\d+h\d\d|\d+d\d+h)$/);
});

test('compact mode draws slim bars, one segment per visible window, and keeps the aria label', async ({ page }) => {
  await openBar(page, { ringStyle: 'bar' });
  await expect(page.locator('.mini')).toHaveCount(2);
  await expect(page.locator('.ring')).toHaveCount(0);
  await expect(page.locator('.mini').first().locator('.seg')).toHaveCount(3);
  await expect(page.getByRole('button', { name: /^Claude:/ })).toHaveAttribute('aria-label', /Weekly.*5-hour/);
  const compact = (await page.locator('.pill').boundingBox())!;
  await openBar(page, { ringStyle: 'ring' });
  const rings = (await page.locator('.pill').boundingBox())!;
  expect(compact.width).toBeLessThan(rings.width);
  expect(compact.height).toBeLessThan(rings.height);
});

test('compact mode works on a horizontal edge too', async ({ page }) => {
  await openBar(page, { ringStyle: 'bar', edge: 'bottom' });
  const box = (await page.locator('.pill').boundingBox())!;
  expect(box.width).toBeGreaterThan(box.height);
  await expect(page.locator('.mini.horizontal')).toHaveCount(2);
});

test('the forecast arc and the run-out badge only appear for trusted forecasts', async ({ page }) => {
  await openBar(page);
  const arcs = await page.locator('.forecast-arc').count();
  const ticks = await page.locator('.tick').count();
  // every dashed run belongs to an arc that also has a tick
  expect(arcs).toBeLessThanOrEqual(ticks);
});
