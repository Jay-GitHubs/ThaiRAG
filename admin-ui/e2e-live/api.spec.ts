import { test, expect } from '@playwright/test';
import { API_ORIGIN } from './fixtures';

/** Public API surface — no browser session needed. */

test('health endpoint is ok', async ({ request }) => {
  const res = await request.get(`${API_ORIGIN}/health`);
  expect(res.ok()).toBeTruthy();
  expect(await res.json()).toMatchObject({ status: 'ok' });
});

test('deep health reports provider readiness', async ({ request }) => {
  const res = await request.get(`${API_ORIGIN}/health?deep=true`);
  // 200 = all providers reachable; 503 = degraded (with per-provider detail).
  // Either is a valid shape — what we assert is a structured answer, not a hang.
  expect([200, 503]).toContain(res.status());
  const body = await res.json();
  expect(body.checks).toBeTruthy();
  if (res.status() === 503) {
    console.log(
      'deep health DEGRADED:',
      Object.entries(body.checks)
        .filter(([, c]: [string, any]) => c.status !== 'ok')
        .map(([k, c]: [string, any]) => `${k}: ${c.detail}`)
        .join('; '),
    );
  }
});

test('/v1/models lists ThaiRAG-1.0 without auth', async ({ request }) => {
  const res = await request.get(`${API_ORIGIN}/v1/models`);
  expect(res.ok()).toBeTruthy();
  const body = await res.json();
  const ids = (body.data ?? []).map((m: { id: string }) => m.id);
  expect(ids).toContain('ThaiRAG-1.0');
});

test('chat completions REJECTS unauthenticated requests', async ({ request }) => {
  const res = await request.post(`${API_ORIGIN}/v1/chat/completions`, {
    data: {
      model: 'ThaiRAG-1.0',
      messages: [{ role: 'user', content: 'hi' }],
      stream: false,
    },
  });
  expect(res.status()).toBe(401);
});

test('chat completions answers with a valid API key', async ({ request }) => {
  const key = process.env.LIVE_API_KEY;
  test.skip(!key, 'LIVE_API_KEY not set — skipping authenticated /v1 test');
  const res = await request.post(`${API_ORIGIN}/v1/chat/completions`, {
    headers: { 'X-API-Key': key! },
    data: {
      model: 'ThaiRAG-1.0',
      messages: [{ role: 'user', content: 'Reply with the single word: pong' }],
      stream: false,
    },
    timeout: 150_000,
  });
  expect(res.ok()).toBeTruthy();
  const body = await res.json();
  const content = body.choices?.[0]?.message?.content ?? '';
  expect(content.length).toBeGreaterThan(0);
});
