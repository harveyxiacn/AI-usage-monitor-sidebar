import { test as base, expect } from '@playwright/test';

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

test('Ctrl+K opens an accessible palette, arrows move, Enter runs, Escape closes and gives focus back', async ({ page }) => {
  const hint = page.getByRole('button', { name: /Command palette/ });
  await hint.focus();
  await page.keyboard.press('Control+k');

  const dialog = page.getByRole('dialog', { name: 'Command palette' });
  await expect(dialog).toBeVisible();
  const box = dialog.getByRole('combobox');
  await expect(box).toBeFocused();
  await expect(box).toHaveAttribute('aria-expanded', 'true');

  await box.fill('history cost');
  const options = dialog.getByRole('option');
  await expect(options.first()).toHaveText(/History · Cost/);
  await expect(options.first()).toHaveAttribute('aria-selected', 'true');
  await expect(box).toHaveAttribute('aria-activedescendant', 'palette-opt-0');

  await box.fill('');
  await page.keyboard.press('ArrowDown');
  await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(box).toHaveAttribute('aria-activedescendant', 'palette-opt-1');
  await page.keyboard.press('ArrowUp');
  await expect(options.first()).toHaveAttribute('aria-selected', 'true');

  // Tab never leaves the dialog
  await page.keyboard.press('Tab');
  await expect(box).toBeFocused();

  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(hint).toBeFocused();
});

test('running a navigation command switches the tab and a sub-view', async ({ page }) => {
  await page.keyboard.press('Control+k');
  const box = page.getByRole('combobox', { name: 'Command palette' });
  await box.fill('history quota');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Command palette' })).toBeHidden();
  await expect(page.getByRole('button', { name: 'History', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(page.getByRole('radio', { name: /Quota/ }).or(page.getByRole('button', { name: /Quota/ })).first()).toBeVisible();

  await page.keyboard.press('Control+k');
  await page.getByRole('combobox', { name: 'Command palette' }).fill('settings privacy');
  await page.keyboard.press('Enter');
  await expect(page.locator('#privacy')).toBeInViewport();
});

test('a setting toggle changes the setting and a query without a match says so', async ({ page }) => {
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox', { name: 'Command palette' }).fill('toggle auto-hide');
  await expect(page.getByRole('option').first()).toContainText('Auto-hide');
  await expect(page.getByRole('option').first()).toContainText('Off');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox', { name: 'Command palette' }).fill('toggle auto-hide');
  await expect(page.getByRole('option').first()).toContainText('On');

  await page.getByRole('combobox', { name: 'Command palette' }).fill('zzzzqqq');
  await expect(page.getByText('No matching command')).toBeVisible();
});

test('the share card renders a 1200x630 image with no e-mail and a hide-cost switch', async ({ page }) => {
  await page.getByRole('button', { name: 'Share card' }).click();
  const dialog = page.getByRole('dialog', { name: 'Share usage card' });
  await expect(dialog).toBeVisible();
  const canvas = dialog.locator('canvas');
  await expect(canvas).toHaveAttribute('width', '1200');
  await expect(canvas).toHaveAttribute('height', '630');
  // something was painted (the page background alone would be one colour)
  const distinct = await canvas.evaluate((el: HTMLCanvasElement) => {
    const ctx = el.getContext('2d')!;
    const data = ctx.getImageData(0, 0, el.width, el.height).data;
    const seen = new Set<number>();
    for (let i = 0; i < data.length; i += 4 * 97) seen.add((data[i] << 16) | (data[i + 1] << 8) | data[i + 2]);
    return seen.size;
  });
  expect(distinct).toBeGreaterThan(5);
  await expect(dialog).not.toContainText('@');
  await expect(dialog.getByRole('button', { name: 'Save PNG…' })).toBeEnabled();

  await dialog.getByLabel('Hide cost').check();
  await expect(dialog.getByLabel('Hide cost')).toBeChecked();

  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
});
