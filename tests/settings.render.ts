import { test, expect } from '@playwright/test';

// Deliberately no pointer, keyboard, dispatchEvent, or element.click calls.
// These checks load the real settings route and observe what it renders.
const SECTION_IDS = [
  'appearance', 'presets', 'sidebarItems', 'sizeColour', 'position', 'behaviour', 'notifications',
  'shortcuts', 'providers', 'accounts', 'data', 'integrations', 'advisor', 'updates', 'privacy', 'backup', 'about',
];
// cards made only of advanced settings: off the page and the index until "Show advanced settings"
const ADVANCED_ONLY = ['sizeColour', 'shortcuts', 'accounts', 'integrations', 'advisor'];
const BASIC_IDS = SECTION_IDS.filter((id) => !ADVANCED_ONLY.includes(id));
const ADVANCED_KEY = 'ai-usage-sidebar.showAdvancedSettings';

for (const language of ['en', 'zh-CN']) {
  test(`settings tab renders every card and its index (${language})`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.setViewportSize({ width: 1100, height: 800 });
    const settings = encodeURIComponent(JSON.stringify({ language }));
    await page.goto(`/dashboard?tab=settings&settings=${settings}`);
    for (const id of SECTION_IDS) await expect(page.locator(`article#${id}`)).toBeAttached();
    // by default only the basic cards: one index link each, the search box and the global reset
    for (const id of BASIC_IDS) await expect(page.locator(`article#${id}`)).toBeVisible();
    for (const id of ADVANCED_ONLY) await expect(page.locator(`article#${id}`)).toBeHidden();
    await expect(page.locator('nav .index a')).toHaveCount(BASIC_IDS.length);
    await expect(page.getByRole('checkbox', { name: /advanced settings|高级设置/ })).not.toBeChecked();
    await expect(page.getByRole('searchbox')).toBeVisible();
    // a card with something to reset starts with its button disabled
    await expect(page.locator('article#position button.reset')).toBeDisabled();
    // the privacy card lists every local read and every request the app can make
    await expect(page.locator('article#privacy li')).toHaveCount(13);
    expect(errors).toEqual([]);
    await page.screenshot({ path: `test-results/settings-${language}.png`, fullPage: true });
  });
}

test('settings index collapses to a select on a narrow window without overflow', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 700, height: 850 });
  await page.goto('/dashboard?tab=settings');
  await expect(page.locator('nav select.jump')).toBeVisible();
  await expect(page.locator('nav .index')).toBeHidden();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/settings-narrow.png', fullPage: true });
});

test('"Show advanced settings" (remembered per viewer) brings back every card and control', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1100, height: 800 });

  const visibleControls = () => page.locator('.setting-field:not([hidden])').count();

  await page.goto('/dashboard?tab=settings');
  // basic-tier controls are there, advanced ones are tucked away
  await expect(page.getByLabel('Theme', { exact: true })).toBeVisible();
  await expect(page.getByLabel('Opacity', { exact: true })).toBeHidden();
  await expect(page.locator('article#appearance')).toBeVisible();
  const basic = await visibleControls();

  await page.addInitScript((key) => window.localStorage.setItem(key, '1'), ADVANCED_KEY);
  await page.goto('/dashboard?tab=settings');
  await expect(page.getByRole('checkbox', { name: 'Show advanced settings' })).toBeChecked();
  for (const id of SECTION_IDS) await expect(page.locator(`article#${id}`)).toBeVisible();
  await expect(page.locator('nav .index a')).toHaveCount(SECTION_IDS.length);
  await expect(page.getByLabel('Opacity', { exact: true })).toBeVisible();
  const all = await visibleControls();
  console.log(`settings controls visible: ${basic} by default, ${all} with advanced settings`);
  expect(basic).toBeLessThan(all * 0.6);
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/settings-advanced.png', fullPage: true });
});
