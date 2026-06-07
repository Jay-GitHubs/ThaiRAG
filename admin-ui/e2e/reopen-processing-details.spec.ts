import { test, expect } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DOCX = path.resolve(__dirname, '../../tests/fixtures/complex_table.docx');

/**
 * After upload, the live processing dialog can be re-opened from the documents
 * table — by clicking the Status tag or the "Processing details" action icon —
 * so a user who closed it can always get back to the per-stage detail (it's
 * persisted on the document).
 */
test.describe('Re-open processing details', () => {
  const sfx = Date.now();
  const orgName = `ReopenOrg-${sfx}`;
  const deptName = `ReopenDept-${sfx}`;
  const wsName = `ReopenWS-${sfx}`;
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
        await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })
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
    // Upload via API so the test focuses on the re-open path.
    const fs = await import('fs');
    const buffer = fs.readFileSync(DOCX);
    await request.post(`${API_BASE}/api/km/workspaces/${wsId}/documents/upload`, {
      headers,
      multipart: { file: { name: 'complex_table.docx', mimeType: 'application/octet-stream', buffer } },
    });
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('status tag and action icon both re-open the processing detail', async ({ page }) => {
    test.setTimeout(120_000);
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();
    // The org list (no search) accumulates entries and virtualizes, so the
    // newest org may not be rendered — scroll the open dropdown to the bottom
    // before selecting. Dept/Workspace are scoped to the new org (one option).
    const pickScrolled = async (label: RegExp, title: string) => {
      await page.locator('.ant-select', { hasText: label }).click();
      await page
        .locator('.rc-virtual-list-holder')
        .last()
        .evaluate((el) => {
          el.scrollTop = el.scrollHeight;
        })
        .catch(() => {});
      await page.getByTitle(title).click();
    };
    await pickScrolled(/Select Organization/i, orgName);
    await pickScrolled(/Select Department/i, deptName);
    await pickScrolled(/Select Workspace/i, wsName);

    const row = page.locator('tr', { hasText: 'complex_table' });
    await expect(row.getByText('Ready')).toBeVisible({ timeout: 120_000 });

    // 1) Click the Status tag → detail modal opens with the step tracker.
    await row.getByText('Ready').click();
    const modal = page.locator('.ant-modal', { hasText: 'Processing details' });
    await expect(modal).toBeVisible();
    await expect(modal.locator('.ant-steps')).toBeVisible();
    await expect(modal.getByText('Uploaded')).toBeVisible();
    await modal.locator('.ant-modal-close').first().click();
    await expect(modal).not.toBeVisible();

    // 2) Click the "Processing details" action icon → re-opens the same detail.
    await row.locator('button:has(.anticon-profile)').click();
    await expect(modal).toBeVisible();
    await expect(modal.locator('.ant-steps')).toBeVisible();
  });
});
