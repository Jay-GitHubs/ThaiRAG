import { test as base, expect } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export const ADMIN_ORIGIN = 'https://admin.thai-rag.com';
export const CHAT_ORIGIN = 'https://chat.thai-rag.com';
export const API_ORIGIN = 'https://api.thai-rag.com';
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
