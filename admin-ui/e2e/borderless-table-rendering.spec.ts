import { test, expect } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import {
  login,
  navigateTo,
  TEST_EMAIL,
  TEST_PASSWORD,
  API_BASE,
  pinSharedModel,
  setSharedModel,
} from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PDF = path.resolve(__dirname, '../../tests/fixtures/borderless_table.pdf');

/**
 * Browser-driven (headed) check that a BORDERLESS PDF table (laid out purely by
 * column whitespace — no ruling lines) uploaded through the admin UI is
 * reconstructed deterministically from the text layer into a faithful HTML
 * `<table>` (exact numbers, intra-cell spacing preserved), and that the one-line
 * title above the table does NOT leak into a cell.
 */
test.describe('Borderless table rendering (whitespace-stream PDF)', () => {
  const sfx = Date.now();
  const orgName = `BorderlessOrg-${sfx}`;
  const deptName = `BorderlessDept-${sfx}`;
  const wsName = `BorderlessWS-${sfx}`;
  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;
  let prevModel: string | undefined;

  test.beforeAll(async ({ request }) => {
    token = (
      await (
        await request.post(`${API_BASE}/api/auth/login`, {
          data: { email: TEST_EMAIL, password: TEST_PASSWORD },
        })
      ).json()
    ).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (
      await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()
    ).id;
    deptId = (
      await (
        await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, {
          data: { name: deptName },
          headers,
        })
      ).json()
    ).id;
    wsId = (
      await (
        await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, {
          data: { name: wsName },
          headers,
        })
      ).json()
    ).id;

    // Pin a known-pulled chat model so ingest's AI analyzer is never blocked by
    // a leaked/unpulled model left by an earlier spec. Restored in afterAll.
    prevModel = await pinSharedModel(request, token);
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, {
      headers,
    });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
    if (prevModel) await setSharedModel(request, token, prevModel);
  });

  test('borderless PDF reconstructs as an HTML table in the preview', async ({ page }) => {
    test.setTimeout(180_000);
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(orgName).slice(0, 18));
    await page.getByTitle(orgName).click();
    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(deptName).slice(0, 18));
    await page.getByTitle(deptName).click();
    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(wsName).slice(0, 18));
    await page.getByTitle(wsName).click();

    await page.getByRole('button', { name: 'Upload File' }).click();
    const modal = page.locator('.ant-modal', { hasText: 'Upload Document' });
    await expect(modal).toBeVisible();
    await modal.locator('input[type="file"]').setInputFiles(PDF);
    await modal.getByRole('button', { name: 'Upload' }).click();
    await page.getByRole('button', { name: 'Done' }).click();

    const row = page.locator('tr', { hasText: 'borderless_table' });
    await expect(row.getByText('Ready')).toBeVisible({ timeout: 120_000 });

    // Open the preview (eye icon) and assert a real rendered table.
    await row.locator('button:has(.anticon-eye)').click();
    const preview = page.locator('.ant-modal', { hasText: 'Preview:' });
    await expect(preview.locator('table')).toBeVisible();
    // Exact cell content from the text layer (deterministic, never fabricated).
    await expect(preview.locator('table').getByText('North', { exact: true })).toBeVisible();
    await expect(preview.locator('table').getByText('200', { exact: true })).toBeVisible();
    // Intra-cell space preserved ("Q1 Sales", not "Q1Sales").
    await expect(preview.locator('table').getByText('Q1 Sales')).toBeVisible();
    // The one-line title is NOT a table cell.
    await expect(preview.locator('table').getByText('Quarterly Sales')).toHaveCount(0);
    // No raw/escaped tags leaked as text.
    await expect(preview.getByText('<table>')).toHaveCount(0);

    await preview.screenshot({ path: 'e2e/screenshots/borderless-table-preview.png' });
  });
});
