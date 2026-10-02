import { test as base, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const test = base.extend<{ runtimeErrors: void }>({
  runtimeErrors: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await use();
    expect(errors).toEqual([]);
  }, { auto: true }],
});

async function openHistory(page: import('@playwright/test').Page, query = '') {
  await page.goto(`/dashboard${query}`);
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await expect(page.locator('.table-wrap tbody tr').first()).toBeVisible();
}

test('clicking a heatmap day narrows the range to that single day', async ({ page }) => {
  await openHistory(page);
  const heatmap = page.getByRole('group', { name: /Activity heatmap/ });
  await expect(heatmap).toBeVisible();
  // days outside the 26-week window are not rendered as cells at all, so
  // "empty" (level 0) and "missing" stay different things
  expect(await heatmap.locator('button[data-level]').count()).toBeGreaterThan(150);

  const busiest = heatmap.locator('button[data-level="4"]').last();
  expect(await busiest.getAttribute('aria-label')).toMatch(/requests$/);

  await busiest.click();
  await expect(page.getByRole('button', { name: 'Custom', exact: true })).toHaveAttribute('aria-pressed', 'true');
  const from = page.locator('#from');
  await expect(from).toBeVisible();
  const picked = await from.inputValue();
  expect(picked).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  await expect(page.locator('#to')).toHaveValue(picked);
  await expect(busiest).toHaveAttribute('aria-pressed', 'true');
  // a single day defaults to hour buckets, and every row lands inside it
  await expect(page.locator('#bucket')).toHaveValue('hour');
  await expect(page.locator('.table-wrap tbody tr').first()).toBeVisible();
  const labels = await page.locator('.table-wrap tbody tr td:nth-child(1)').allTextContents();
  expect(labels.length).toBeGreaterThan(0);
  expect(new Set(labels.map((text) => text.split(',')[0]))).toHaveProperty('size', 1);
  await page.screenshot({ path: test.info().outputPath('heatmap.png') });
});

test('the punch card shows every weekday and labels each cell', async ({ page }) => {
  await openHistory(page);
  await page.getByRole('group', { name: 'Activity view', exact: true })
    .getByRole('button', { name: 'Time of day', exact: true }).click();
  const punch = page.getByRole('table', { name: /Activity heatmap/ });
  await expect(punch).toBeVisible();
  await expect(punch.locator('tbody tr')).toHaveCount(7);
  await expect(punch.locator('tbody td')).toHaveCount(7 * 24);
  const labelled = await punch.locator('td[title]').first().getAttribute('title');
  expect(labelled).toMatch(/\d\d:00/);
});

test('the sessions view is a summary that exports CSV and hands its filters to the Sessions tab', async ({ page }) => {
  await openHistory(page);
  await page.getByRole('group', { name: 'Table', exact: true })
    .getByRole('button', { name: 'Sessions', exact: true }).click();

  // a lightweight summary, not a second full table
  await expect(page.getByRole('heading', { name: 'Largest sessions by tokens', exact: true })).toBeVisible();
  expect(await page.locator('.top-sessions li').count()).toBeGreaterThan(0);
  await expect(page.locator('.table-wrap')).toHaveCount(0);

  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export CSV', exact: true }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toMatch(/^ai-usage-sessions-.*\.csv$/);
  const csv = await readFile((await download.path())!, 'utf8');
  expect(csv).toContain('session_id,provider,project,first_activity,last_activity,duration_ms,models');
  expect(csv.trim().split(/\r?\n/).length).toBeGreaterThan(1);
  // identifiers and counters only — no prompt or response text anywhere
  expect(csv).not.toMatch(/prompt|message|content/i);

  await page.getByRole('button', { name: 'Open insights', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Sessions', exact: true })).toBeVisible();
  await expect(page.getByText('Median cost / session', { exact: true })).toBeVisible();
});

test('a monthly budget draws the burn-up in the Cost & budget view, reached by deep link', async ({ page }) => {
  const seeded = `?tab=history&view=cost&settings=${encodeURIComponent(JSON.stringify({ monthlyBudgetUsd: 250 }))}`;
  await page.goto(`/dashboard${seeded}`);

  const budget = page.getByRole('heading', { name: 'Monthly budget', exact: true });
  await expect(budget).toBeVisible();
  await expect(page.getByRole('group', { name: 'History view', exact: true }).getByRole('button', { name: 'Cost & budget', exact: true }))
    .toHaveAttribute('aria-pressed', 'true');
  const stat = page.getByRole('status').filter({ hasText: 'of the monthly budget used' });
  await expect(stat).toBeVisible();
  await expect(stat).toContainText(/on pace for \$[\d,]+\.\d\d of \$250\.00/);
  await expect(page.locator('canvas[aria-label*="monthly budget"]')).toBeVisible();
  // the estimate disclaimer stays on screen next to the money
  await expect(page.getByText("Estimate — subscriptions don't bill per token.").first()).toBeVisible();
  await page.screenshot({ path: test.info().outputPath('budget.png') });
});

test('no budget setting means no budget chart, and the Cost view says how to set one', async ({ page }) => {
  await openHistory(page);
  await expect(page.getByRole('heading', { name: 'Monthly budget', exact: true })).toHaveCount(0);
  await page.getByRole('group', { name: 'History view', exact: true }).getByRole('button', { name: 'Cost & budget', exact: true }).click();
  await expect(page.getByText('No monthly budget is set.')).toBeVisible();
  await expect(page.locator('canvas[aria-label*="monthly budget"]')).toHaveCount(0);
});
