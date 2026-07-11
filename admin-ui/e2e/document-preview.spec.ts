import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

// Pre-ingest "Preview analysis": the upload modal can dry-run the complexity
// router (POST .../documents/preview) and show what the pipeline WOULD do —
// classes, fidelity-tier split, thresholds, recommendation — before ingesting.

test.describe('Document handling preview', () => {
  const suffix = Date.now();
  const orgName = `PrevOrg-${suffix}`;
  const deptName = `PrevDept-${suffix}`;
  const wsName = `PrevWS-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    token = (await loginRes.json()).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()).id;
    deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: wsName }, headers })).json()).id;
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('Preview analysis shows the handling decision before ingest', async ({ page }) => {
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

    // Open the upload modal.
    await page.getByRole('button', { name: 'Upload File' }).click();
    const modal = page.locator('.ant-modal-content').filter({ hasText: 'Upload Document' });
    await expect(modal).toBeVisible();

    // Select a small text file (clean → fully deterministic, fast preview).
    await modal.locator('input[type="file"]').setInputFiles({
      name: 'note.txt',
      mimeType: 'text/plain',
      buffer: Buffer.from('สวัสดี ThaiRAG — เอกสารทดสอบ preview'),
    });

    // Run the dry-run analysis.
    await modal.getByRole('button', { name: 'Preview analysis' }).click();

    // The preview panel appears with the handling decision.
    await expect(page.getByText(/Handling preview/i)).toBeVisible({ timeout: 10000 });
    await expect(page.getByText(/Fully deterministic/i)).toBeVisible();
    // Tier legend + a class tag are shown.
    await expect(page.getByText('Native', { exact: true }).first()).toBeVisible();
    await expect(page.getByText(/Thresholds:/i)).toBeVisible();
    // The Upload button relabels to "Ingest anyway" once a preview is shown.
    await expect(modal.getByRole('button', { name: 'Ingest anyway' })).toBeVisible();
  });
});
