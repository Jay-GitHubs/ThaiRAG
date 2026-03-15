import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

test.describe('Documents', () => {
  const suffix = Date.now();
  const orgName = `DocOrg-${suffix}`;
  const deptName = `DocDept-${suffix}`;
  const wsName = `DocWS-${suffix}`;
  const docTitle = `TestDoc-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    // Login via API to get token
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const loginData = await loginRes.json();
    token = loginData.token;
    const headers = { Authorization: `Bearer ${token}` };

    // Create org → dept → workspace via API
    const orgRes = await request.post(`${API_BASE}/api/km/orgs`, {
      data: { name: orgName },
      headers,
    });
    orgId = (await orgRes.json()).id;

    const deptRes = await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, {
      data: { name: deptName },
      headers,
    });
    deptId = (await deptRes.json()).id;

    const wsRes = await request.post(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
      { data: { name: wsName }, headers },
    );
    wsId = (await wsRes.json()).id;
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`,
      { headers },
    );
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('ingest text document, verify in table, then delete', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    // Select org → dept → workspace from dropdowns
    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    await page.getByTitle(orgName).click();

    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    await page.getByTitle(deptName).click();

    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    await page.getByTitle(wsName).click();

    // Wait for document table to appear
    await expect(page.getByRole('button', { name: 'Ingest Text' })).toBeVisible({ timeout: 5000 });

    // Click "Ingest Text" button
    await page.getByRole('button', { name: 'Ingest Text' }).click();
    const modal = page.locator('.ant-modal', { hasText: 'Ingest Text Document' });
    await expect(modal).toBeVisible();

    // Fill form
    await modal.getByPlaceholder('Document title').fill(docTitle);
    await modal.getByPlaceholder('Paste document content here...').fill(
      'This is test document content for Playwright E2E testing.',
    );
    await modal.getByRole('button', { name: 'OK' }).click();
    await expect(modal).not.toBeVisible({ timeout: 10_000 });

    // Verify document appears in table
    await expect(page.getByText(docTitle)).toBeVisible({ timeout: 5000 });
    await expect(page.locator('.ant-tag', { hasText: 'text/plain' })).toBeVisible();

    // Delete document
    const docRow = page.locator('tr', { hasText: docTitle });
    await docRow.getByRole('button', { name: 'Delete' }).click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await expect(page.getByText(docTitle)).not.toBeVisible({ timeout: 5000 });
  });
});
