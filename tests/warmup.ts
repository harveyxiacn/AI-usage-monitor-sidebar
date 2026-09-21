// Playwright global setup. [FRONTEND]
//
// A cold Vite dev server discovers dependencies on the first visit of each
// route, re-bundles them and *reloads the page* — which used to fail whichever
// tests happened to run first. Visiting every route once, before any test,
// moves that reload out of the suite.
import { chromium, type FullConfig } from '@playwright/test';

export default async function warmup(config: FullConfig) {
  const project = config.projects.find((p) => p.name === 'chromium');
  const baseURL = project?.use.baseURL;
  if (!baseURL) return;
  const browser = await chromium.launch(project.use.launchOptions);
  const page = await browser.newPage();
  for (const route of ['/', '/popover', '/dashboard']) {
    // twice: the second load is the one after Vite's optimise-and-reload
    for (let pass = 0; pass < 2; pass++) {
      await page.goto(baseURL + route, { waitUntil: 'networkidle' }).catch(() => {});
    }
  }
  await browser.close();
}
