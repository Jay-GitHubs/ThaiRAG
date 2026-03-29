import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

// ── Shared state ──────────────────────────────────────────────────────
let token: string;

// ── Identity Provider API CRUD ────────────────────────────────────────
test.describe('Identity Provider API CRUD', () => {
  test.beforeAll(async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(res.ok()).toBeTruthy();
    const data = await res.json();
    token = data.token;
  });

  test('create OIDC provider returns 201 with provider data', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    const createRes = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: `E2E OIDC - ${Date.now()}`,
        provider_type: 'oidc',
        enabled: true,
        config: {
          issuer_url: 'https://keycloak.example.com/realms/test',
          client_id: 'thairag-test',
          client_secret: 'test-secret-abc',
          scopes: 'openid profile email',
          redirect_uri: 'http://localhost:8080/api/auth/oauth/callback',
        },
      },
      headers,
    });

    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    expect(created.id).toBeTruthy();
    expect(created.provider_type).toBe('oidc');
    expect(created.enabled).toBe(true);

    // Cleanup
    await request.delete(`${API_BASE}/api/km/settings/identity-providers/${created.id}`, {
      headers,
    });
  });

  test('full OIDC provider lifecycle: create, list, get, update, delete', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const name = `Lifecycle OIDC - ${Date.now()}`;

    // Create
    const createRes = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name,
        provider_type: 'oidc',
        enabled: true,
        config: {
          issuer_url: 'https://sso.example.com/realms/master',
          client_id: 'lifecycle-client',
          client_secret: 'lifecycle-secret',
        },
      },
      headers,
    });
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    const idpId: string = created.id;

    // List — provider should appear
    const listRes = await request.get(`${API_BASE}/api/km/settings/identity-providers`, {
      headers,
    });
    expect(listRes.ok()).toBeTruthy();
    const listBody = await listRes.json();
    expect(listBody.data).toBeInstanceOf(Array);
    expect(listBody.total).toBeGreaterThanOrEqual(1);
    const found = listBody.data.find((p: { id: string }) => p.id === idpId);
    expect(found).toBeTruthy();
    expect(found.name).toBe(name);

    // Get by ID
    const getRes = await request.get(
      `${API_BASE}/api/km/settings/identity-providers/${idpId}`,
      { headers },
    );
    expect(getRes.ok()).toBeTruthy();
    const fetched = await getRes.json();
    expect(fetched.id).toBe(idpId);
    expect(fetched.provider_type).toBe('oidc');

    // Update — rename and disable
    const updatedName = `${name} (updated)`;
    const updateRes = await request.put(
      `${API_BASE}/api/km/settings/identity-providers/${idpId}`,
      {
        data: {
          name: updatedName,
          provider_type: 'oidc',
          enabled: false,
          config: {
            issuer_url: 'https://sso.example.com/realms/master',
            client_id: 'lifecycle-client-v2',
            client_secret: 'lifecycle-secret-v2',
          },
        },
        headers,
      },
    );
    expect(updateRes.ok()).toBeTruthy();
    const updated = await updateRes.json();
    expect(updated.name).toBe(updatedName);
    expect(updated.enabled).toBe(false);

    // Delete
    const deleteRes = await request.delete(
      `${API_BASE}/api/km/settings/identity-providers/${idpId}`,
      { headers },
    );
    expect(deleteRes.status()).toBe(204);

    // Confirm gone — GET should return 404
    const missingRes = await request.get(
      `${API_BASE}/api/km/settings/identity-providers/${idpId}`,
      { headers },
    );
    expect(missingRes.status()).toBe(404);
  });

  test('enabled provider appears in public /api/auth/providers endpoint', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    // Create an enabled OIDC provider
    const createRes = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: `Public OIDC - ${Date.now()}`,
        provider_type: 'oidc',
        enabled: true,
        config: {
          issuer_url: 'https://public.example.com/realms/test',
          client_id: 'public-client',
        },
      },
      headers,
    });
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();

    // Public endpoint — no auth required
    const publicRes = await request.get(`${API_BASE}/api/auth/providers`);
    expect(publicRes.ok()).toBeTruthy();
    const publicProviders = await publicRes.json();
    expect(publicProviders).toBeInstanceOf(Array);

    const found = publicProviders.find((p: { id: string }) => p.id === created.id);
    expect(found).toBeTruthy();
    expect(found.name).toBeTruthy();
    expect(found.provider_type).toBe('oidc');
    // Public endpoint should NOT expose config
    expect(found.config).toBeUndefined();

    // Cleanup
    await request.delete(`${API_BASE}/api/km/settings/identity-providers/${created.id}`, {
      headers,
    });
  });

  test('disabled provider does NOT appear in public /api/auth/providers', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    const createRes = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: `Disabled OIDC - ${Date.now()}`,
        provider_type: 'oidc',
        enabled: false,
        config: { issuer_url: 'https://disabled.example.com', client_id: 'disabled-client' },
      },
      headers,
    });
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();

    const publicRes = await request.get(`${API_BASE}/api/auth/providers`);
    expect(publicRes.ok()).toBeTruthy();
    const publicProviders = await publicRes.json();
    const found = publicProviders.find((p: { id: string }) => p.id === created.id);
    expect(found).toBeUndefined();

    // Cleanup
    await request.delete(`${API_BASE}/api/km/settings/identity-providers/${created.id}`, {
      headers,
    });
  });

  test('create rejects invalid provider_type', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    const res = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: 'Invalid Type',
        provider_type: 'kerberos',
        enabled: true,
        config: {},
      },
      headers,
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error.message).toContain('provider_type');
  });

  test('IdP endpoints require authentication', async ({ request }) => {
    const listRes = await request.get(`${API_BASE}/api/km/settings/identity-providers`);
    expect(listRes.status()).toBe(401);

    const createRes = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: { name: 'No Auth', provider_type: 'oidc', config: {} },
    });
    expect(createRes.status()).toBe(401);
  });

  test('get non-existent provider returns 404', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const fakeId = '00000000-0000-0000-0000-000000000000';
    const res = await request.get(
      `${API_BASE}/api/km/settings/identity-providers/${fakeId}`,
      { headers },
    );
    expect(res.status()).toBe(404);
  });
});

// ── LDAP Login Error Handling ─────────────────────────────────────────
test.describe('LDAP Login Error Handling', () => {
  test('LDAP endpoint returns 501 (not yet implemented)', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/ldap`, {
      data: { username: 'alice', password: 'wrongpassword' },
    });
    // LDAP is stubbed — expect 501 Not Implemented
    expect(res.status()).toBe(501);
    const body = await res.json();
    expect(body.error.type).toBe('not_implemented');
  });

  test('LDAP endpoint responds even without LDAP provider configured', async ({ request }) => {
    // The endpoint exists and returns a consistent error regardless of provider state
    const res = await request.post(`${API_BASE}/api/auth/ldap`, {
      data: { username: 'testuser', password: 'testpass' },
    });
    // Either 501 (stub) or 400/404 (no provider) — never 500
    expect([400, 404, 501]).toContain(res.status());
    const body = await res.json();
    expect(body.error).toBeTruthy();
    expect(body.error.message).toBeTruthy();
  });

  test('LDAP endpoint does not expose internal errors', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/ldap`, {
      data: { username: '', password: '' },
    });
    expect(res.status()).not.toBe(500);
    const body = await res.json();
    expect(body.error).toBeTruthy();
  });
});

// ── OAuth / OIDC Flow Validation ──────────────────────────────────────
test.describe('OAuth Flow Validation', () => {
  let oauthToken: string;

  test.beforeAll(async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    if (res.ok()) {
      const data = await res.json();
      oauthToken = data.token;
    }
  });

  test('authorize endpoint returns error for non-existent provider', async ({ request }) => {
    const fakeId = '00000000-0000-0000-0000-000000000001';
    const res = await request.get(`${API_BASE}/api/auth/oauth/${fakeId}/authorize`, {
      // Prevent Playwright from following the redirect
      maxRedirects: 0,
    });
    // 404 (provider not found), or 429 if rate-limited
    expect([400, 404, 422, 429]).toContain(res.status());
  });

  test('authorize endpoint rejects malformed provider ID', async ({ request }) => {
    const res = await request.get(`${API_BASE}/api/auth/oauth/not-a-uuid/authorize`, {
      maxRedirects: 0,
    });
    // Axum path extraction fails — 400 or 422, or 429 if rate-limited
    expect([400, 404, 422, 429]).toContain(res.status());
  });

  test('callback endpoint rejects invalid state parameter', async ({ request }) => {
    const res = await request.get(`${API_BASE}/api/auth/oauth/callback?code=fakecode&state=badstate`, {
      maxRedirects: 0,
    });
    // Should return an auth error, not 500 (429 if rate-limited)
    expect([400, 401, 302, 429]).toContain(res.status());
    if (res.status() !== 302) {
      const body = await res.json();
      expect(body.error).toBeTruthy();
    }
  });

  test('authorize endpoint redirects for a valid enabled OIDC provider', async ({ request }) => {
    test.skip(!oauthToken, 'Could not obtain auth token (rate-limited)');
    const headers = { Authorization: `Bearer ${oauthToken}` };

    const createRes = await request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: `OAuth Test - ${Date.now()}`,
        provider_type: 'oidc',
        enabled: true,
        config: {
          // Use a real-enough issuer to trigger a redirect attempt
          issuer_url: 'https://accounts.google.com',
          client_id: 'test-client-id',
          client_secret: 'test-secret',
          redirect_uri: 'http://localhost:8080/api/auth/oauth/callback',
          scopes: 'openid email profile',
        },
      },
      headers,
    });
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();

    // Attempt the authorize redirect — the server will try to build the auth URL.
    // It may succeed (3xx) or fail (500/400) depending on whether it can reach the issuer.
    // Either way, a 404 would indicate routing is broken.
    const authorizeRes = await request.get(
      `${API_BASE}/api/auth/oauth/${created.id}/authorize`,
      { maxRedirects: 0 },
    );
    expect(authorizeRes.status()).not.toBe(404);

    // Cleanup
    await request.delete(`${API_BASE}/api/km/settings/identity-providers/${created.id}`, {
      headers,
    });
  });
});

// ── Identity Provider UI Tests ────────────────────────────────────────
test.describe('Identity Providers Settings UI', () => {
  test('Settings page has an Identity Providers tab', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(500);

    // The Settings page uses Ant Design Tabs — find the Identity Providers tab
    const idpTab = page.getByRole('tab', { name: 'Identity Providers' });
    await expect(idpTab).toBeVisible({ timeout: 5000 });
  });

  test('Identity Providers tab shows Add Provider button', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');

    await page.getByRole('tab', { name: 'Identity Providers' }).click();
    await page.waitForTimeout(500);

    await expect(page.getByRole('button', { name: 'Add Provider' })).toBeVisible({ timeout: 5000 });
  });

  test('Identity Providers tab shows providers table', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');

    await page.getByRole('tab', { name: 'Identity Providers' }).click();
    await page.waitForTimeout(500);

    // Table headers
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole('columnheader', { name: 'Type' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Enabled' })).toBeVisible();
  });

  test('can create an OIDC provider via the UI modal', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');

    await page.getByRole('tab', { name: 'Identity Providers' }).click();
    await page.waitForTimeout(500);

    // Open the Add Provider modal
    await page.getByRole('button', { name: 'Add Provider' }).click();

    const modal = page.getByRole('dialog', { name: 'Add Identity Provider' });
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Fill in the name
    const providerName = `UI OIDC Test - ${Date.now()}`;
    await modal.getByLabel('Name').fill(providerName);

    // Select OIDC type — click the Select next to the "Type" label
    await modal.locator('.ant-select').first().click();
    await page.locator('.ant-select-dropdown').getByTitle('OIDC').click();
    await page.waitForTimeout(300);

    // Fill OIDC-specific fields that appear after type selection
    const issuerInput = modal.getByLabel('issuer url');
    if (await issuerInput.isVisible({ timeout: 2000 }).catch(() => false)) {
      await issuerInput.fill('https://keycloak.example.com/realms/test');
    }
    const clientIdInput = modal.getByLabel('client id');
    if (await clientIdInput.isVisible({ timeout: 2000 }).catch(() => false)) {
      await clientIdInput.fill('ui-test-client');
    }

    // Submit
    await modal.getByRole('button', { name: 'OK' }).click();
    await expect(modal).not.toBeVisible({ timeout: 5000 });

    // Verify provider appears in table
    await expect(page.getByText(providerName)).toBeVisible({ timeout: 5000 });

    // Cleanup — delete via API
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token: authToken } = await loginRes.json();
    const headers = { Authorization: `Bearer ${authToken}` };

    const listRes = await page.request.get(`${API_BASE}/api/km/settings/identity-providers`, {
      headers,
    });
    const listBody = await listRes.json();
    const created = listBody.data.find((p: { name: string }) => p.name === providerName);
    if (created) {
      await page.request.delete(
        `${API_BASE}/api/km/settings/identity-providers/${created.id}`,
        { headers },
      );
    }
  });

  test('can delete an OIDC provider via the UI', async ({ page }) => {
    // Create a provider via API first so we have something to delete
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token: authToken } = await loginRes.json();
    const headers = { Authorization: `Bearer ${authToken}` };

    const providerName = `UI Delete Test - ${Date.now()}`;
    const createRes = await page.request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: providerName,
        provider_type: 'ldap',
        enabled: false,
        config: {
          server_url: 'ldap://ldap.example.com:389',
          bind_dn: 'cn=admin,dc=example,dc=com',
          bind_password: 'ldap-admin-pass',
          search_base: 'ou=users,dc=example,dc=com',
          search_filter: '(uid={username})',
        },
      },
      headers,
    });
    expect(createRes.status()).toBe(201);

    // Navigate to the IdP tab
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Identity Providers' }).click();
    await page.waitForTimeout(1000);

    // Find the row with our provider name
    const row = page.locator('tr', { hasText: providerName });
    await expect(row).toBeVisible({ timeout: 5000 });

    // Click the Delete button inside that row
    await row.getByRole('button', { name: 'Delete' }).click();

    // Confirm the Ant Design Popconfirm
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();

    // Provider should no longer appear in the table
    await expect(page.getByText(providerName, { exact: true })).not.toBeVisible({ timeout: 5000 });
  });

  test('can edit an existing provider via the UI', async ({ page }) => {
    // Create a provider via API
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token: authToken } = await loginRes.json();
    const headers = { Authorization: `Bearer ${authToken}` };

    const providerName = `UI Edit Test - ${Date.now()}`;
    const createRes = await page.request.post(`${API_BASE}/api/km/settings/identity-providers`, {
      data: {
        name: providerName,
        provider_type: 'oauth2',
        enabled: true,
        config: {
          authorize_url: 'https://provider.example.com/oauth/authorize',
          token_url: 'https://provider.example.com/oauth/token',
          client_id: 'edit-test-client',
        },
      },
      headers,
    });
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();

    // Navigate to IdP settings
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Identity Providers' }).click();
    await page.waitForTimeout(1000);

    // Click Edit on the row
    const row = page.locator('tr', { hasText: providerName });
    await expect(row).toBeVisible({ timeout: 5000 });
    await row.getByRole('button', { name: 'Edit' }).click();

    const editModal = page.getByRole('dialog', { name: 'Edit Identity Provider' });
    await expect(editModal).toBeVisible({ timeout: 5000 });

    // Rename the provider
    const updatedName = `${providerName} (edited)`;
    const nameInput = editModal.getByLabel('Name');
    await nameInput.clear();
    await nameInput.fill(updatedName);

    await editModal.getByRole('button', { name: 'OK' }).click();
    await expect(editModal).not.toBeVisible({ timeout: 5000 });

    // Updated name should appear in table
    await expect(page.getByText(updatedName)).toBeVisible({ timeout: 5000 });

    // Cleanup
    await page.request.delete(
      `${API_BASE}/api/km/settings/identity-providers/${created.id}`,
      { headers },
    );
  });
});
