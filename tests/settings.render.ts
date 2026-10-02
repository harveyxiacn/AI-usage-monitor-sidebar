import { test, expect } from '@playwright/test';

// Deliberately no pointer, keyboard, dispatchEvent, or element.click calls.
// These checks load the real settings route and observe what it renders.
const SECTION_IDS = [
  'appearance', 'presets', 'sidebarItems', 'sizeColour', 'position', 'behaviour', 'notifications',
  'shortcuts', 'providers', 'data', 'integrations', 'updates', 'privacy', 'backup', 'about',
];

for (const language of ['en', 'zh-CN']) {
  test(`settings tab renders every card and its index (${language})`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.setViewportSize({ width: 1100, height: 800 });
    const settings = encodeURIComponent(JSON.stringify({ language }));
    await page.goto(`/dashboard?tab=settings&settings=${settings}`);
    for (const id of SECTION_IDS) await expect(page.locator(`article#${id}`)).toBeAttached();
    // one index link per card, the search box and the global reset
    await expect(page.locator('nav .index a')).toHaveCount(SECTION_IDS.length);
    await expect(page.getByRole('searchbox')).toBeVisible();
    // a card with something to reset starts with its button disabled
    await expect(page.locator('article#position button.reset')).toBeDisabled();
    // the privacy card lists every request the app can make
    await expect(page.locator('article#privacy li')).toHaveCount(12);
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
