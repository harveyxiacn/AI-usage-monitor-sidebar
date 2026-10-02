import { test, expect } from '@playwright/test';

// Deliberately no pointer, keyboard, dispatchEvent, or element.click calls.
// These checks load real routes with the `?mock=accounts` scenario (a personal
// and a work Claude account) and observe what is rendered.

for (const language of ['en', 'zh-CN']) {
  test(`the accounts card lists the configured account (${language})`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.setViewportSize({ width: 1100, height: 900 });
    const settings = encodeURIComponent(JSON.stringify({ language }));
    await page.goto(`/dashboard?tab=settings&mock=accounts&settings=${settings}`);
    const card = page.locator('article#accounts');
    await expect(card).toBeAttached();
    const row = card.locator('[data-account="work"]');
    await expect(row).toBeVisible();
    await expect(row).toContainText('Work');
    await expect(row).toContainText('/home/you/.claude-work');
    // the existence check of the mock backend finds folder and credentials
    await expect(row.locator('.astatus')).toHaveAttribute('data-tone', 'ok');
    expect(errors).toEqual([]);
    await card.screenshot({ path: `test-results/accounts-card-${language}.png` });
  });
}

test('the overview shows one card per account and the history has an account selector', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1100, height: 900 });
  await page.goto('/dashboard?tab=overview&mock=accounts');
  const cards = page.locator('article.provider');
  await expect(cards).toHaveCount(3);
  await expect(cards.filter({ hasText: 'Claude · Work' })).toHaveCount(1);
  await expect(cards.filter({ hasText: 'you@work.example' })).toHaveCount(1);
  await page.screenshot({ path: 'test-results/accounts-overview.png', fullPage: true });

  await page.goto('/dashboard?tab=history&view=quota&mock=accounts');
  await expect(page.getByLabel('Account')).toBeVisible();
  expect(errors).toEqual([]);
});

test('without extra accounts there is no account selector and no extra card', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1100, height: 900 });
  await page.goto('/dashboard?tab=overview');
  await expect(page.locator('article.provider')).toHaveCount(2);
  await page.goto('/dashboard?tab=history&view=quota');
  await expect(page.getByText('Quota history', { exact: false }).first()).toBeVisible();
  await expect(page.getByLabel('Account')).toHaveCount(0);
  expect(errors).toEqual([]);
});

test('with an extra account the history, table and sessions name and filter by account', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1200, height: 1000 });
  // the work account has its own token history in the preview data
  await page.goto('/dashboard?tab=history&mock=accounts');
  const selector = page.locator('select#account');
  await expect(selector).toBeVisible();
  await expect(page.locator('table .account-chip').first()).toBeVisible();
  await selector.selectOption('work');
  await expect(page.locator('table .account-chip').first()).toBeVisible();
  await selector.selectOption('primary');
  await expect(page.locator('table .account-chip')).toHaveCount(0);
  await page.screenshot({ path: 'test-results/accounts-history.png', fullPage: true });

  await page.goto('/dashboard?tab=sessions&mock=accounts');
  await expect(page.locator('.session-row .account-chip').first()).toBeVisible();
  expect(errors).toEqual([]);
});

test('without extra accounts the history has no account selector, chip or column', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1200, height: 1000 });
  await page.goto('/dashboard?tab=history');
  await expect(page.locator('.table-wrap table').first()).toBeVisible();
  await expect(page.locator('select#account')).toHaveCount(0);
  await expect(page.locator('.account-chip')).toHaveCount(0);
  await page.goto('/dashboard?tab=sessions');
  await expect(page.locator('.session-row').first()).toBeVisible();
  await expect(page.locator('.account-chip')).toHaveCount(0);
  expect(errors).toEqual([]);
});
