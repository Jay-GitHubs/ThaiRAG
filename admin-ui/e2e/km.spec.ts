import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

test.describe('KM Hierarchy', () => {
  const suffix = Date.now();
  const orgName = `TestOrg-${suffix}`;
  const deptName = `TestDept-${suffix}`;
  const wsName = `TestWS-${suffix}`;

  test('full CRUD: create org → dept → workspace → delete all', async ({ page, request }) => {
    // Login via API to get token for cleanup fallback
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token } = await loginRes.json();
    const headers = { Authorization: `Bearer ${token}` };

    await login(page);
    await navigateTo(page, 'KM Hierarchy');
    await expect(page.getByRole('heading', { name: 'KM Hierarchy' })).toBeVisible();

    // --- Create Organization ---
    await page.getByRole('button', { name: 'New Org' }).click();
    const createOrgModal = page.locator('.ant-modal', { hasText: 'Create Organization' });
    await expect(createOrgModal).toBeVisible();
    await createOrgModal.getByPlaceholder('Organization name').fill(orgName);
    await createOrgModal.getByRole('button', { name: 'OK' }).click();
    await expect(createOrgModal).not.toBeVisible({ timeout: 5000 });

    // Org should appear in the tree
    await expect(page.locator('.ant-tree').getByText(orgName)).toBeVisible({ timeout: 5000 });

    // --- Select Org → OrgPanel ---
    await page.locator('.ant-tree').getByText(orgName).click();
    await expect(page.getByText(`Organization: ${orgName}`)).toBeVisible({ timeout: 5000 });

    // --- Create Department (from OrgPanel) ---
    await page.getByRole('button', { name: 'New Department' }).click();
    const createDeptModal = page.locator('.ant-modal', { hasText: 'Create Department' });
    await expect(createDeptModal).toBeVisible();
    await createDeptModal.getByPlaceholder('Department name').fill(deptName);
    await createDeptModal.getByRole('button', { name: 'OK' }).click();
    await expect(createDeptModal).not.toBeVisible({ timeout: 5000 });

    // Dept appears in the OrgPanel's departments table
    await expect(page.getByRole('cell', { name: deptName })).toBeVisible({ timeout: 5000 });

    // --- Navigate to DeptPanel via tree ---
    // After mutation, tree refreshes (refreshKey++). Need to expand org to see dept.
    // The tree data is reset, so org node is collapsed — click to expand triggers loadData.
    const orgNode = page.locator('.ant-tree-treenode', { hasText: orgName });
    await orgNode.locator('.ant-tree-switcher').click();
    // Wait for dept to appear in tree via lazy load
    await expect(page.locator('.ant-tree').getByText(deptName)).toBeVisible({ timeout: 5000 });
    await page.locator('.ant-tree').getByText(deptName).click();
    await expect(page.getByText(`Department: ${deptName}`)).toBeVisible({ timeout: 5000 });

    // --- Create Workspace (from DeptPanel) ---
    await page.getByRole('button', { name: 'New Workspace' }).click();
    const createWsModal = page.locator('.ant-modal', { hasText: 'Create Workspace' });
    await expect(createWsModal).toBeVisible();
    await createWsModal.getByPlaceholder('Workspace name').fill(wsName);
    await createWsModal.getByRole('button', { name: 'OK' }).click();
    await expect(createWsModal).not.toBeVisible({ timeout: 5000 });

    // Workspace appears in the DeptPanel's workspaces table
    await expect(page.getByRole('cell', { name: wsName })).toBeVisible({ timeout: 5000 });

    // --- Delete workspace from DeptPanel table ---
    const wsRow = page.locator('tr', { hasText: wsName });
    await wsRow.locator('button').click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await expect(page.getByRole('cell', { name: wsName })).not.toBeVisible({ timeout: 5000 });

    // --- Delete department via "Delete Dept" button on DeptPanel ---
    await page.getByRole('button', { name: 'Delete Dept' }).click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();

    // After deleting dept, panel should clear — click org in tree to get OrgPanel
    await page.locator('.ant-tree').getByText(orgName).click();
    await expect(page.getByText(`Organization: ${orgName}`)).toBeVisible({ timeout: 5000 });

    // --- Delete organization via "Delete Org" button ---
    await page.getByRole('button', { name: 'Delete Org' }).click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await expect(page.locator('.ant-tree').getByText(orgName)).not.toBeVisible({ timeout: 5000 });
  });
});
