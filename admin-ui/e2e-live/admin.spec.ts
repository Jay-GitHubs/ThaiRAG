import { test, expect, ADMIN_ORIGIN, API_ORIGIN, tokenFor } from './fixtures';
import { navigateTo } from '../e2e/helpers';

const STAMP = Date.now().toString(36);
const ORG = `livetest-org-${STAMP}`;
const DEPT = `livetest-dept-${STAMP}`;
const WS = `livetest-ws-${STAMP}`;

test.describe('admin UI (live)', () => {
  test('dashboard renders for the logged-in admin', async ({ page }) => {
    await page.goto(`${ADMIN_ORIGIN}/`);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({
      timeout: 20_000,
    });
  });

  test('core pages render without an error state', async ({ page }) => {
    // Direct routes (session token is injected on every navigation).
    for (const route of ['/documents', '/analytics', '/inference-logs', '/settings', '/usage']) {
      await page.goto(`${ADMIN_ORIGIN}${route}`);
      // Any crash/red error screen would blank the main area; assert the app
      // shell is present and no error boundary fired.
      await expect(page.getByRole('menu').first()).toBeVisible({ timeout: 20_000 });
      await expect(page.locator('.ant-result-error')).toHaveCount(0);
      // A redirect back to /login would mean the injected session broke.
      expect(page.url()).not.toContain('/login');
    }
  });

  test('provider settings never echo a raw API key to the browser', async ({ page }) => {
    await page.goto(`${ADMIN_ORIGIN}/`);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({
      timeout: 20_000,
    });
    await navigateTo(page, 'Settings');
    // The GET /providers payload must expose has_api_key booleans, not keys.
    const res = await page.request.get(`${API_ORIGIN}/api/km/settings/providers`, {
      headers: { Authorization: `Bearer ${tokenFor(ADMIN_ORIGIN)}` },
    });
    if (res.ok()) {
      const text = await res.text();
      expect(text).not.toMatch(/"api_key"\s*:\s*"[^"]+"/);
    }
    // And no password/key input on the page is pre-filled with a value.
    for (const input of await page.locator('input[type="password"]').all()) {
      expect(await input.inputValue()).toBe('');
    }
  });

  test('KM hierarchy: create org → dept → workspace, then delete all', async ({
    page,
    request,
  }) => {
    test.setTimeout(180_000);
    try {
      await page.goto(`${ADMIN_ORIGIN}/`);
      await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({
        timeout: 20_000,
      });
      await navigateTo(page, 'KM Hierarchy');
      await expect(page.getByRole('heading', { name: 'KM Hierarchy' })).toBeVisible();

      await page.getByRole('button', { name: 'New Org' }).click();
      const orgModal = page.locator('.ant-modal', { hasText: 'Create Organization' });
      await orgModal.getByPlaceholder('Organization name').fill(ORG);
      await orgModal.getByRole('button', { name: 'OK' }).click();
      await expect(orgModal).not.toBeVisible({ timeout: 15_000 });
      await page.getByText(ORG, { exact: true }).click();

      await page.getByRole('button', { name: 'New Department' }).click();
      const deptModal = page.locator('.ant-modal', { hasText: 'Create Department' });
      await deptModal.getByPlaceholder('Department name').fill(DEPT);
      await deptModal.getByRole('button', { name: 'OK' }).click();
      await expect(deptModal).not.toBeVisible({ timeout: 15_000 });
      await expect(page.getByRole('cell', { name: DEPT })).toBeVisible({ timeout: 15_000 });

      // After the mutation the tree refreshes collapsed — expand the org node
      // and select the dept in the tree to open its panel (mirrors km.spec).
      const orgNode = page.locator('.ant-tree-treenode', { hasText: ORG });
      await orgNode.locator('.ant-tree-switcher').click();
      await expect(page.locator('.ant-tree').getByText(DEPT)).toBeVisible({ timeout: 15_000 });
      await page.locator('.ant-tree').getByText(DEPT).click();
      await expect(page.getByText(`Department: ${DEPT}`)).toBeVisible({ timeout: 15_000 });

      await page.getByRole('button', { name: 'New Workspace' }).click();
      const wsModal = page.locator('.ant-modal', { hasText: 'Create Workspace' });
      await wsModal.getByPlaceholder('Workspace name').fill(WS);
      await wsModal.getByRole('button', { name: 'OK' }).click();
      await expect(wsModal).not.toBeVisible({ timeout: 15_000 });
      await expect(page.getByRole('cell', { name: WS })).toBeVisible({ timeout: 15_000 });

      // Delete workspace → dept → org from the UI
      const wsRow = page.locator('tr', { hasText: WS });
      await wsRow.locator('button').click();
      await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
      await expect(page.getByRole('cell', { name: WS })).not.toBeVisible({ timeout: 15_000 });

      await page.getByRole('button', { name: 'Delete Dept' }).click();
      await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();

      // Deleting the dept clears the panel — reselect the org in the tree.
      await page.locator('.ant-tree').getByText(ORG).click();
      await expect(page.getByText(`Organization: ${ORG}`)).toBeVisible({ timeout: 15_000 });

      await page.getByRole('button', { name: 'Delete Org' }).click();
      await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
      await expect(page.locator('.ant-tree').getByText(ORG)).not.toBeVisible({ timeout: 15_000 });
    } finally {
      // API fallback cleanup so a mid-test failure can't leak livetest-* orgs
      // into production (delete via API cascades correctly).
      const token = tokenFor(ADMIN_ORIGIN);
      if (token) {
        const headers = { Authorization: `Bearer ${token}` };
        const orgs = await request.get(`${API_ORIGIN}/api/km/orgs`, { headers });
        if (orgs.ok()) {
          const body = await orgs.json();
          const list = Array.isArray(body) ? body : (body.orgs ?? body.data ?? []);
          for (const o of list) {
            if (o.name?.startsWith('livetest-org-')) {
              await request.delete(`${API_ORIGIN}/api/km/orgs/${o.id}`, { headers });
            }
          }
        }
      }
    }
  });
});
