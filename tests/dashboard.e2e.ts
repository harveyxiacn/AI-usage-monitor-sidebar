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

test.beforeEach(async ({ page }) => {
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();
});

test('a rate-limited provider reads as stale, not as an error', async ({ page }) => {
  await page.goto('/dashboard?mock=rate-limited');
  const claude = page.locator('.provider').filter({ hasText: 'Claude' });
  // the message says how old the data is and when the app will look again
  await expect(
    claude.getByText(/rate-limiting the usage endpoint .* showing values from .* next attempt in/)
  ).toBeVisible();
  await expect(claude.locator('.dot[data-status="rate_limited"]')).toHaveAttribute(
    'title',
    'Rate-limited'
  );
  // amber staleness, not a red error badge, and the last windows stay visible
  await expect(page.locator('.provider .hint.bad')).toHaveCount(0);
  await expect(claude.locator('.win').first()).toBeVisible();
});

test('refreshing prices from a URL is opt-in', async ({ page }) => {
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const refresh = page.getByRole('button', { name: 'Refresh prices now', exact: true });
  // No URL configured → nothing to fetch, so the button cannot be pressed.
  await expect(refresh).toBeDisabled();

  const url = page.getByLabel('Pricing table URL');
  await url.fill('https://raw.githubusercontent.com/example/repo/main/pricing.json');
  await url.blur();
  await expect(refresh).toBeEnabled();
  await refresh.click();
  await expect(page.getByText(/Updated /)).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('overview refreshes and history filters and exports the visible rows', async ({ page }) => {
  await expect(page.locator('.provider')).toHaveCount(2);
  await page.getByRole('button', { name: 'Refresh all', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Refresh all', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page.locator('#provider').selectOption('codex');
  await page.locator('#group').selectOption('model');
  await expect(page.locator('tbody tr').first()).toBeVisible();
  for (const text of await page.locator('tbody tr td:nth-child(2)').allTextContents()) expect(text).toBe('Codex');
  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export CSV', exact: true }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toMatch(/^ai-usage-.*\.csv$/);
  const csv = await readFile((await download.path())!, 'utf8');
  expect(csv).toContain('bucket_start,provider,model');
  expect(csv).toContain(',codex,');
  expect(csv).not.toContain(',claude,');
  expect(csv.trim().split(/\r?\n/)).toHaveLength(await page.locator('tbody tr').count() + 1);
  await page.screenshot({ path: test.info().outputPath('history.png') });
  await page.getByRole('group', { name: 'Show', exact: true }).getByRole('button', { name: 'Est. cost', exact: true }).click();
  // `gpt-5.3-codex-spark` is not in the table; it is priced from its family
  // (`gpt-5.3-codex`), so the estimate is shown but flagged as approximate.
  await expect(
    page.getByText('Some models are priced from the closest known family')
  ).toBeVisible();
});

test('the project filter narrows the table and exports complete paths', async ({ page }) => {
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await expect(page.locator('tbody tr').first()).toBeVisible();
  // no project column until something is filtered or split by project
  await expect(page.getByRole('columnheader', { name: /Project/ })).toHaveCount(0);

  const path = '/home/demo/work/client/website';
  await page.locator('#project').selectOption(`project:${path}`);
  await expect(page.getByRole('columnheader', { name: /Project/ })).toBeVisible();
  const cells = page.locator('tbody tr td:nth-child(3)');
  expect(await cells.count()).toBeGreaterThan(0);
  // the shortened label disambiguates the two "website" projects…
  for (const text of await cells.allTextContents()) expect(text).toBe('website — /home/demo/work/client');
  // …while the exact path stays available for selection and export
  for (const title of await cells.evaluateAll((els) => els.map((el) => el.getAttribute('title')))) expect(title).toBe(path);

  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export CSV', exact: true }).click();
  const csv = await readFile((await (await downloadPromise).path())!, 'utf8');
  expect(csv).toContain('model,project,input_tokens');
  for (const line of csv.trim().split(/\r?\n/).slice(1)) expect(line).toContain(`,${path},`);

  // splitting by project keeps the filter but shows every project once cleared
  await page.getByLabel('Split by project').check();
  await expect(cells).toHaveCount(await page.locator('tbody tr').count());
  await page.locator('#project').selectOption('all');
  await expect(page.locator('#project')).toHaveValue('all');
  await expect.poll(async () => new Set(await cells.allTextContents()).size).toBeGreaterThan(1);
});

test('invalid custom dates do not silently fall back to another range', async ({ page }) => {
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page.getByRole('button', { name: 'Custom', exact: true }).click();
  await page.locator('#from').fill('');
  await expect(page.locator('#from')).toHaveAttribute('aria-invalid', 'true');
  await expect(page.locator('#range-error')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Export CSV', exact: true })).toBeDisabled();
});

test('theme and language changes persist across tab navigation without overflow', async ({ page }) => {
  await page.setViewportSize({ width: 880, height: 600 });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const theme = page.locator('select').filter({ has: page.locator('option[value="dark"]') });
  await theme.selectOption('light');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  const language = page.locator('select').filter({ has: page.locator('option[value="zh-CN"]') });
  await language.selectOption('zh-CN');
  await expect(page.getByRole('button', { name: '设置', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '历史', exact: true }).click();
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await expect(theme).toHaveValue('light');
  await expect(language).toHaveValue('zh-CN');
  expect(await page.locator('main').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.screenshot({ path: test.info().outputPath('settings-zh-light.png') });
});

test('clipboard failures show a useful error instead of a copied confirmation', async ({ page }) => {
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await expect(page.locator('tbody tr').first()).toBeVisible();
  await page.evaluate(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) }, configurable: true });
    document.execCommand = () => false;
  });
  await page.getByRole('button', { name: 'Copy as CSV', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText(/clipboard|copy/i);
  await expect(page.getByRole('button', { name: 'Copied', exact: true })).toHaveCount(0);
});

test('popover expands scoped windows and sidebar can pin via keyboard', async ({ page }) => {
  await page.goto('/popover');
  await expect(page.getByRole('heading', { name: 'Claude Usage' })).toBeVisible();
  const more = page.getByRole('button', { name: /More limits/ });
  await more.click();
  await expect(more).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('.scoped')).toContainText('Opus');
  await page.goto('/');
  const claude = page.getByRole('button', { name: /^Claude:/ });
  await claude.focus();
  await claude.press('Enter');
  await expect(claude).toHaveClass(/pinned/);
  await claude.press('Enter');
  await expect(claude).not.toHaveClass(/pinned/);
});

test('dragging the bar is not a click, and a small press still pins', async ({ page }) => {
  await page.goto('/');
  const claude = page.getByRole('button', { name: /^Claude:/ });
  const pill = page.locator('.pill');
  const box = (await claude.boundingBox())!;
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;

  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 2, y + 1);
  await expect(pill).not.toHaveAttribute('data-dragging', '');
  await page.mouse.up();
  await expect(claude).toHaveClass(/pinned/);
  await claude.click();
  await expect(claude).not.toHaveClass(/pinned/);

  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 4, y + 30, { steps: 4 });
  await expect(pill).toHaveAttribute('data-dragging', '');
  await page.mouse.up();
  await expect(pill).not.toHaveAttribute('data-dragging', '');
  await expect(claude).not.toHaveClass(/pinned/);
});
