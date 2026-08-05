import { defineConfig } from '@playwright/test';

/**
 * Live smoke suite against the production domains
 * (admin/chat/api.thai-rag.com). No credentials anywhere: the `live-login`
 * setup project opens a headed browser and WAITS for a human to log in on
 * both UIs, then captures each origin's sessionStorage (both UIs keep the
 * JWT there — Playwright's storageState does NOT cover sessionStorage,
 * hence the custom capture + init-script injection in fixtures.ts).
 *
 * Run (headed is the point — you must be present to log in):
 *   cd admin-ui && npx playwright test -c playwright.live.config.ts --headed
 *
 * Optional: LIVE_API_KEY=<static /v1 key> enables the authenticated API tests.
 */
export default defineConfig({
  testDir: './e2e-live',
  timeout: 180_000,
  workers: 1, // production target + one shared human login — keep it serial
  retries: 0,
  reporter: [['list']],
  use: {
    trace: 'retain-on-failure',
    actionTimeout: 20_000,
  },
  projects: [
    { name: 'live-login', testMatch: /live-login\.setup\.ts/ },
    {
      name: 'live',
      testMatch: /.*\.spec\.ts/,
      dependencies: ['live-login'],
    },
  ],
});
