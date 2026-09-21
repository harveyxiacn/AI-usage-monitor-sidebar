import { defineConfig, devices } from '@playwright/test';

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
        baseURL: 'http://127.0.0.1:1420',
        launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
          ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH }
          : {},
      },
    },
  ],
  webServer: {
    command: 'pnpm dev --host 127.0.0.1',
    url: 'http://127.0.0.1:1420',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
