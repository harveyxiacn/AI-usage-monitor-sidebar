// Browser-preview coverage for the update surface and the shortcut fields.
// The mock backend offers version 9.9.9 on the first explicit check and
// reports `canInstall: false`, i.e. the package-manager branch.
import { test, expect } from '@playwright/test';
import { showAdvancedSettings } from './advanced-settings';

test.beforeEach(async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'About', exact: true })).toBeVisible();
  await showAdvancedSettings(page); // the Shortcuts card is advanced-only
});

test('a manual check surfaces the offer in the Updates card and in the banner', async ({ page }) => {
  await expect(page.getByText('Not checked yet', { exact: true })).toBeVisible();
  // nothing may appear before the user asks
  await expect(page.getByText(/Update 9\.9\.9 available/)).toHaveCount(0);

  await page.getByRole('button', { name: 'Check for app updates', exact: true }).click();

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

test('the shortcut recorder only accepts a modifier plus a key', async ({ page }) => {
  const recorder = page.getByRole('button', { name: 'Shortcut: show/hide bar', exact: true });
  await expect(recorder).toContainText('Not set');

  // a bare key is refused and the recorder keeps listening
  await recorder.click();
  await page.keyboard.press('KeyU');
  await expect(page.getByText('Hold Ctrl, Alt, Shift or Super as well.')).toBeVisible();

  await page.keyboard.press('Control+Alt+KeyU');
  await expect(recorder).toContainText('Ctrl+Alt+U');
  await expect(page.getByText('Registered', { exact: true }).first()).toBeVisible();

  // Escape cancels without changing anything, Backspace clears
  await recorder.click();
  await page.keyboard.press('Escape');
  await expect(recorder).toContainText('Ctrl+Alt+U');
  await recorder.click();
  await page.keyboard.press('Backspace');
  await expect(recorder).toContainText('Not set');
});
