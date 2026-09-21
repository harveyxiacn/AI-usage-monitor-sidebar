import { test as base, expect, type Page } from '@playwright/test';

const test = base.extend<{ runtimeErrors: void }>({
  runtimeErrors: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await use();
    expect(errors).toEqual([]);
  }, { auto: true }],
});

/** The row of the popover / overview card whose label is `label`. */
const row = (page: Page, label: string) => page.locator('.row').filter({ hasText: label }).first();
/** The settings row whose label is `label`. */
const field = (page: Page, label: string) =>
  page.locator('.setting-field').filter({ hasText: label }).first();

test('the popover shows a forecast line per window and leaves idle windows alone', async ({ page }) => {
  await page.goto('/popover');
  // the browser preview renders the first mock provider (Claude)
  await expect(page.getByRole('heading', { name: 'Claude Usage' })).toBeVisible();

  // the 5-hour window runs out before it resets → countdown, warning colour
  const session = row(page, 'Current session');
  await expect(session.locator('.forecast')).toHaveText(/^Runs out in ~\d+ min$/);
  await expect(session.locator('.forecast.warn')).toBeVisible();

  // the weekly window only drifts upwards → a muted "on pace" statement
  const weekly = row(page, 'Weekly');
  await expect(weekly.locator('.forecast')).toHaveText('On pace for 74% at reset');
  await expect(weekly.locator('.forecast.warn')).toHaveCount(0);

  // a window the backend could not forecast shows no line at all
  await page.getByRole('button', { name: /More limits/ }).click();
  await expect(row(page, 'Fable').locator('.forecast')).toHaveCount(0);
  await expect(row(page, 'Opus').locator('.forecast')).toHaveText('On pace for 96% at reset');

  await page.screenshot({ path: test.info().outputPath('popover-forecast.png') });
});

test('the forecast wording follows percentMode and the overview repeats it', async ({ page }) => {
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();
  await expect(row(page, '5-hour').locator('.forecast')).toHaveText(/^Runs out in ~\d+ min$/);
  await expect(row(page, 'Weekly').locator('.forecast')).toHaveText('On pace for 74% at reset');

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByLabel('Percent shows').selectOption('remaining');
  await page.getByRole('button', { name: 'Overview', exact: true }).click();

  // the "used" wording flips to the remaining side…
  await expect(row(page, 'Weekly').locator('.forecast')).toHaveText('On pace for 26% left at reset');
  // …while a countdown is a countdown in either mode
  await expect(row(page, '5-hour').locator('.forecast')).toHaveText(/^Runs out in ~\d+ min$/);
});

test('the predictive notification toggle follows the notifications setting', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();

  const notify = field(page, 'Notify near the limit');
  const predict = field(page, 'Warn when on pace to run out early');
  await expect(predict.getByRole('checkbox')).toBeChecked();
  await expect(predict.getByRole('checkbox')).toBeDisabled();

  // the switch input is visually hidden; users click its label
  await notify.locator('label').click();
  await expect(predict.getByRole('checkbox')).toBeEnabled();
  await predict.locator('label').click();
  await expect(predict.getByRole('checkbox')).not.toBeChecked();
});
