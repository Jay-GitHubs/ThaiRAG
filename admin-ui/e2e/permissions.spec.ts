import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

const GRANT_EMAIL = 'playwright2@test.com';

test.describe('Permissions', () => {
  const suffix = Date.now();
  const orgName = `PermOrg-${suffix}`;

  let token: string;
  let orgId: string;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const loginData = await loginRes.json();
    token = loginData.token;
    const headers = { Authorization: `Bearer ${token}` };

    const orgRes = await request.post(`${API_BASE}/api/km/orgs`, {
      data: { name: orgName },
      headers,
    });
    orgId = (await orgRes.json()).id;
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('grant and revoke permission', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Permissions');
    await expect(page.getByRole('heading', { name: 'Permissions' })).toBeVisible();

    // Select organization
    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    await page.getByTitle(orgName).click();

    // Wait for PermissionMatrix to load
    await expect(page.getByRole('button', { name: 'Grant Permission' })).toBeVisible({
      timeout: 5000,
    });

    // Grant permission to a different user (creator already has "owner")
    await page.getByRole('button', { name: 'Grant Permission' }).click();
    const modal = page.locator('.ant-modal', { hasText: 'Grant Permission' });
    await expect(modal).toBeVisible();

    await modal.getByPlaceholder('user@example.com').fill(GRANT_EMAIL);
    // Role defaults to "viewer"
    await modal.getByRole('button', { name: 'OK' }).click();
    await expect(modal).not.toBeVisible({ timeout: 5000 });

    // Verify permission row appears
    await expect(page.getByRole('cell', { name: GRANT_EMAIL })).toBeVisible({ timeout: 5000 });

    // Revoke permission — click delete on the granted user's row
    const permRow = page.locator('tr', { hasText: GRANT_EMAIL });
    await permRow.locator('button').click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await expect(page.getByRole('cell', { name: GRANT_EMAIL })).not.toBeVisible({ timeout: 5000 });
  });
});
