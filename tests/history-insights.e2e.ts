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

test('the sessions view sorts server-capped rows and exports them as CSV', async ({ page }) => {
  await openHistory(page);
  await page.getByRole('group', { name: 'Table', exact: true })
    .getByRole('button', { name: 'Sessions', exact: true }).click();

  await expect(page.getByRole('columnheader', { name: /^Session/ })).toBeVisible();
  await expect(page.getByRole('columnheader', { name: 'Models / reasoning effort used', exact: true })).toBeVisible();
  const rows = page.locator('.table-wrap tbody tr');
  expect(await rows.count()).toBeGreaterThan(0);

  // the default order is the server's: biggest session first
  const total = page.getByRole('columnheader', { name: /^Total/ });
  await expect(total).toHaveAttribute('aria-sort', 'descending');
  await total.getByRole('button').click();
  await expect(total).toHaveAttribute('aria-sort', 'ascending');
  const durations = page.getByRole('columnheader', { name: /^Duration/ });
  await durations.getByRole('button').click();
  await expect(durations).toHaveAttribute('aria-sort', 'descending');

  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export CSV', exact: true }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toMatch(/^ai-usage-sessions-.*\.csv$/);
  const csv = await readFile((await download.path())!, 'utf8');
  expect(csv).toContain('session_id,provider,project,first_activity,last_activity,duration_ms,models');
  expect(csv.trim().split(/\r?\n/)).toHaveLength(await rows.count() + 1);
  // identifiers and counters only — no prompt or response text anywhere
  expect(csv).not.toMatch(/prompt|message|content/i);
});

test('a monthly budget draws the burn-up against the budget when showing cost', async ({ page }) => {
  const seeded = `?settings=${encodeURIComponent(JSON.stringify({ monthlyBudgetUsd: 250 }))}`;
  await openHistory(page, seeded);

  const budget = page.getByRole('heading', { name: 'Monthly budget', exact: true });
  await expect(budget).toBeVisible();
  // tokens are not money: the panel says what to switch to instead of guessing
  await expect(page.getByText('Switch “Show” to Est. cost to see the budget.')).toBeVisible();

  await page.getByRole('group', { name: 'Show', exact: true })
    .getByRole('button', { name: 'Est. cost', exact: true }).click();
  const stat = page.getByRole('status').filter({ hasText: 'of the monthly budget used' });
  await expect(stat).toBeVisible();
  await expect(stat).toContainText(/on pace for \$[\d,]+\.\d\d of \$250\.00/);
  await expect(page.locator('canvas[aria-label*="monthly budget"]')).toBeVisible();
  // the estimate disclaimer stays on screen next to the money
  await expect(page.getByText("Estimate — subscriptions don't bill per token.").first()).toBeVisible();
  await page.screenshot({ path: test.info().outputPath('budget.png') });
});

test('no budget setting means no budget panel', async ({ page }) => {
  await openHistory(page);
  await expect(page.getByRole('heading', { name: 'Monthly budget', exact: true })).toHaveCount(0);
});
