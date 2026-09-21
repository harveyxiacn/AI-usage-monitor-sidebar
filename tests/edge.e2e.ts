// Docking the bar to the top / bottom screen edge. The sidebar window has no
// UI of its own for the edge, so the mock backend is seeded through the
// `?settings=` query parameter (see src/lib/mock.ts).
import { test, expect, type Page } from '@playwright/test';

/** Centres of the ring buttons, in DOM order. */
async function ringCentres(page: Page) {
  const slots = page.locator('.slot[role="button"]');
  await expect(slots.first()).toBeVisible();
  const boxes = await slots.evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    })
  );
  expect(boxes.length).toBeGreaterThan(1);
  return boxes;
}

test('a top-edge bar lays its rings out in a row and keeps the labels and the ⋯ button', async ({ page }) => {
  await page.goto('/?settings=' + encodeURIComponent('{"edge":"top"}'));
  const stage = page.locator('.stage');
  await expect(stage).toHaveAttribute('data-edge', 'top');

  const pill = page.locator('.pill');
  await expect(pill).toHaveCSS('flex-direction', 'row');
  // The docked side is flush with the screen: no rounding, no border there.
  await expect(pill).toHaveCSS('border-top-left-radius', '0px');
  await expect(pill).toHaveCSS('border-top-width', '0px');

  const centres = await ringCentres(page);
  for (let i = 1; i < centres.length; i += 1) {
    expect(centres[i].x).toBeGreaterThan(centres[i - 1].x);
    expect(Math.abs(centres[i].y - centres[i - 1].y)).toBeLessThan(2);
  }
  // A horizontal strip is wider than it is tall, and everything still fits.
  const box = (await pill.boundingBox())!;
  expect(box.width).toBeGreaterThan(box.height);
  await expect(page.locator('.pct').first()).toBeVisible();
  const dots = page.getByRole('button', { name: 'Open dashboard', exact: true });
  await expect(dots).toBeVisible();
  const dotsBox = (await dots.boundingBox())!;
  expect(dotsBox.x).toBeGreaterThan(centres[centres.length - 1].x);
  expect(dotsBox.x + dotsBox.width).toBeLessThanOrEqual(box.x + box.width + 1);
});

test('the default right-edge bar still stacks its rings in a column', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.stage')).toHaveAttribute('data-edge', 'right');
  await expect(page.locator('.pill')).toHaveCSS('flex-direction', 'column');
  const centres = await ringCentres(page);
  expect(centres[1].y).toBeGreaterThan(centres[0].y);
  expect(Math.abs(centres[1].x - centres[0].x)).toBeLessThan(2);
});

test('the popover tail points back at the bar on every edge', async ({ page }) => {
  // Top edge: the bubble opens below the bar, so the tail is on its top side
  // and slides along x.
  await page.goto('/popover?settings=' + encodeURIComponent('{"edge":"top"}'));
  const root = page.locator('.root');
  await expect(root).toHaveAttribute('data-edge', 'top');
  await expect(root).toHaveCSS('padding-top', '8px');
  const tail = page.locator('.tail');
  let tailBox = (await tail.boundingBox())!;
  let rootBox = (await root.boundingBox())!;
  expect(tailBox.y).toBeLessThan(rootBox.y + 8);
  expect(Math.abs(tailBox.x + tailBox.width / 2 - (rootBox.x + rootBox.width / 2))).toBeLessThan(2);

  // Bottom edge: the bubble opens above it, tail on the bottom side.
  await page.goto('/popover?settings=' + encodeURIComponent('{"edge":"bottom"}'));
  await expect(root).toHaveAttribute('data-edge', 'bottom');
  await expect(root).toHaveCSS('padding-bottom', '8px');
  tailBox = (await tail.boundingBox())!;
  rootBox = (await root.boundingBox())!;
  expect(tailBox.y + tailBox.height).toBeGreaterThan(rootBox.y + rootBox.height - 8);

  // Right edge (the default): tail on the right, sliding along y.
  await page.goto('/popover');
  await expect(root).toHaveAttribute('data-edge', 'right');
  await expect(root).toHaveCSS('padding-right', '8px');
  tailBox = (await tail.boundingBox())!;
  rootBox = (await root.boundingBox())!;
  expect(tailBox.x + tailBox.width).toBeGreaterThan(rootBox.x + rootBox.width - 8);
});
