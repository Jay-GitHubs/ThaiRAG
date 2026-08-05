import { defineConfig } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

// Load e2e-live/.env.live (gitignored) so endpoints and the optional API key
// never live in the repo. Already-exported env vars win over the file.
const envFile = path.join(path.dirname(fileURLToPath(import.meta.url)), 'e2e-live', '.env.live');
if (fs.existsSync(envFile)) {
  for (const line of fs.readFileSync(envFile, 'utf8').split('\n')) {
    const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/);
    if (m && !(m[1] in process.env)) process.env[m[1]] = m[2];
  }
}

/**
 * Live smoke suite against the production domains
 * (endpoints from env / gitignored .env.live). No credentials anywhere: the `live-login`
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
