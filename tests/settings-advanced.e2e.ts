import { test, expect } from '@playwright/test';
import { showAdvancedSettings } from './advanced-settings';

// Basic / advanced tiers of the Settings tab (docs/SETTINGS-AUDIT.md). The
// no-input render suite covers the default and the "all shown" layouts; this
// one drives the switch, the search and the palette like a user.

test.beforeEach(async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Appearance', exact: true })).toBeVisible();
});

test('advanced controls are hidden until the switch is on, and the choice is remembered', async ({ page }) => {
  const opacity = page.getByLabel('Opacity', { exact: true });
  await expect(opacity).toBeHidden();
  await expect(page.locator('article#sizeColour')).toBeHidden();
  await expect(page.locator('nav .index a', { hasText: 'Size & colour' })).toHaveCount(0);

  await showAdvancedSettings(page);
  await expect(opacity).toBeVisible();
  await expect(page.locator('article#sizeColour')).toBeVisible();
  await expect(page.locator('nav .index a', { hasText: 'Size & colour' })).toHaveCount(1);
  // switched on by the user: no badge, they know what they asked for
  await expect(page.locator('.adv-badge')).toHaveCount(0);

  await page.reload();
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByLabel('Opacity', { exact: true })).toBeVisible();
});

test('a search always finds advanced controls and marks them', async ({ page }) => {
  await page.getByRole('searchbox').fill('opacity');
  const row = page.locator('.setting-field').filter({ hasText: 'Opacity' });
  await expect(row).toBeVisible();
  await expect(row.locator('.adv-badge')).toHaveText('advanced');
  // a basic control that matches carries no badge
  await page.getByRole('searchbox').fill('theme');
  await expect(page.locator('.setting-field').filter({ hasText: 'Theme' }).locator('.adv-badge')).toHaveCount(0);

  // an advanced-only card is found through its controls too
  await page.getByRole('searchbox').fill('accent');
  await expect(page.locator('article#sizeColour')).toBeVisible();
  await page.getByRole('searchbox').fill('');
  await expect(page.locator('article#sizeColour')).toBeHidden();
});

test('the command palette still lists an advanced-only card and opens it', async ({ page }) => {
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox', { name: 'Command palette' }).fill('settings size');
  await expect(page.getByRole('option').first()).toContainText('Size & colour');
  await page.keyboard.press('Enter');
  await expect(page.locator('#sizeColour')).toBeVisible();
  await expect(page.locator('#sizeColour')).toBeInViewport();
});

test('a card reset confirms and says it includes the advanced settings', async ({ page }) => {
  await page.getByLabel('Theme', { exact: true }).selectOption('light');
  let message = '';
  page.once('dialog', (dialog) => {
    message = dialog.message();
    void dialog.accept();
  });
  await page.locator('article#appearance button.reset').click();
  await expect.poll(() => message).toContain('including advanced');
  await expect(page.getByLabel('Theme', { exact: true })).toHaveValue('dark');
});
