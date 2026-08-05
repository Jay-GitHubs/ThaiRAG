import { test as setup, expect } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';
import { ADMIN_ORIGIN, CHAT_ORIGIN, AUTH_FILE } from './fixtures';

/**
 * Manual-login gate. Opens the real login pages and waits (up to 5 minutes
 * each) for a HUMAN to sign in — no credentials in code, env, or CI. After
 * each login it captures the origin's sessionStorage (where both UIs keep
 * the JWT) for injection into every test page.
 */
setup('manual login on live domains (ACTION REQUIRED)', async ({ page }) => {
  setup.setTimeout(660_000);

  // Reuse a recent session so re-runs are hands-free (JWTs outlive this).
  if (fs.existsSync(AUTH_FILE)) {
    const ageMs = Date.now() - fs.statSync(AUTH_FILE).mtimeMs;
    if (ageMs < 8 * 3600_000) {
      console.log('✓ reusing captured session (<8h old) — no login needed');
      return;
    }
  }

  const grabSession = () =>
    page.evaluate(() => {
      const out: Record<string, string> = {};
      for (let i = 0; i < sessionStorage.length; i++) {
        const k = sessionStorage.key(i)!;
        out[k] = sessionStorage.getItem(k)!;
      }
      return out;
    });

  const sessions: Record<string, Record<string, string>> = {};

  /* eslint-disable no-console */
  console.log('\n══════════════════════════════════════════════════════');
  console.log('  ACTION REQUIRED: log in as your admin user in the');
  console.log(`  opened browser window (${ADMIN_ORIGIN}).`);
  console.log('  You have 5 minutes.');
  console.log('══════════════════════════════════════════════════════\n');
  await page.goto(`${ADMIN_ORIGIN}/login`);
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({
    timeout: 300_000,
  });
  sessions[ADMIN_ORIGIN] = await grabSession();
  console.log('✓ admin session captured');

  console.log('\n══════════════════════════════════════════════════════');
  console.log(`  ACTION REQUIRED: now log in on ${CHAT_ORIGIN}.`);
  console.log('  You have 5 minutes.');
  console.log('══════════════════════════════════════════════════════\n');
  await page.goto(`${CHAT_ORIGIN}/login`);
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible({
    timeout: 300_000,
  });
  sessions[CHAT_ORIGIN] = await grabSession();
  console.log('✓ chat session captured — running the suite');

  fs.mkdirSync(path.dirname(AUTH_FILE), { recursive: true });
  fs.writeFileSync(AUTH_FILE, JSON.stringify(sessions, null, 2));
});
