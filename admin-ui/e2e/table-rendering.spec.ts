import { test, expect } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DOCX = path.resolve(__dirname, '../../tests/fixtures/complex_table.docx');

/**
 * Browser-driven (headed) check that a complex merged-cell DOCX uploaded through
 * the admin UI reconstructs into a faithful HTML table — header cell spanning 2
 * columns, a category cell spanning 2 rows — and renders as a real `<table>` in
 * the document preview (the sanitized HTML render path), not as escaped tags.
 */
test.describe('Complex table rendering (DOCX merged cells)', () => {
  const sfx = Date.now();
  const orgName = `TblRenderOrg-${sfx}`;
  const deptName = `TblRenderDept-${sfx}`;
  const wsName = `TblRenderWS-${sfx}`;
  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;

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
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, {
      headers,
    });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('DOCX merged table renders as HTML in the preview', async ({ page }) => {
    test.setTimeout(180_000);
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    await page.getByTitle(orgName).click();
    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    await page.getByTitle(deptName).click();
    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    await page.getByTitle(wsName).click();

    // Upload via the UI.
    await page.getByRole('button', { name: 'Upload File' }).click();
    const modal = page.locator('.ant-modal', { hasText: 'Upload Document' });
    await expect(modal).toBeVisible();
    await modal.locator('input[type="file"]').setInputFiles(DOCX);
    await modal.getByRole('button', { name: 'Upload' }).click();
    // Live tracker → dismiss.
    await page.getByRole('button', { name: 'Done' }).click();

    // Wait until the row reports Ready.
    const row = page.locator('tr', { hasText: 'complex_table' });
    await expect(row.getByText('Ready')).toBeVisible({ timeout: 120_000 });

    // Open the preview (eye icon) and assert a real rendered table.
    await row.locator('button:has(.anticon-eye)').click();
    const preview = page.locator('.ant-modal', { hasText: 'Preview:' });
    await expect(preview.locator('table')).toBeVisible();
    // Merged structure rendered as actual span attributes (not escaped text).
    await expect(preview.locator('td[colspan="2"]')).toBeVisible();
    await expect(preview.locator('td[rowspan="2"]')).toBeVisible();
    // Merged-cell content + Thai numerals present in the rendered table.
    await expect(preview.locator('table').getByText('กลุ่ม A')).toBeVisible();
    await expect(preview.locator('table').getByText('๕,๖๗๘')).toBeVisible();
    // No raw/escaped tags leaked as text.
    await expect(preview.getByText('<table>')).toHaveCount(0);

    await preview.screenshot({ path: 'e2e/screenshots/complex-table-preview.png' });
  });
});
