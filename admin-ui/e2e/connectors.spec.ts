import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

const orgName = `ConnOrg-${Date.now()}`;
const deptName = `ConnDept-${Date.now()}`;
const wsName = `ConnWs-${Date.now()}`;

let token: string;
let orgId: string;
let deptId: string;
let wsId: string;

test.describe('MCP Connectors', () => {
  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok()).toBeTruthy();
    const loginData = await loginRes.json();
    token = loginData.token;
    const headers = { Authorization: `Bearer ${token}` };

    const orgRes = await request.post(`${API_BASE}/api/km/orgs`, {
      data: { name: orgName },
      headers,
    });
    expect(orgRes.ok()).toBeTruthy();
    orgId = (await orgRes.json()).id;

    const deptRes = await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, {
      data: { name: deptName },
      headers,
    });
    expect(deptRes.ok()).toBeTruthy();
    deptId = (await deptRes.json()).id;

    const wsRes = await request.post(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
      { data: { name: wsName }, headers },
    );
    expect(wsRes.ok()).toBeTruthy();
    wsId = (await wsRes.json()).id;
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const connRes = await request.get(`${API_BASE}/api/km/connectors`, { headers });
    if (connRes.ok()) {
      const { data } = await connRes.json();
      for (const c of data) {
        if (c.workspace_id === wsId) {
          await request.delete(`${API_BASE}/api/km/connectors/${c.id}`, { headers });
        }
      }
    }
    await request.delete(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`,
      { headers },
    );
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('navigate to connectors page', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Connectors');
    await expect(page.getByText('MCP Connectors')).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole('button', { name: 'Create Connector' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'From Template' })).toBeVisible();
  });

  test('view templates and env key fields', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Connectors');
    await page.waitForTimeout(500);

    await page.getByRole('button', { name: 'From Template' }).click();

    // Ant Design renders Modal as <dialog> role
    const modal = page.getByRole('dialog', { name: 'Choose a Template' });
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Verify template cards (use strong text to avoid matching description text)
    await expect(modal.locator('strong', { hasText: 'Filesystem' })).toBeVisible();
    await expect(modal.locator('strong', { hasText: 'GitHub' })).toBeVisible();
    await expect(modal.locator('strong', { hasText: 'Confluence' })).toBeVisible();

    // Click GitHub to see env key fields
    await modal.locator('.ant-card', { hasText: 'GitHub' }).click();

    const configModal = page.getByRole('dialog', { name: /Create from "GitHub" Template/ });
    await expect(configModal).toBeVisible({ timeout: 5000 });
    await expect(configModal.getByText('GITHUB_TOKEN')).toBeVisible();

    await configModal.getByRole('button', { name: 'Cancel' }).click();
    await page.waitForTimeout(300);
  });

  test('full connector lifecycle: create, edit, pause, resume, history, delete', async ({
    page,
  }) => {
    await login(page);
    await navigateTo(page, 'Connectors');
    await page.waitForTimeout(500);

    // ── CREATE ───────────────────────────────────────────────────
    await page.getByRole('button', { name: 'Create Connector' }).click();
    const createModal = page.getByRole('dialog', { name: 'Create Connector' });
    await expect(createModal).toBeVisible({ timeout: 5000 });

    await createModal.getByPlaceholder('e.g. My Confluence').fill('Test Connector');
    await createModal.getByPlaceholder('Optional description').fill('E2E test');

    // Cascading workspace selectors
    await createModal.locator('.ant-select').filter({ hasText: /Select organization/i }).click();
    await page.locator('.ant-select-dropdown').getByTitle(orgName).click();
    await page.waitForTimeout(500);

    await createModal.locator('.ant-select').filter({ hasText: /Select department/i }).click();
    await page.locator('.ant-select-dropdown').getByTitle(deptName).click();
    await page.waitForTimeout(500);

    await createModal.locator('.ant-select').filter({ hasText: /Select workspace/i }).click();
    await page.locator('.ant-select-dropdown').getByTitle(wsName).click();
    await page.waitForTimeout(300);

    await createModal.getByPlaceholder('e.g. npx').fill('echo');
    await createModal.getByPlaceholder(/e.g. -y/).fill('hello');

    await createModal.getByRole('button', { name: 'OK' }).click();
    await expect(createModal).not.toBeVisible({ timeout: 5000 });

    // Verify in table
    await expect(page.getByText('Test Connector')).toBeVisible({ timeout: 5000 });
    const row = page.locator('tr', { hasText: 'Test Connector' });
    await expect(row.getByText('STDIO')).toBeVisible();
    await expect(row.getByText('active')).toBeVisible();

    // ── EDIT ─────────────────────────────────────────────────────
    await row.getByRole('button', { name: 'edit' }).click();

    const editModal = page.getByRole('dialog', { name: 'Edit Connector' });
    await expect(editModal).toBeVisible({ timeout: 5000 });

    const nameInput = editModal.getByPlaceholder('e.g. My Confluence');
    await nameInput.clear();
    await nameInput.fill('Test Connector Edited');

    await editModal.getByRole('button', { name: 'OK' }).click();
    await expect(editModal).not.toBeVisible({ timeout: 5000 });

    await expect(page.getByText('Test Connector Edited')).toBeVisible({ timeout: 5000 });
    const updatedRow = page.locator('tr', { hasText: 'Test Connector Edited' });

    // ── PAUSE ────────────────────────────────────────────────────
    await updatedRow.getByRole('button', { name: 'pause-circle' }).click();
    await expect(updatedRow.getByText('paused')).toBeVisible({ timeout: 5000 });

    // ── RESUME ───────────────────────────────────────────────────
    await updatedRow.getByRole('button', { name: 'play-circle' }).click();
    await expect(updatedRow.getByText('active')).toBeVisible({ timeout: 5000 });

    // ── SYNC HISTORY ─────────────────────────────────────────────
    await updatedRow.getByRole('button', { name: 'history' }).click();

    const historyModal = page.getByRole('dialog', {
      name: /Sync History/,
    });
    await expect(historyModal).toBeVisible({ timeout: 5000 });
    // Empty table shows "No data" (Ant Design renders both an image alt and text, use last)
    await expect(historyModal.getByText('No data').last()).toBeVisible({ timeout: 5000 });

    // Close history modal
    await historyModal.getByRole('button', { name: 'Close' }).click();
    await expect(historyModal).not.toBeVisible({ timeout: 5000 });

    // ── DELETE ────────────────────────────────────────────────────
    await updatedRow.getByRole('button', { name: 'delete' }).click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();

    await expect(page.getByText('Test Connector Edited')).not.toBeVisible({ timeout: 5000 });
  });

  test('connector API CRUD', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    // Create
    const createRes = await request.post(`${API_BASE}/api/km/connectors`, {
      data: {
        name: 'API Test Connector',
        transport: 'stdio',
        command: 'echo',
        args: ['test'],
        workspace_id: wsId,
        sync_mode: 'on_demand',
      },
      headers,
    });
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    expect(created.name).toBe('API Test Connector');
    expect(created.status).toBe('active');

    // List
    const listRes = await request.get(`${API_BASE}/api/km/connectors`, { headers });
    expect(listRes.ok()).toBeTruthy();
    expect((await listRes.json()).data.length).toBeGreaterThanOrEqual(1);

    // Get
    const getRes = await request.get(`${API_BASE}/api/km/connectors/${created.id}`, { headers });
    expect(getRes.ok()).toBeTruthy();

    // Update
    const updateRes = await request.put(`${API_BASE}/api/km/connectors/${created.id}`, {
      data: { name: 'Updated Connector' },
      headers,
    });
    expect(updateRes.ok()).toBeTruthy();
    expect((await updateRes.json()).name).toBe('Updated Connector');

    // Pause + Resume
    expect(
      (await request.post(`${API_BASE}/api/km/connectors/${created.id}/pause`, { headers })).ok(),
    ).toBeTruthy();
    expect(
      (await request.post(`${API_BASE}/api/km/connectors/${created.id}/resume`, { headers })).ok(),
    ).toBeTruthy();

    // Sync runs (empty)
    const runsRes = await request.get(
      `${API_BASE}/api/km/connectors/${created.id}/sync-runs`,
      { headers },
    );
    expect(runsRes.ok()).toBeTruthy();
    expect((await runsRes.json()).data).toHaveLength(0);

    // Templates
    const templatesRes = await request.get(`${API_BASE}/api/km/connectors/templates`, {
      headers,
    });
    expect(templatesRes.ok()).toBeTruthy();
    expect((await templatesRes.json()).length).toBe(9);

    // Delete
    expect(
      (await request.delete(`${API_BASE}/api/km/connectors/${created.id}`, { headers })).status(),
    ).toBe(204);
  });

  test('connector requires authentication', async ({ request }) => {
    const noAuthRes = await request.get(`${API_BASE}/api/km/connectors`);
    expect(noAuthRes.status()).toBe(401);
  });
});
