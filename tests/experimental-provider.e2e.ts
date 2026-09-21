// A provider the backend reports but the frontend was never taught about has
// to render anyway: a row in Settings, a monogram instead of a mark, its own
// colour picker, and — when its quota source was never verified against a
// live account — an "Experimental" badge and a switch that starts off.
// The mock backend reports Copilot in exactly that state (mock.ts).
import { test as base, expect } from '@playwright/test';

const test = base.extend<{ runtimeErrors: void }>({
  runtimeErrors: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await use();
    expect(errors).toEqual([]);
  }, { auto: true }],
});

test('an experimental provider is listed, badged and switched off', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();

  const row = page.locator('.prow').filter({ hasText: 'GitHub Copilot' });
  await expect(row).toHaveCount(1);
  await expect(row.getByText('Experimental')).toBeVisible();

  // Off until its credentials are found — an unverified quota source must
  // never appear on its own.
  const toggle = row.getByRole('checkbox');
  await expect(toggle).not.toBeChecked();
  await expect(page.locator('.prow')).toHaveCount(3);

  // No hand-written mark exists for it: the fallback monogram is drawn.
  await expect(row.locator('.plogo svg text')).toHaveText('C');

  // And it stays out of the sidebar while it is switched off.
  await page.goto('/');
  await expect(page.getByRole('button', { name: /Copilot/ })).toHaveCount(0);
});

test('an unknown provider gets a colour picker without a hand-written label', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();

  // "Claude accent" / "Codex accent" are translated; a provider with no i18n
  // key of its own is labelled from its name instead of being dropped.
  await expect(page.getByLabel('Claude accent', { exact: true })).toHaveValue('#ff5c1a');
  await expect(page.getByLabel('Copilot accent', { exact: true })).toHaveValue('#8250df');
});

test('the built-in provider accents are unchanged by the ramp refactor', async ({ page }) => {
  await page.goto('/');
  // The concentric group's outermost arc must still paint the hand-tuned
  // theme.css value, not the neutral fallback the refactor added behind it.
  const stroke = await page
    .locator('.ring').first()
    .locator('circle[stroke]:not([stroke="transparent"])').nth(1)
    .evaluate((el) => getComputedStyle(el).stroke);
  expect(stroke).toBe('rgb(255, 92, 26)'); // --accent-claude-1
});
