import { test as base, expect, type Page } from '@playwright/test';

const test = base.extend<{ runtimeErrors: void }>({
  runtimeErrors: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await use();
    expect(errors).toEqual([]);
  }, { auto: true }],
});

async function openHistory(page: Page, query = '') {
  await page.goto(`/dashboard${query}`);
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await expect(page.locator('.table-wrap tbody tr').first()).toBeVisible();
}

const views = (page: Page) => page.getByRole('group', { name: 'History view', exact: true });

test('Usage opens with six KPI tiles, each with a change against the previous period', async ({ page }) => {
  await openHistory(page);
  const tiles = page.locator('section[aria-label="Usage summary"] li.tile');
  await expect(tiles).toHaveCount(6);
  await expect(tiles.nth(0)).toContainText('Total tokens');
  await expect(tiles.nth(1)).toContainText('Est. cost');
  await expect(tiles.nth(2)).toContainText('Requests');
  await expect(tiles.nth(3)).toContainText('Active days');
  await expect(tiles.nth(4)).toContainText('Cache hit rate');
  await expect(tiles.nth(5)).toContainText('Daily average');
  // the mock has data for the 7 days before the default 7-day range, so every
  // change is a number with a direction glyph rather than "no earlier data"
  for (const label of await tiles.locator('.delta').evaluateAll((els) => els.map((el) => el.getAttribute('aria-label')))) {
    expect(label).toMatch(/compared with the previous period/);
  }
  await expect(tiles.nth(4)).toContainText('%');
  await expect(tiles.nth(0).locator('svg')).toBeVisible();
});

test('the composition, model-mix and project panels render with text alternatives', async ({ page }) => {
  await openHistory(page);
  await expect(page.getByRole('heading', { name: 'Token composition', exact: true })).toBeVisible();
  await expect(page.locator('canvas[aria-label^="Token composition"]')).toBeVisible();
  await expect(page.locator('canvas[aria-label^="Model mix over time"]')).toBeVisible();
  await expect(page.getByRole('region', { name: 'Reasoning effort', exact: true })).toBeVisible();
  // every chart has its numbers as a collapsed table
  expect(await page.getByText('Show data table').count()).toBeGreaterThanOrEqual(2);
  await page.getByText('Show data table').first().click();
  await expect(page.locator('details[open] table tbody tr').first()).toBeVisible();
});

test('clicking a project in the ranking filters the whole view to it', async ({ page }) => {
  await openHistory(page);
  const ranking = page.getByRole('heading', { name: 'Top projects', exact: true }).locator('xpath=ancestor::div[contains(@class,"panel")][1]');
  const rows = ranking.getByRole('button');
  await expect.poll(() => rows.count()).toBeGreaterThanOrEqual(3);
  expect(await rows.count()).toBeLessThanOrEqual(10);
  const first = rows.first();
  const title = await first.getAttribute('title');
  await first.click();
  await expect(page.locator('#project')).toHaveValue(`project:${title === 'No project' ? '' : title}`);
  await expect(first).toHaveAttribute('aria-pressed', 'true');
  // the table now carries the project column, restricted to that path
  await expect(page.getByRole('columnheader', { name: /Project/ })).toBeVisible();
  // a second click on the same row clears the filter again
  await first.click();
  await expect(page.locator('#project')).toHaveValue('all');
});

test('clicking a bar narrows the range to that day', async ({ page }) => {
  await openHistory(page);
  const canvas = page.locator('canvas[aria-label^="Usage chart"]');
  await expect(canvas).toBeVisible();
  const box = (await canvas.boundingBox())!;
  await page.mouse.click(box.x + box.width * 0.6, box.y + box.height * 0.4);
  await expect(page.getByRole('button', { name: 'Custom', exact: true })).toHaveAttribute('aria-pressed', 'true');
  const from = await page.locator('#from').inputValue();
  expect(from).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  await expect(page.locator('#to')).toHaveValue(from);
});

test('the heatmap switches measure and states the peak hours', async ({ page }) => {
  await openHistory(page);
  const insight = page.getByText(/^Your peak is \d+–\d+h/);
  await expect(insight).toBeVisible();
  const measure = page.getByRole('group', { name: 'Heatmap measure', exact: true });
  await measure.getByRole('button', { name: 'Requests', exact: true }).click();
  await expect(insight).toContainText('of requests');
  const heatmap = page.getByRole('group', { name: /Activity heatmap/ });
  expect(await heatmap.locator('button[data-level="4"]').last().getAttribute('aria-label')).toMatch(/: \d[\d,]* · [\d,]+ requests$/);
  await measure.getByRole('button', { name: 'Cache hit', exact: true }).click();
  expect(await heatmap.locator('button[data-level="4"]').last().getAttribute('aria-label')).toMatch(/: \d+% · /);
});

test('the sub-view choice is remembered across tabs and reachable by deep link', async ({ page }) => {
  await openHistory(page);
  await expect(views(page).getByRole('button', { name: 'Usage', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await views(page).getByRole('button', { name: 'Quota', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Quota history', exact: true })).toBeVisible();
  await expect(page.locator('.table-wrap')).toHaveCount(0);
  // the shared filters stay in place and sticky
  await expect(page.locator('#provider')).toBeVisible();
  await page.getByRole('button', { name: 'Overview', exact: true }).click();
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await expect(views(page).getByRole('button', { name: 'Quota', exact: true })).toHaveAttribute('aria-pressed', 'true');

  await page.goto('/dashboard?tab=history&view=cost');
  await expect(views(page).getByRole('button', { name: 'Cost & budget', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

test('the filter bar stays at the top while the page scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 });
  await openHistory(page);
  const bar = page.getByRole('search', { name: 'History filters' });
  await page.locator('main').evaluate((el) => el.scrollTo(0, 900));
  const top = (await bar.boundingBox())!.y;
  const main = (await page.locator('main').boundingBox())!.y;
  expect(top - main).toBeLessThan(24);
});

test('an empty range explains itself and offers a rescan', async ({ page }) => {
  await openHistory(page);
  await page.getByRole('button', { name: 'Custom', exact: true }).click();
  await page.locator('#from').fill('2020-01-01');
  await page.locator('#to').fill('2020-01-03');
  const empty = page.locator('[data-kind]');
  await expect(empty).toHaveAttribute('data-kind', 'range');
  await expect(empty).toContainText('No usage recorded in this range');
  await expect(empty.getByRole('button', { name: 'Rescan logs', exact: true })).toBeEnabled();

  // with a provider chosen the hint changes to point at the filters
  await page.locator('#provider').selectOption('codex');
  await expect(empty).toHaveAttribute('data-kind', 'filtered');
  await empty.getByRole('button', { name: 'Clear filters', exact: true }).click();
  await expect(page.locator('#provider')).toHaveValue('');
});

test('switching off log reading is named as the reason for an empty history', async ({ page }) => {
  const seeded = `?settings=${encodeURIComponent(JSON.stringify({ ingestEnabled: false }))}`;
  await page.goto(`/dashboard${seeded}`);
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page.getByRole('button', { name: 'Custom', exact: true }).click();
  await page.locator('#from').fill('2020-01-01');
  await page.locator('#to').fill('2020-01-03');
  await expect(page.locator('[data-kind="ingestDisabled"]')).toContainText('Reading session logs is turned off');
});
