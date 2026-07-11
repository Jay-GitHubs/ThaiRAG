import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

// "Reprocess with options": the document table's reprocess action opens a modal
// that dry-runs the complexity router for the ALREADY-stored doc, shows the
// handling decision, and lets an admin pick a handling mode before re-running.
// Without this, reprocess silently re-ran in Auto mode and ignored every lever.

test.describe('Reprocess with options', () => {
  const suffix = Date.now();
  const orgName = `RepOrg-${suffix}`;
  const deptName = `RepDept-${suffix}`;
  const wsName = `RepWS-${suffix}`;

  let token: string;
  let orgId: string;
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    token = (await loginRes.json()).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()).id;
    const deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: wsName }, headers })).json()).id;

    // Ingest a small clean text doc and wait until it's READY.
    await request.post(`${API_BASE}/api/km/workspaces/${wsId}/documents/upload`, {
      headers,
      multipart: {
        file: { name: 'reproc.txt', mimeType: 'text/plain', buffer: Buffer.from('สวัสดี ThaiRAG — เอกสารทดสอบ reprocess') },
        title: 'reproc-doc',
      },
    });
    for (let i = 0; i < 30; i++) {
      const docs = (await (await request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, { headers })).json()).data;
      if (docs?.[0]?.status === 'ready') break;
      await new Promise((r) => setTimeout(r, 1000));
    }
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('reprocess modal previews + sends the chosen handling mode', async ({ page }) => {
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

    await expect(page.getByText('reproc-doc')).toBeVisible({ timeout: 10000 });

    // Open the reprocess modal (the per-row ReloadOutlined action — scoped to the
    // table so it can't collide with the toolbar's "Re-embed All" reload button).
    await page.locator('.ant-table').getByRole('button', { name: 'reload' }).first().click();
    const modal = page.locator('.ant-modal-content').filter({ hasText: 'Reprocess —' });
    await expect(modal).toBeVisible();

    // The dry-run preview auto-runs and renders the handling decision.
    await expect(modal.getByText(/Handling preview/i)).toBeVisible({ timeout: 10000 });
    await expect(modal.getByText('Handling', { exact: true })).toBeVisible(); // HandlingControls label

    // Pick a non-default handling mode (antd Radio.Button = a label wrapper).
    await modal.locator('label.ant-radio-button-wrapper', { hasText: 'Text only' }).click();

    // Capture the reprocess request and confirm it carries the chosen mode.
    const reqs: Record<string, unknown>[] = [];
    page.on('request', (req) => {
      if (req.url().includes('/reprocess') && req.method() === 'POST') {
        const d = req.postData();
        if (d) reqs.push(JSON.parse(d));
      }
    });

    await modal.getByRole('button', { name: 'Reprocess', exact: true }).click();
    await expect(page.getByText('Reprocessing started')).toBeVisible({ timeout: 10000 });

    expect(reqs.length).toBeGreaterThan(0);
    expect(reqs[reqs.length - 1].handling_mode).toBe('text_only');
  });
});
