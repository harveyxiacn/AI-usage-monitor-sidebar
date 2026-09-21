import { test, expect } from '@playwright/test';

// Settings.hideAccountEmail — the screen-sharing switch. The mock signs both
// providers in as you@example.com.

test('hiding the account e-mail masks it on every overview card and survives tabs', async ({
  page,
}) => {
  await page.goto('/dashboard');
  const subs = page.locator('.provider .sub');
  await expect(subs).toHaveCount(2);
  for (const text of await subs.allTextContents()) expect(text).toContain('you@example.com');

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  // the switch is a visually hidden checkbox inside its label
  const row = page.locator('.setting-field').filter({ hasText: 'Hide account e-mail' });
  const box = row.locator('input[type="checkbox"]');
  await expect(box).not.toBeChecked();
  await row.locator('label.switch').click();
  await expect(box).toBeChecked();

  await page.getByRole('button', { name: 'Overview', exact: true }).click();
  await expect(subs).toHaveCount(2);
  for (const text of await subs.allTextContents()) {
    expect(text).toContain('y•••@e•••.com');
    expect(text).not.toContain('you@example.com');
  }
  // the address must not survive in a tooltip either
  for (const title of await subs.evaluateAll((els) => els.map((el) => el.getAttribute('title')))) {
    expect(title ?? '').not.toContain('you@example.com');
  }
  // …nor anywhere else in the rendered overview
  expect(await page.locator('main').innerHTML()).not.toContain('you@example.com');

  // the setting persists across tab navigation like every other one
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(box).toBeChecked();
  await page.getByRole('button', { name: 'Overview', exact: true }).click();
  await expect(subs.first()).toContainText('y•••@e•••.com');
});
