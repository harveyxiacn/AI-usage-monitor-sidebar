// The sidebar route rendered with a seeded mock configuration
// (`?settings=<JSON patch>`, see src/lib/mock.ts) — one page load per
// configuration, because the bar window has no settings UI of its own.
import { test as base, expect, type Page } from '@playwright/test';
import type { SettingsPatch } from '../src/lib/settings-writer';

const test = base.extend<{ runtimeErrors: void }>({
  runtimeErrors: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await use();
    expect(errors).toEqual([]);
  }, { auto: true }],
});

/** Open the bar with `patch` applied to the mock settings. */
async function openBar(page: Page, patch: SettingsPatch = {}) {
  await page.goto(`/?settings=${encodeURIComponent(JSON.stringify(patch))}`);
  await expect(page.locator('.pill')).toBeVisible();
}

/** The provider mark lives inside the ring's centre span, next to the arcs. */
const logos = (page: Page) => page.locator('.slot .center svg');

test('by default the bar shows both providers with logo, percent and the ⋯ button', async ({ page }) => {
  await openBar(page);
  await expect(page.locator('.slot')).toHaveCount(2);
  await expect(page.locator('.pct')).toHaveCount(2);
  await expect(logos(page)).toHaveCount(2);
  await expect(page.locator('.dots')).toHaveCount(1);
  // three concentric arcs for Claude: weekly, 5-hour, per-model weekly
  await expect(page.getByRole('button', { name: /^Claude:/ })).toHaveAttribute(
    'aria-label',
    /Weekly.*5-hour.*Weekly · Fable/
  );
});

test('each element toggle removes exactly its own part of the bar', async ({ page }) => {
  await openBar(page, { sidebarItems: { percentLabel: false } });
  await expect(page.locator('.slot')).toHaveCount(2);
  await expect(page.locator('.pct')).toHaveCount(0);
  await expect(logos(page)).toHaveCount(2);

  await openBar(page, { sidebarItems: { logo: false } });
  await expect(logos(page)).toHaveCount(0);
  await expect(page.locator('.slot')).toHaveCount(2);
  await expect(page.locator('.pct')).toHaveCount(2);

  await openBar(page, { sidebarItems: { moreButton: false } });
  await expect(page.locator('.slot')).toHaveCount(2);
  await expect(page.locator('.dots')).toHaveCount(0);
});

test('window kinds and providers can be hidden from the bar independently', async ({ page }) => {
  // the per-model ring is the innermost arc of the Claude group
  await openBar(page, { sidebarItems: { scoped: false } });
  await expect(page.getByRole('button', { name: /^Claude:/ })).toHaveAttribute(
    'aria-label',
    /^Claude: Weekly \d+%, 5-hour \d+%$/
  );

  await openBar(page, { sidebarItems: { weekly: false } });
  await expect(page.getByRole('button', { name: /^Codex:/ })).toHaveAttribute(
    'aria-label',
    /^Codex: 5-hour \d+%$/
  );

  await openBar(page, { providers: { codex: { showInSidebar: false } } });
  await expect(page.locator('.slot')).toHaveCount(1);
  await expect(page.getByRole('button', { name: /^Claude:/ })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Codex:/ })).toHaveCount(0);
});

test('hiding every window still leaves a draggable grip with a non-zero box', async ({ page }) => {
  await openBar(page, {
    sidebarItems: { fiveHour: false, weekly: false, scoped: false, other: false, moreButton: false },
  });
  await expect(page.locator('.slot')).toHaveCount(1);
  await expect(page.getByRole('button', { name: /^Claude:/ })).toHaveCount(0);

  // the grip survives `moreButton: false` — the bar must never become unreachable
  const grip = page.locator('.dots');
  await expect(grip).toHaveCount(1);
  const box = (await page.locator('.stage').boundingBox())!;
  expect(box.width).toBeGreaterThan(0);
  expect(box.height).toBeGreaterThan(0);

  // and the pill still drags
  const pill = page.locator('.pill');
  const from = (await pill.boundingBox())!;
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(from.x + from.width / 2 - 6, from.y + from.height / 2 + 30, { steps: 4 });
  await expect(pill).toHaveAttribute('data-dragging', '');
  await page.mouse.up();
  await expect(pill).not.toHaveAttribute('data-dragging', '');
});

test('the settings tab toggles sidebar items and the preview follows the label switch', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Sidebar items' })).toBeVisible();

  // Toggle hides the real checkbox behind its track, so flip it by its label
  const toggle = (name: string) => page.getByRole('checkbox', { name, exact: true });
  const flip = (name: string) => toggle(name).locator('..').click();

  const preview = page.locator('.preview');
  await expect(preview.locator('.pct')).toHaveCount(1);
  await flip('Percent under each ring');
  await expect(toggle('Percent under each ring')).not.toBeChecked();
  await expect(preview.locator('.pct')).toHaveCount(0);

  await expect(preview.locator('.center svg')).toHaveCount(1);
  await flip('Provider logo');
  await expect(preview.locator('.center svg')).toHaveCount(0);

  // per-provider "Bar" switch sits next to the enable switch
  await flip('Codex — Show on the bar');
  await expect(toggle('Codex — Show on the bar')).not.toBeChecked();
  // switching the provider off entirely disables the bar switch
  await flip('Codex — Track this provider');
  await expect(toggle('Codex — Show on the bar')).toBeDisabled();
});

for (const ringMode of ['concentric', 'primary', 'all'] as const) {
  test(`${ringMode} rings and forecast ticks follow the remaining setting`, async ({ page }) => {
    const geometry = () => page.locator('.slot .arc').evaluateAll((arcs) => arcs.map((arc) => ({
      fraction: 1 - Number(arc.getAttribute('stroke-dashoffset')) / Number(arc.getAttribute('stroke-dasharray')),
      color: arc.getAttribute('stroke'),
    })));
    const ticks = () => page.locator('.slot .tick').evaluateAll((lines) => lines.map((line) => ({
      x: Number(line.getAttribute('x2')), y: Number(line.getAttribute('y2')),
      size: Number(line.closest('svg')!.getAttribute('viewBox')!.split(' ')[2]),
    })));
    await openBar(page, { ringMode, percentMode: 'used' });
    await expect(page.locator('.slot .arc').first()).toBeVisible();
    const used = await geometry();
    const usedTicks = await ticks();
    expect(used.length).toBeGreaterThan(0);
    expect(usedTicks.length).toBeGreaterThan(0);
    expect(used[0].fraction).toBeCloseTo(ringMode === 'concentric' ? 0.31 : 0.73, 8);

    await openBar(page, { ringMode, percentMode: 'remaining' });
    await expect(page.locator('.slot .arc')).toHaveCount(used.length);
    const remaining = await geometry();
    for (let i = 0; i < used.length; i++) {
      expect(remaining[i].fraction).toBeCloseTo(1 - used[i].fraction, 8);
      expect(remaining[i].color).toBe(used[i].color);
    }
    await expect(page.locator('.slot .pct').first()).toHaveText('27%');
    const remainingTicks = await ticks();
    expect(remainingTicks).toHaveLength(usedTicks.length);
    for (let i = 0; i < usedTicks.length; i++) {
      expect(remainingTicks[i].x + usedTicks[i].x).toBeCloseTo(usedTicks[i].size, 8);
      expect(remainingTicks[i].y).toBeCloseTo(usedTicks[i].y, 8);
    }
  });
}

test('changing percent mode updates the preview arcs immediately and reversibly', async ({ page }) => {
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const arcs = page.locator('.preview .arc');
  const fractions = () => arcs.evaluateAll((nodes) => nodes.map((node) =>
    1 - Number(node.getAttribute('stroke-dashoffset')) / Number(node.getAttribute('stroke-dasharray'))));
  await expect(arcs).toHaveCount(3);
  await page.getByLabel('Percent shows').selectOption('remaining');
  await expect.poll(fractions).toEqual([0.69, 0.27, 0.76]);
  await expect(page.locator('.preview .pct')).toHaveText('27%');
  await page.getByLabel('Percent shows').selectOption('used');
  await expect.poll(async () => (await fractions()).map((v) => Math.round(v * 100))).toEqual([31, 73, 24]);
  await expect(page.locator('.preview .pct')).toHaveText('73%');
});
