import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: 'list',

  use: {
    baseURL: 'http://localhost:8081',
    headless: false,
    viewport: { width: 1280, height: 720 },
    actionTimeout: 10_000,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },

  projects: [
    {
      name: 'setup',
      testMatch: /auth\.setup\.ts/,
    },
    {
      name: 'e2e',
      dependencies: ['setup'],
      testMatch: /.*\.spec\.ts/,
    },
  ],
});
