import { request } from '@playwright/test';
import { API_BASE, TEST_EMAIL, TEST_PASSWORD } from './helpers';

// The specs create conversations against the live backend and (by design)
// don't delete them mid-run — an interrupted spec must not orphan half a
// conversation. Instead the SUITE cleans up after itself here: the test
// account is dedicated, so every conversation on it is suite-produced.
// Without this, runs accumulated 600+ leaked conversations (2026-07-13 UX
// audit finding). Deletes are paced for the per-user rate limiter.
export default async function globalTeardown() {
  const ctx = await request.newContext();
  try {
    const login = await ctx.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    if (!login.ok()) return;
    const { token } = await login.json();
    const headers = { Authorization: `Bearer ${token}` };

    // Bounded sweep: a few passes with backoff instead of an unbounded loop,
    // so teardown can never hang the suite.
    for (let pass = 0; pass < 10; pass++) {
      const listRes = await ctx.get(`${API_BASE}/api/chat/conversations`, { headers });
      if (!listRes.ok()) break;
      const list = (await listRes.json()) as Array<{ id: string }>;
      if (!Array.isArray(list) || list.length === 0) break;
      let rateLimited = false;
      for (const c of list) {
        const del = await ctx.delete(`${API_BASE}/api/chat/conversations/${c.id}`, { headers });
        if (del.status() === 429) {
          rateLimited = true;
          break;
        }
      }
      if (rateLimited) await new Promise((r) => setTimeout(r, 20_000));
    }
  } finally {
    await ctx.dispose();
  }
}
