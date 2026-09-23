// Browser-preview coverage for the update surface and the shortcut fields.
// The mock backend offers version 9.9.9 on the first explicit check and
// reports `canInstall: false`, i.e. the package-manager branch.
import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'About', exact: true })).toBeVisible();
});

test('a manual check surfaces the offer in the About card and in the banner', async ({ page }) => {
  await expect(page.getByText('Not checked yet')).toBeVisible();
  // nothing may appear before the user asks
  await expect(page.getByText(/Update 9\.9\.9 available/)).toHaveCount(0);

  await page.getByRole('button', { name: 'Check program updates', exact: true }).click();

  await expect(page.getByText('Update 9.9.9 available', { exact: true })).toBeVisible();
  // the preview is not a bundle we may replace, so no install button
  await expect(page.getByRole('button', { name: 'Install and restart' })).toHaveCount(0);
  await expect(page.getByText(/installed by a package manager/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Open the release page' })).toBeVisible();

  const banner = page.locator('.update-bar');
  await expect(banner).toContainText('Version 9.9.9 is available.');
  await banner.getByRole('button', { name: 'Dismiss', exact: true }).click();
  await expect(banner).toHaveCount(0);
});

test('a global shortcut is validated before it is saved', async ({ page }) => {
  const field = page.getByLabel('Shortcut: show/hide bar', { exact: true });
  await expect(field).toHaveValue('');

  await field.fill('U');
  await field.blur();
  await expect(page.getByText('Not a usable shortcut. Use modifiers and one key, e.g. Ctrl+Alt+U.')).toBeVisible();

  await field.fill('Ctrl+Alt+U');
  await field.blur();
  await expect(page.getByText('Not a usable shortcut. Use modifiers and one key, e.g. Ctrl+Alt+U.')).toHaveCount(0);
  await expect(field).toHaveValue('Ctrl+Alt+U');

  // empty is a valid value again: it simply turns the shortcut off
  await field.fill('');
  await field.blur();
  await expect(page.getByText('Empty = off. Example: Ctrl+Alt+U')).toBeVisible();
});
