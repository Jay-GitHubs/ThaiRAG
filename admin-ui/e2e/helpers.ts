import { type Page, type APIRequestContext, expect } from '@playwright/test';

export const TEST_EMAIL = 'playwright@test.com';
export const TEST_PASSWORD = 'Test1234!';
export const API_BASE = 'http://localhost:8080';

/**
 * A known-good chat/generation model on the configured provider. The stack runs
 * against an OpenAI-compatible gateway, so this defaults to a gateway model;
 * override with E2E_CHAT_MODEL for a different deployment. Chat-dependent specs
 * pin to this so they don't inherit a leaked model from an earlier spec, and
 * model-mutating specs restore to it so they never leave a bad model behind.
 */
export const GOOD_CHAT_MODEL = process.env.E2E_CHAT_MODEL ?? 'qwen3.6-27b-fast';

/** Read the current global shared chat-pipeline LLM model (or undefined). */
export async function getSharedModel(
  request: APIRequestContext,
  token: string,
): Promise<string | undefined> {
  const headers = { Authorization: `Bearer ${token}` };
  const cp = await (
    await request.get(`${API_BASE}/api/km/settings/chat-pipeline`, { headers })
  ).json();
  return cp.llm?.model as string | undefined;
}

/**
 * Set the global shared chat-pipeline LLM model. Sends ONLY the model: the API
 * merges it into the existing provider config, so the configured kind, base_url
 * and api_key are left intact (no provider-kind change → no credential reset).
 * This keeps the specs provider-agnostic — they just pick a model on whatever
 * provider the stack is configured with — and needs no gateway secret in CI.
 */
export async function setSharedModel(
  request: APIRequestContext,
  token: string,
  model: string,
): Promise<void> {
  const headers = { Authorization: `Bearer ${token}` };
  await request.put(`${API_BASE}/api/km/settings/chat-pipeline`, {
    data: { llm: { model } },
    headers,
  });
}

/**
 * Pin the shared model to a known-pulled one for the duration of a spec and
 * return the previous model so `afterAll` can restore it. Guards chat-dependent
 * specs against inheriting a leaked/unpulled model from an earlier spec.
 */
export async function pinSharedModel(
  request: APIRequestContext,
  token: string,
  model: string = GOOD_CHAT_MODEL,
): Promise<string | undefined> {
  const prev = await getSharedModel(request, token);
  await setSharedModel(request, token, model);
  return prev;
}

/**
 * Server-side settings snapshot/restore bracket for specs that mutate GLOBAL
 * settings (presets, document-processing toggles, pipeline-card saves…).
 * The snapshot captures every raw settings row server-side — including api
 * keys, which are never readable via GET — so restore is exact where any
 * spec-side capture/PUT restore would be lossy. Take the snapshot in
 * beforeAll, restore in afterAll (runs on failure too).
 */
export async function snapshotSettings(
  request: APIRequestContext,
  token: string,
  name: string,
): Promise<string> {
  const headers = { Authorization: `Bearer ${token}` };
  const resp = await request.post(`${API_BASE}/api/km/settings/snapshots`, {
    data: { name },
    headers,
  });
  expect(resp.ok()).toBeTruthy();
  return (await resp.json()).id as string;
}

/**
 * Restore (and consume) a settings snapshot. The restore endpoint clears all
 * settings rows and rewrites them from the snapshot — which also removes the
 * snapshot row itself, so only the index entry needs best-effort cleanup.
 * If the embedding fingerprint changed mid-spec the endpoint answers with a
 * warning instead of restoring; retry preserving the current embedding so a
 * restore can never wipe the vector store.
 */
export async function restoreSettingsSnapshot(
  request: APIRequestContext,
  token: string,
  id: string,
): Promise<void> {
  const headers = { Authorization: `Bearer ${token}` };
  const resp = await request.post(`${API_BASE}/api/km/settings/snapshots/${id}/restore`, {
    headers,
  });
  expect(resp.ok()).toBeTruthy();
  const body = await resp.json();
  if (body.status === 'warning') {
    const retry = await request.post(
      `${API_BASE}/api/km/settings/snapshots/${id}/restore?skip_embedding=true`,
      { headers },
    );
    expect(retry.ok()).toBeTruthy();
  }
  await request
    .delete(`${API_BASE}/api/km/settings/snapshots/${id}`, { headers })
    .catch(() => {});
}

/** Suppress guided tours and quick start from auto-starting during tests. */
export async function suppressTours(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('thairag-tour-state', '{}');
    localStorage.setItem('thairag-quickstart-dismissed', 'true');
  });
}

/** Login via the UI and wait for dashboard to load. */
export async function login(page: Page) {
  await suppressTours(page);
  await page.goto('/login');
  await page.getByPlaceholder('Email').fill(TEST_EMAIL);
  await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
  await page.getByRole('button', { name: 'Sign In' }).click();
  await page.waitForURL('/', { timeout: 10_000 });
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
}

/**
 * Navigate to a page via the sidebar menu (client-side navigation).
 * This avoids full page reload which would lose the in-memory auth token.
 * Handles grouped sub-menus by expanding parent groups if needed.
 */
export async function navigateTo(page: Page, menuLabel: string) {
  const menu = page.getByRole('menu');
  const target = menu.getByText(menuLabel, { exact: true });

  // If the target is already visible (top-level or group already open), click it
  if (await target.isVisible().catch(() => false)) {
    await target.click();
    return;
  }

  // Otherwise, expand each collapsed sub-menu group until we find it
  const subMenus = menu.locator('.ant-menu-submenu-title');
  const count = await subMenus.count();
  for (let i = 0; i < count; i++) {
    const submenu = subMenus.nth(i);
    // Check if this group is already open
    const parent = submenu.locator('..');
    const isOpen = await parent.evaluate((el) =>
      el.closest('.ant-menu-submenu')?.classList.contains('ant-menu-submenu-open'),
    );
    if (!isOpen) {
      await submenu.click();
      // Check if our target is now visible
      if (await target.isVisible().catch(() => false)) {
        await target.click();
        return;
      }
      // Collapse it back if not the right group
      await submenu.click();
    } else {
      // Already open — check if target is inside
      if (await target.isVisible().catch(() => false)) {
        await target.click();
        return;
      }
    }
  }

  // Fallback: try clicking directly (may throw if not visible)
  await target.click();
}
