import { defineConfig } from '@playwright/test';

// Live-stack e2e: assumes the Docker stack is up — chat-ui on :8082 and the
// ThaiRAG backend on :8080 (same convention as admin-ui's suite). Headed by
// default; run with `npm run test:e2e`.
export default defineConfig({
  testDir: './e2e',
  globalTeardown: './e2e/global-teardown.ts',
  timeout: 300_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  retries: 1,
  reporter: 'list',

  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:8082',
    headless: false,
    viewport: { width: 1280, height: 720 },
    actionTimeout: 15_000,
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
