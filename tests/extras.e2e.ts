import { test, expect } from '@playwright/test';

// `ProviderQuota.extras` in the browser preview: the mock gives Codex two
// extras and Claude none, so the same data proves both the rendering and the
// "hidden when empty" rule.

test('the overview shows a provider’s extras and nothing at all without them', async ({ page }) => {
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();

  const cards = page.locator('.provider');
  await expect(cards).toHaveCount(2);
  const codex = cards.filter({ hasText: 'Codex' });
  const claude = cards.filter({ hasText: 'Claude' });

  await expect(codex.locator('.extras')).toBeVisible();
  await expect(codex.getByRole('listitem')).toHaveCount(2);
  await expect(codex.getByTitle('Reset credits: 2 (0)')).toBeVisible();
  const models = codex.getByTitle('Models unavailable: 1 (gpt-5.3-codex-spark)');
  await expect(models).toBeVisible();
  await expect(models).toHaveAttribute('data-severity', 'warn');

  // Claude reports no extras — the block must not appear at all
  await expect(claude.locator('.extras')).toHaveCount(0);
});

test('extra labels follow the UI language', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page
    .locator('select')
    .filter({ has: page.locator('option[value="zh-CN"]') })
    .selectOption('zh-CN');
  await page.getByRole('button', { name: '总览', exact: true }).click();
  await expect(page.locator('.provider').filter({ hasText: 'Codex' }).locator('.extras')).toContainText(
    '重置额度券'
  );
});

// The browser preview always renders the first snapshot provider (Claude) —
// the platform layer picks the target in the real app — so this covers the
// empty case only; the populated one is the dashboard test above, which
// renders the very same component.
test('the popover leaves no empty extras block behind', async ({ page }) => {
  await page.goto('/popover');
  await expect(page.getByRole('heading', { name: 'Claude Usage' })).toBeVisible();
  await expect(page.locator('.extras')).toHaveCount(0);
  await expect(page.locator('footer')).toBeVisible();
});
