import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';

const cwd = path.resolve(import.meta.dirname, '..');
const port = Number(process.env.RENDER_PORT ?? 14897);
const origin = `http://127.0.0.1:${port}`;
export default defineConfig({
  testDir: import.meta.dirname, testMatch: '**/*.render.ts', workers: 1, reporter: 'list',
  outputDir: path.join(cwd, 'test-results/render'),
  use: { ...devices['Desktop Chrome'], baseURL: origin, locale: 'en-US', timezoneId: 'Asia/Shanghai',
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH } : {} },
  webServer: { command: 'node tests/render-server.mjs', cwd, url: origin, reuseExistingServer: false, timeout: 60000 },
});
