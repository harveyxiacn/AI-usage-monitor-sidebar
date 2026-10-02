import { test, expect } from '@playwright/test';

// Deliberately no pointer, keyboard, dispatchEvent, or element.click calls.
// These checks load real routes (backed by the browser mock), observe the
// rendered output and capture screenshots.
const settingsParam = (patch: Record<string, unknown>) => encodeURIComponent(JSON.stringify(patch));
const noOverflow = (page: import('@playwright/test').Page) =>
  page.evaluate(() => document.documentElement.scrollWidth <= innerWidth);

for (const theme of ['dark', 'light']) {
  test(`overview shows the routing advice with its basis (${theme})`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`/dashboard?tab=overview&settings=${settingsParam({ theme, language: 'en' })}`);
    const card = page.getByRole('article', { name: 'Where to work next' });
    await expect(card).toBeVisible();
    // the mock's Claude 5-hour window runs out in ~40 min; Codex has room
    await expect(card.getByRole('status')).toContainText('Claude 5-hour is at 73%');
    await expect(card.getByRole('status')).toContainText('consider Codex for the next ~');
    await expect(card.getByText('Confidence:', { exact: false })).toBeVisible();
    await expect(card.getByText('Estimate.', { exact: false })).toBeVisible();
    await expect(card.locator('details li')).toHaveCount(2);
    expect(await noOverflow(page)).toBe(true);
    expect(errors).toEqual([]);
    await page.screenshot({ path: `test-results/advisor-overview-${theme}.png`, fullPage: true });
  });
}

test('overview advice is localised', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 700, height: 850 });
  await page.goto(`/dashboard?tab=overview&settings=${settingsParam({ language: 'zh-CN' })}`);
  const card = page.getByRole('article', { name: '接下来在哪里工作' });
  await expect(card).toBeVisible();
  await expect(card.getByRole('status')).toContainText('可考虑改用 Codex');
  expect(await noOverflow(page)).toBe(true);
  expect(errors).toEqual([]);
});

test('the popover shows a one-line hint only while a recommendation exists', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`/popover?settings=${settingsParam({ language: 'en' })}`);
  await expect(page.getByRole('dialog')).toBeVisible();
  await expect(page.locator('.advice')).toContainText('Consider Codex for the next ~');
  await expect(page.locator('.advice')).toContainText('Claude 5-hour at 73%');
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/advisor-popover.png' });
});

test('cost view carries the plan advisor with reasons and a hint-only disclaimer', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`/dashboard?tab=history&view=cost&settings=${settingsParam({ language: 'en', subscriptionUsd: { claude: 100, codex: 20 } })}`);
  const card = page.locator('.plan');
  await expect(card.getByRole('heading', { name: 'Plan advisor (estimate)' })).toBeVisible();
  await expect(card.locator('.row[data-verdict]').first()).toBeVisible();
  // every verdict is one of the four, and each row explains itself
  const verdicts = await card.locator('.verdict').allTextContents();
  expect(verdicts.length).toBeGreaterThan(0);
  for (const v of verdicts) {
    expect(['Consider a higher plan', 'Consider a lower plan', 'Keep your current plan', 'Not enough data yet']).toContain(v);
  }
  expect(await card.locator('.row li').count()).toBeGreaterThan(0);
  await expect(card.getByText('not billing facts', { exact: false })).toBeVisible();
  expect(await noOverflow(page)).toBe(true);
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/advisor-plan.png', fullPage: true });
});

test('commits view is off by default and renders attributed commits once enabled', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const project = encodeURIComponent('/home/demo/projects/website');
  await page.goto(`/dashboard?tab=history&view=commits&project=${project}&settings=${settingsParam({ language: 'en' })}`);
  await expect(page.getByText('Commit attribution is off.')).toBeVisible();
  await expect(page.locator('.commits table')).toHaveCount(0);

  await page.goto(`/dashboard?tab=history&view=commits&project=${project}&settings=${settingsParam({ language: 'en', gitAttribution: true })}`);
  await expect(page.locator('.commits table tbody tr').first()).toBeVisible();
  expect(await page.locator('.commits table tbody tr').count()).toBeGreaterThan(5);
  await expect(page.locator('.commits .summary')).toContainText('commits');
  await expect(page.getByText('Estimate. Tokens between two commits', { exact: false })).toBeVisible();
  expect(await noOverflow(page)).toBe(true);
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/advisor-commits.png', fullPage: true });
});

test('commits view asks for a project, and the mock backend never runs without the setting', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`/dashboard?tab=history&view=commits&settings=${settingsParam({ language: 'en', gitAttribution: true })}`);
  await expect(page.getByText('Pick a project in the filter above', { exact: false })).toBeVisible();
  const result = await page.evaluate(async () => {
    const path = '/src/lib/mock.ts';
    const { mockInvoke } = await import(path);
    const query = { project: '/home/demo/projects/website', from: new Date(Date.now() - 7 * 86_400_000).toISOString(), to: new Date().toISOString() };
    const ok = await mockInvoke('get_project_commits', { query });
    const unknown = await mockInvoke('get_project_commits', { query: { ...query, project: '/not/recorded' } });
    return { status: ok.status, commits: ok.commits.length, sumTokens: ok.commits.reduce((s: number, c: { totalTokens: number }) => s + c.totalTokens, 0), unknown: unknown.status };
  });
  expect(result.status).toBe('ok');
  expect(result.commits).toBeGreaterThan(0);
  expect(result.sumTokens).toBeGreaterThan(0);
  expect(result.unknown).toBe('unknown_project');
  expect(errors).toEqual([]);
});
