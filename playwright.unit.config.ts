import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.unit.ts',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  reporter: 'list',
});
