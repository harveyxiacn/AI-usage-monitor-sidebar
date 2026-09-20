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
  await expect(page.getByRole('status').filter({ hasText: 'Some models have no price' })).toBeVisible();
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
