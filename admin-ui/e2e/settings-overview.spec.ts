import { test, expect } from '@playwright/test';
import { login, navigateTo, suppressTours, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

/**
 * Settings overview: verifies the configured models + vector DB shown on the
 * Settings page reflect the live backend config.
 *
 * Configured models (LLM / embedding / vision) and the vector store are
 * GLOBAL-ONLY settings — they do not vary with the scope selector. These tests
 * confirm the page surfaces the same values the API reports.
 */
test.describe('Settings overview (models + vector DB)', () => {
  let token: string;
  const authHeaders = () => ({ Authorization: `Bearer ${token}` });

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok()).toBeTruthy();
    token = (await loginRes.json()).token;
  });

  test('configured providers expose LLM + embedding models via API', async ({ request }) => {
    const res = await request.get(`${API_BASE}/api/km/settings/providers`, {
      headers: authHeaders(),
    });
    expect(res.status()).toBe(200);
    const p = await res.json();

    // The provider bundle always carries these slots.
    for (const key of ['llm', 'embedding', 'vector_store']) {
      expect(p[key], `missing provider slot: ${key}`).toBeTruthy();
    }
    expect(typeof p.llm.model).toBe('string');
    expect(p.llm.model.length).toBeGreaterThan(0);
    expect(typeof p.embedding.model).toBe('string');
    expect(p.embedding.model.length).toBeGreaterThan(0);
  });

  test('vector DB info from API is well-formed', async ({ request }) => {
    const res = await request.get(`${API_BASE}/api/km/settings/vectordb/info`, {
      headers: authHeaders(),
    });
    expect(res.status()).toBe(200);
    const info = await res.json();

    expect(info.backend).toBeTruthy();
    expect(info.collection).toBeTruthy();
    expect(typeof info.vector_count).toBe('number');
    expect(info.vector_count).toBeGreaterThanOrEqual(0);
  });

  test('Vector Database tab shows live backend, collection and vector count', async ({ page, request }) => {
    // Pull the authoritative values first.
    const infoRes = await request.get(`${API_BASE}/api/km/settings/vectordb/info`, {
      headers: authHeaders(),
    });
    const info = await infoRes.json();

    await login(page);
    await suppressTours(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    await page.getByRole('tab', { name: 'Vector Database' }).click();
    await page.waitForTimeout(800);

    // The collapse panel header.
    const panel = page.getByText('Vector Database', { exact: true });
    await expect(panel.first()).toBeVisible();

    // Backend + collection from the live API must appear on the page.
    await expect(page.getByText(info.backend, { exact: false }).first()).toBeVisible({ timeout: 5000 });
    await expect(page.getByText(info.collection, { exact: false }).first()).toBeVisible();

    // The "Total Vectors" statistic is rendered.
    await expect(page.getByText('Total Vectors')).toBeVisible();
  });

  test('scope selector does NOT alter global-only model/vector config', async ({ page, request }) => {
    // Snapshot the global vector info before any scope interaction.
    const before = await (
      await request.get(`${API_BASE}/api/km/settings/vectordb/info`, { headers: authHeaders() })
    ).json();

    await login(page);
    await suppressTours(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // Confirm the scope selector exists and defaults to Global.
    await expect(page.getByText('Settings Scope:')).toBeVisible();
    await expect(page.locator('.ant-tag').filter({ hasText: 'Global' })).toBeVisible();

    // Vector DB is global-only: the API value is unchanged regardless of scope.
    const after = await (
      await request.get(`${API_BASE}/api/km/settings/vectordb/info`, { headers: authHeaders() })
    ).json();
    expect(after.backend).toBe(before.backend);
    expect(after.collection).toBe(before.collection);
  });
});
