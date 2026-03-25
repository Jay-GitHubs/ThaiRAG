import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

test.describe('Scoped Settings', () => {
  const suffix = Date.now();
  const orgName = `ScopeOrg-${suffix}`;
  let token: string;
  let orgId: string;

  test.beforeAll(async ({ request }) => {
    // Login via API
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const data = await loginRes.json();
    token = data.token;
    const headers = { Authorization: `Bearer ${token}` };

    // Create an org for scoped settings tests
    const orgRes = await request.post(`${API_BASE}/api/km/orgs`, {
      data: { name: orgName },
      headers,
    });
    expect(orgRes.ok()).toBeTruthy();
    const org = await orgRes.json();
    orgId = org.id;
  });

  test.afterAll(async ({ request }) => {
    // Cleanup: delete the org
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('scope selector is visible on settings page', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // Scope selector label
    await expect(page.getByText('Settings Scope:')).toBeVisible();

    // Default scope tag shows "Global"
    await expect(page.locator('.ant-tag').filter({ hasText: 'Global' })).toBeVisible();
  });

  test('scope selector shows org option after hierarchy creation', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // Open the scope selector dropdown
    const scopeSelect = page.locator('.ant-select').filter({ hasText: /Global/ });
    await scopeSelect.click();
    await page.waitForTimeout(500);

    // Should show org option in the dropdown
    await expect(page.getByText(`Org: ${orgName}`)).toBeVisible();
  });

  test('switching scope changes the scope tag', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // Select org scope
    const scopeSelect = page.locator('.ant-select').filter({ hasText: /Global/ });
    await scopeSelect.click();
    await page.waitForTimeout(500);
    await page.getByText(`Org: ${orgName}`).click();
    await page.waitForTimeout(500);

    // Tag should now show "Organization"
    await expect(page.locator('.ant-tag').filter({ hasText: 'Organization' })).toBeVisible();

    // Should show inheritance info text
    await expect(page.getByText('Overrides at this level take precedence')).toBeVisible();
  });

  test('scoped pipeline config loads and saves independently', async ({ page }) => {
    const headers = { Authorization: `Bearer ${token}` };

    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // Go to Chat & Response Pipeline tab at global scope
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Note the global pipeline switch state
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const globalChecked = await pipelineSwitch.getAttribute('aria-checked');

    // Switch to org scope
    const scopeSelect = page.locator('.ant-select').filter({ hasText: /Global/ });
    await scopeSelect.click();
    await page.waitForTimeout(500);
    await page.getByText(`Org: ${orgName}`).click();
    await page.waitForTimeout(1000);

    // Pipeline should reload — the config should be inherited from global initially
    await expect(pipelineSwitch).toBeVisible({ timeout: 5000 });

    // Toggle the pipeline switch to opposite of global state (creating an org-level override)
    const orgCheckedBefore = await pipelineSwitch.getAttribute('aria-checked');
    await pipelineSwitch.click();
    await page.waitForTimeout(300);

    const orgCheckedAfter = await pipelineSwitch.getAttribute('aria-checked');
    expect(orgCheckedAfter).not.toBe(orgCheckedBefore);

    // Save the scoped config
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Response Pipeline' });
    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    // Verify via API that the scoped config has the override
    const scopedRes = await page.request.get(
      `${API_BASE}/api/km/settings/chat-pipeline?scope_type=org&scope_id=${orgId}`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect(scopedRes.status()).toBe(200);
    const scopedData = await scopedRes.json();

    // The pipeline enabled state should reflect what we just saved
    const expectedEnabled = orgCheckedAfter === 'true';
    expect(scopedData.enabled).toBe(expectedEnabled);

    // Switch back to global — verify it still has the original value
    const scopeSelectGlobal = page.locator('.ant-select').filter({ hasText: /Org/ });
    await scopeSelectGlobal.click();
    await page.waitForTimeout(500);
    await page.getByText('Global (Default)').click();
    await page.waitForTimeout(1000);

    const globalCheckedNow = await pipelineSwitch.getAttribute('aria-checked');
    expect(globalCheckedNow).toBe(globalChecked);

    // Cleanup: reset the org override via API
    await page.request.delete(
      `${API_BASE}/api/km/settings/scoped?scope_type=org&scope_id=${orgId}`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
  });

  test('scope-info API returns override information', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    // First, set a scoped setting via API
    await request.put(
      `${API_BASE}/api/km/settings/chat-pipeline?scope_type=org&scope_id=${orgId}`,
      {
        data: { enabled: true },
        headers,
      },
    );

    // Get scope info
    const res = await request.get(
      `${API_BASE}/api/km/settings/scope-info?scope_type=org&scope_id=${orgId}`,
      { headers },
    );
    expect(res.status()).toBe(200);

    const data = await res.json();
    // Should have overrides structure
    expect(data.overrides).toBeDefined();
    expect(data.overrides.org).toBeDefined();
    expect(data.overrides.global).toBeDefined();

    // The org scope should have at least one override key
    expect(data.overrides.org.length).toBeGreaterThan(0);
  });

  test('reset scoped setting removes override', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    // Set a scoped setting
    await request.put(
      `${API_BASE}/api/km/settings/chat-pipeline?scope_type=org&scope_id=${orgId}`,
      {
        data: { enabled: false },
        headers,
      },
    );

    // Verify it exists in scope-info
    const infoRes = await request.get(
      `${API_BASE}/api/km/settings/scope-info?scope_type=org&scope_id=${orgId}`,
      { headers },
    );
    const infoBefore = await infoRes.json();
    expect(infoBefore.overrides.org.length).toBeGreaterThan(0);

    // Reset all scoped settings at org level
    const resetRes = await request.delete(
      `${API_BASE}/api/km/settings/scoped?scope_type=org&scope_id=${orgId}`,
      { headers },
    );
    expect(resetRes.status()).toBe(200);

    // Verify overrides are cleared
    const infoRes2 = await request.get(
      `${API_BASE}/api/km/settings/scope-info?scope_type=org&scope_id=${orgId}`,
      { headers },
    );
    const infoAfter = await infoRes2.json();
    expect(infoAfter.overrides.org.length).toBe(0);
  });

  test('scoped config persists after page reload', async ({ page }) => {
    const headers = { Authorization: `Bearer ${token}` };

    // Set a distinct scoped config via API
    await page.request.put(
      `${API_BASE}/api/km/settings/chat-pipeline?scope_type=org&scope_id=${orgId}`,
      {
        data: { enabled: true, max_context_tokens: 9999 },
        headers: { Authorization: `Bearer ${token}` },
      },
    );

    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // Switch to org scope
    const scopeSelect = page.locator('.ant-select').filter({ hasText: /Global/ });
    await scopeSelect.click();
    await page.waitForTimeout(500);
    await page.getByText(`Org: ${orgName}`).click();
    await page.waitForTimeout(500);

    // Go to Chat & Response Pipeline tab
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Pipeline should be enabled (as we set via API)
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    await expect(pipelineSwitch).toHaveAttribute('aria-checked', 'true');

    // Reload the page
    await page.reload();
    await page.waitForTimeout(1500);

    // Navigate back to settings and select org scope again
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    const scopeSelect2 = page.locator('.ant-select').filter({ hasText: /Global/ });
    await scopeSelect2.click();
    await page.waitForTimeout(500);
    await page.getByText(`Org: ${orgName}`).click();
    await page.waitForTimeout(500);

    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Should still show the scoped config
    await expect(pipelineSwitch).toHaveAttribute('aria-checked', 'true');

    // Cleanup: reset the override
    await page.request.delete(
      `${API_BASE}/api/km/settings/scoped?scope_type=org&scope_id=${orgId}`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
  });
});
