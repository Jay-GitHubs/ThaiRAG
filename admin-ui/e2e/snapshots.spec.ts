import { test, expect } from '@playwright/test';
import { login, navigateTo, suppressTours, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

/**
 * Config Snapshot tests.
 *
 * Snapshots capture ALL GLOBAL settings + an embedding fingerprint. They do NOT
 * capture scoped (org/dept/workspace) overrides. Restore rewrites global config
 * and reloads providers; vectors are only cleared if the embedding fingerprint
 * differs and force=true. These tests never touch embedding, so restore is a
 * safe (non-destructive) global-config swap.
 *
 * Core assertion: two snapshots taken at two DIFFERENT configurations restore
 * back to their OWN distinct configuration.
 */
test.describe('Config Snapshots', () => {
  const suffix = Date.now();
  const snapAName = `PW-SnapA-${suffix}`;
  const snapBName = `PW-SnapB-${suffix}`;
  const baselineName = `PW-Baseline-${suffix}`;

  let token: string;
  let baselineTokens: number;
  let snapAId = '';
  let snapBId = '';
  let baselineId = '';

  const VAL_A = 1111;
  const VAL_B = 2222;

  const authHeaders = () => ({ Authorization: `Bearer ${token}` });

  // Read the current global chat-pipeline config.
  async function getPipeline(request: any) {
    const res = await request.get(`${API_BASE}/api/km/settings/chat-pipeline`, {
      headers: authHeaders(),
    });
    expect(res.status()).toBe(200);
    return res.json();
  }

  // Set max_context_tokens on the GLOBAL chat-pipeline config (full-body PUT,
  // preserving every other field so we only vary the one value under test).
  async function setMaxTokens(request: any, value: number) {
    const current = await getPipeline(request);
    current.max_context_tokens = value;
    const res = await request.put(`${API_BASE}/api/km/settings/chat-pipeline`, {
      data: current,
      headers: authHeaders(),
    });
    expect(res.ok()).toBeTruthy();
  }

  async function createSnapshot(request: any, name: string): Promise<string> {
    const res = await request.post(`${API_BASE}/api/km/settings/snapshots`, {
      data: { name, description: 'created by playwright snapshot spec' },
      headers: authHeaders(),
    });
    expect(res.ok()).toBeTruthy();
    const snap = await res.json();
    return snap.id;
  }

  async function restoreSnapshot(request: any, id: string) {
    const res = await request.post(
      `${API_BASE}/api/km/settings/snapshots/${id}/restore`,
      { headers: authHeaders() },
    );
    expect(res.status()).toBe(200);
    return res.json();
  }

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok()).toBeTruthy();
    token = (await loginRes.json()).token;

    // Remember the real starting value so we can restore it at the very end.
    const start = await getPipeline(request);
    baselineTokens = start.max_context_tokens;
    baselineId = await createSnapshot(request, baselineName);
  });

  test.afterAll(async ({ request }) => {
    // Restore the original global config, then delete the 3 snapshots we made.
    if (baselineId) {
      await restoreSnapshot(request, baselineId).catch(() => {});
    }
    for (const id of [snapAId, snapBId, baselineId]) {
      if (id) {
        await request
          .delete(`${API_BASE}/api/km/settings/snapshots/${id}`, { headers: authHeaders() })
          .catch(() => {});
      }
    }
  });

  test('snapshot captures the current global config', async ({ request }) => {
    await setMaxTokens(request, VAL_A);
    snapAId = await createSnapshot(request, snapAName);
    expect(snapAId).toBeTruthy();

    // The new snapshot shows up in the list with a settings_count > 0.
    const listRes = await request.get(`${API_BASE}/api/km/settings/snapshots`, {
      headers: authHeaders(),
    });
    const list = await listRes.json();
    const found = list.find((s: any) => s.id === snapAId);
    expect(found).toBeTruthy();
    expect(found.name).toBe(snapAName);
    expect(found.settings_count).toBeGreaterThan(0);
  });

  test('a second snapshot captures a DIFFERENT config', async ({ request }) => {
    await setMaxTokens(request, VAL_B);
    snapBId = await createSnapshot(request, snapBName);
    expect(snapBId).toBeTruthy();

    // Live config is now B.
    const live = await getPipeline(request);
    expect(live.max_context_tokens).toBe(VAL_B);
  });

  test('restoring snapshot A brings back config A', async ({ request }) => {
    const result = await restoreSnapshot(request, snapAId);
    // Same embedding fingerprint => no warning, full restore.
    expect(result.status).not.toBe('warning');

    const live = await getPipeline(request);
    expect(live.max_context_tokens).toBe(VAL_A);
  });

  test('restoring snapshot B brings back config B (distinct from A)', async ({ request }) => {
    const result = await restoreSnapshot(request, snapBId);
    expect(result.status).not.toBe('warning');

    const live = await getPipeline(request);
    expect(live.max_context_tokens).toBe(VAL_B);
    // Proves the two snapshots hold independent configurations.
    expect(VAL_B).not.toBe(VAL_A);
  });

  test('Config Snapshots card renders snapshots in the UI', async ({ page }) => {
    // Self-sufficient: create a dedicated snapshot for this test so it does not
    // depend on sibling tests having run (and survives -g filtering).
    const uiName = `PW-UI-${Date.now()}`;
    const uiId = await createSnapshot(page.request, uiName);

    try {
      await login(page);
      await suppressTours(page);
      await navigateTo(page, 'Settings');
      await page.waitForTimeout(500);

      // The snapshots card is a collapse panel labelled "Config Snapshots".
      const cardHeader = page.getByText('Config Snapshots', { exact: true });
      await expect(cardHeader).toBeVisible();

      // "Save Current Config" lives in the header. Scope to a real <button> so
      // we don't also match the collapse-header div (role=button) whose
      // accessible name nests the button label.
      const saveBtn = page.locator('button:has-text("Save Current Config")');
      await expect(saveBtn).toBeVisible();

      // Expand the panel to reveal the snapshots table.
      await cardHeader.click();
      await page.waitForTimeout(500);

      // Our snapshot is listed in the table.
      await expect(page.getByText(uiName)).toBeVisible({ timeout: 5000 });

      // Rows expose a Restore action.
      await expect(page.locator('button:has-text("Restore")').first()).toBeVisible();
    } finally {
      await page.request
        .delete(`${API_BASE}/api/km/settings/snapshots/${uiId}`, { headers: authHeaders() })
        .catch(() => {});
    }
  });

  test('Save Current Config modal opens and can be cancelled', async ({ page }) => {
    await login(page);
    await suppressTours(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // The "Save Current Config" button is in the collapse header and must open
    // the dialog even while the panel is collapsed (Modal is mounted at the card
    // root, not inside the lazily-rendered panel body).
    await page.locator('button:has-text("Save Current Config")').click();

    // The create dialog appears.
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible({ timeout: 5000 });
    await expect(dialog.getByText('Save Configuration Snapshot')).toBeVisible();
    await expect(page.getByPlaceholder(/Before switching/i)).toBeVisible();

    // Cancel — do not create another global snapshot from the UI.
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await page.waitForTimeout(300);
    await expect(page.getByText('Save Configuration Snapshot')).not.toBeVisible();
  });
});
