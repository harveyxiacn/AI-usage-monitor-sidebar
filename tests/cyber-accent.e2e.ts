import { test, expect } from '@playwright/test';

// Settings.cyberAccent — the neon pair the "Cyber HUD" surface is painted
// with. It only exists for that surface, and the size-and-colour preview has
// to show the chosen pair, not the default one.

const surface = (page: import('@playwright/test').Page) =>
  page.locator('select').filter({ has: page.locator('option[value="cyber"]') });
const accent = (page: import('@playwright/test').Page) =>
  page.locator('select').filter({ has: page.locator('option[value="synthwave"]') });

/** The resolved `--cy-a` of the live preview plate inside the dashboard. */
const previewAccent = (page: import('@playwright/test').Page) =>
  page
    .locator('.stage.surface')
    .evaluate((el) => getComputedStyle(el).getPropertyValue('--cy-a').trim());

test('the accent selector appears only for the cyber surface and repaints the preview', async ({
  page,
}) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();

  // glass is the default: no accent selector at all
  await expect(accent(page)).toHaveCount(0);

  await surface(page).selectOption('cyber');
  await expect(page.locator('html')).toHaveAttribute('data-surface', 'cyber');
  await expect(accent(page)).toHaveValue('neon');
  expect(await previewAccent(page)).toBe('#00e5ff');

  await accent(page).selectOption('matrix');
  await expect(page.locator('html')).toHaveAttribute('data-cyber', 'matrix');
  expect(await previewAccent(page)).toBe('#34ff9b');

  await accent(page).selectOption('amber');
  expect(await previewAccent(page)).toBe('#ffb300');

  // back to a non-HUD surface: the selector goes away, the choice is kept
  await surface(page).selectOption('solid');
  await expect(accent(page)).toHaveCount(0);
  await surface(page).selectOption('cyber');
  await expect(accent(page)).toHaveValue('amber');
});

test('every preset defines its own pair and keeps the HUD text legible', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await surface(page).selectOption('cyber');

  const seen = new Set<string>();
  for (const preset of ['neon', 'matrix', 'amber', 'ice', 'synthwave']) {
    await accent(page).selectOption(preset);
    const tokens = await page.locator('.stage.surface').evaluate((el) => {
      const css = getComputedStyle(el);
      const get = (name: string) => css.getPropertyValue(name).trim();
      return { a: get('--cy-a'), b: get('--cy-b'), text: get('--text'), muted: get('--muted') };
    });
    // a real pair, not the inherited default, and distinct from each other
    expect(tokens.a).toMatch(/^#[0-9a-f]{6}$/i);
    expect(tokens.b).toMatch(/^#[0-9a-f]{6}$/i);
    expect(tokens.a).not.toBe(tokens.b);
    expect(seen.has(tokens.a)).toBe(false);
    seen.add(tokens.a);
    // the plate is near-black in every preset, so the text tokens are pale
    for (const colour of [tokens.text, tokens.muted]) {
      const [r, g, b] = [1, 3, 5].map((i) => parseInt(colour.slice(i, i + 2), 16));
      expect(0.2126 * r + 0.7152 * g + 0.0722 * b).toBeGreaterThan(140);
    }
  }
  expect(seen.size).toBe(5);
});
