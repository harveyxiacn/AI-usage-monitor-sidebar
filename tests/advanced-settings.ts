import { expect, type Page } from '@playwright/test';

/**
 * On the Settings tab: flip "Show advanced settings" on, the way a user would.
 * Advanced controls (accents, shortcuts, price list, percent placement, …) are
 * not on the page until then; a search would find them too.
 */
export async function showAdvancedSettings(page: Page): Promise<void> {
  const box = page.getByRole('checkbox', { name: 'Show advanced settings', exact: true });
  await expect(box).toBeVisible();
  if (!(await box.isChecked())) await box.locator('..').click();
  await expect(box).toBeChecked();
}
