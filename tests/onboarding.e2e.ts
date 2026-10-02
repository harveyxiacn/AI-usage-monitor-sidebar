// Browser-preview coverage for the first-run wizard, the getting-started card,
// "What's new", the weekly summary card and "skip this version".
// Scenarios come from the mock backend: `?mock=firstrun` (nobody signed in, not
// onboarded), `?mock=upgraded` (a build with bundled notes, `lastSeenVersion`
// empty) and the default (an installed, introduced app).
import { test, expect } from '@playwright/test';

test('a fresh install walks the wizard and never sees it again', async ({ page }) => {
  await page.goto('/dashboard?mock=firstrun');
  const wizard = page.getByRole('dialog');
  await expect(wizard).toBeVisible();
  await expect(wizard.getByText('Step 1 of 3')).toBeVisible();

  // step 1: language switches the UI immediately
  await wizard.getByLabel('Language', { exact: true }).selectOption('zh-CN');
  await expect(wizard.getByText('第 1 步，共 3 步')).toBeVisible();
  await wizard.getByLabel('语言', { exact: true }).selectOption('en');
  await expect(wizard.getByText('Step 1 of 3')).toBeVisible();
  await wizard.getByRole('button', { name: 'Next' }).click();

  // step 2: a visual edge picker
  await expect(wizard.getByText('Step 2 of 3')).toBeVisible();
  const left = wizard.getByRole('radio', { name: 'Left' });
  await left.click();
  await expect(left).toHaveAttribute('aria-checked', 'true');
  await wizard.getByRole('button', { name: 'Next' }).click();

  // step 3: providers are detected, with the sign-in commands and a re-check
  await expect(wizard.getByText('Step 3 of 3')).toBeVisible();
  await expect(wizard.getByText('Not signed in').first()).toBeVisible();
  await expect(wizard.locator('code', { hasText: '/login' })).toBeVisible();
  await expect(wizard.locator('code', { hasText: 'codex login' })).toBeVisible();
  const notify = wizard.getByLabel('Notify me when a limit is getting close');
  await wizard.locator('label.switch:has(input[aria-label="Notify me when a limit is getting close"])').click();

  await expect(notify).toBeChecked();

  await wizard.getByRole('button', { name: 'Finish' }).click();
  await expect(wizard).toHaveCount(0);
});

test('skipping the wizard also finishes it', async ({ page }) => {
  await page.goto('/dashboard?mock=firstrun');
  const wizard = page.getByRole('dialog');
  await expect(wizard).toBeVisible();
  await wizard.getByRole('button', { name: 'Skip setup' }).click();
  await expect(wizard).toHaveCount(0);
});

test('the wizard never appears for an existing user', async ({ page }) => {
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('with nobody signed in the overview explains what to do', async ({ page }) => {
  await page.goto('/dashboard?mock=logged-out');
  const card = page.getByRole('article', { name: 'Get started' });
  await expect(card).toBeVisible();
  await expect(card.getByText('Not signed in')).toHaveCount(1);
  await expect(card.getByText(/CLI not found: no folder at ~\/\.claude/)).toBeVisible();

  await expect(card.locator('code', { hasText: /^claude$/ })).toBeVisible();
  await expect(card.getByText(/never reads or uploads credentials/)).toBeVisible();
  await expect(card.getByRole('button', { name: 'Check again' })).toBeEnabled();
});

test('the getting-started card stays away while a provider works', async ({ page }) => {
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  await expect(page.getByRole('article', { name: 'Get started' })).toHaveCount(0);
});

test('what is new shows the bundled notes once per version', async ({ page }) => {
  await page.goto('/dashboard?mock=upgraded');
  const panel = page.getByRole('complementary', { name: /What's new in 0\.5\.0/ });
  await expect(panel).toBeVisible();
  // current language section only, rendered as text (no raw HTML)
  await expect(panel.getByText(/Connect token usage to named sessions/)).toBeVisible();
  await expect(panel.getByText('新增「会话」工作区')).toHaveCount(0);
  await panel.getByRole('button', { name: 'Got it' }).click();
  await expect(panel).toHaveCount(0);
});

test('a version without bundled notes shows no panel', async ({ page }) => {
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: /What's new/ })).toHaveCount(0);
});

test('the weekly summary card reports last week', async ({ page }) => {
  await page.goto('/dashboard');
  const card = page.getByRole('article', { name: 'Last week' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('48.3M');
  await expect(card).toContainText('~$37.42');
  await expect(card).toContainText('14.2M');
  await expect(card).toContainText('estimate');
});

test('skipping a version hides its banner, a user can still update from About', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Check for app updates', exact: true }).click();
  const banner = page.locator('.update-bar');
  await expect(banner).toContainText('Version 9.9.9 is available.');
  await banner.getByRole('button', { name: 'Skip this version' }).click();
  await expect(banner).toHaveCount(0);
  // the About card still reports the offer: skipping only silences the banner
  await expect(page.getByText('Update 9.9.9 available', { exact: true })).toBeVisible();
});
