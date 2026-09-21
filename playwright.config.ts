import { defineConfig, devices } from '@playwright/test';

// Every checkout gets its own dev-server port (override with E2E_PORT). With a
// fixed port and `reuseExistingServer`, a second worktree running the suite at
// the same time silently tested the *other* checkout's app.
function portFor(dir: string): number {
  let hash = 0;
  for (const ch of dir) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return 14200 + (hash % 800);
}
const port = Number(process.env.E2E_PORT) || portFor(process.cwd());
const origin = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: './tests',
  globalSetup: './tests/warmup.ts',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: 'list',
  use: { trace: 'retain-on-failure', screenshot: 'only-on-failure' },
  projects: [
    {
      name: 'chromium',
      testMatch: '**/*.e2e.ts',
      use: {
        ...devices['Desktop Chrome'],
        locale: 'en-US',
        timezoneId: 'Asia/Singapore',
        baseURL: origin,
        launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
          ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH }
          : {},
      },
    },
  ],
  webServer: {
    command: `pnpm dev --host 127.0.0.1 --port ${port} --strictPort`,
    url: origin,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
