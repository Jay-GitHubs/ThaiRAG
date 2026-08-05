import { test as base, expect } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Deployment endpoints come from the environment ONLY — never hardcode real
 * hostnames here: the repo is public and the source must not advertise the
 * infrastructure it is tested against. Set them via env vars or the
 * gitignored `e2e-live/.env.live` (see `.env.live.example`).
 */
function requiredEnv(name: string): string {
  const v = process.env[name];
  if (!v) {
    throw new Error(
      `${name} is not set. Export it or copy e2e-live/.env.live.example to ` +
        `e2e-live/.env.live (gitignored) and fill in your deployment's URLs.`,
    );
  }
  return v.replace(/\/+$/, '');
}

export const ADMIN_ORIGIN = requiredEnv('LIVE_ADMIN_URL');
export const CHAT_ORIGIN = requiredEnv('LIVE_CHAT_URL');
export const API_ORIGIN = requiredEnv('LIVE_API_URL');
export const AUTH_FILE = path.join(__dirname, '.auth', 'live-session.json');

export function loadSessions(): Record<string, Record<string, string>> {
  return JSON.parse(fs.readFileSync(AUTH_FILE, 'utf8'));
}

/** Bearer token captured from a UI origin's sessionStorage (for API cleanup). */
export function tokenFor(origin: string): string | undefined {
  const s = loadSessions()[origin] ?? {};
  return s['thairag-token'] ?? s['thairag-chat-token'];
}

/**
 * Test base that re-injects the manually captured sessionStorage into every
 * page before app code runs (init scripts fire on each navigation, so
 * reloads keep the login too), and suppresses the admin guided tours.
 */
export const test = base.extend({
  context: async ({ context }, use) => {
    const sessions = loadSessions();
    await context.addInitScript((all: Record<string, Record<string, string>>) => {
      const mine = all[location.origin];
      if (mine) {
        for (const [k, v] of Object.entries(mine)) sessionStorage.setItem(k, v);
      }
      localStorage.setItem('thairag-tour-state', '{}');
      localStorage.setItem('thairag-quickstart-dismissed', 'true');
    }, sessions);
    await use(context);
  },
});

export { expect };
