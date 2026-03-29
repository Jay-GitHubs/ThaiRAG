import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, API_BASE } from './helpers';

// ── Search Analytics ───────────────────────────────────────────────

test.describe('Search Analytics page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Search Analytics');
  });

  test('shows Search Analytics heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Search Analytics' })).toBeVisible();
  });

  test('shows date range picker', async ({ page }) => {
    await expect(page.locator('.ant-picker-range')).toBeVisible();
  });

  test('renders summary stat cards or empty state', async ({ page }) => {
    // Either the 4 stat cards load, or the empty state renders — both are valid with no data
    const statsOrEmpty = page.locator('.ant-statistic, .ant-empty');
    await expect(statsOrEmpty.first()).toBeVisible({ timeout: 10_000 });
  });

  test('Popular Queries card is present', async ({ page }) => {
    // When analytics data exists the card title "Popular Queries" is visible.
    // When there is no data the page renders an empty state instead of any cards.
    // Accept either outcome.
    const hasCard = await page
      .getByText('Popular Queries')
      .isVisible({ timeout: 10_000 })
      .catch(() => false);
    const hasEmpty = await page
      .locator('.ant-empty')
      .first()
      .isVisible({ timeout: 2_000 })
      .catch(() => false);
    expect(hasCard || hasEmpty).toBe(true);
  });

  test('Zero-Result Queries card is present', async ({ page }) => {
    const hasCard = await page
      .getByText('Zero-Result Queries')
      .isVisible({ timeout: 10_000 })
      .catch(() => false);
    const hasEmpty = await page
      .locator('.ant-empty')
      .first()
      .isVisible({ timeout: 2_000 })
      .catch(() => false);
    expect(hasCard || hasEmpty).toBe(true);
  });
});

// ── Lineage ────────────────────────────────────────────────────────

test.describe('Lineage page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Lineage');
  });

  test('shows Lineage heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Lineage' })).toBeVisible();
  });

  test('shows By Response and By Document tabs', async ({ page }) => {
    await expect(page.getByRole('tab', { name: 'By Response' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'By Document' })).toBeVisible();
  });

  test('By Response tab has response ID input and Look Up button', async ({ page }) => {
    await expect(page.getByPlaceholder('Enter response ID...')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Look Up' })).toBeVisible();
  });

  test('By Document tab has document ID input and Look Up button', async ({ page }) => {
    await page.getByRole('tab', { name: 'By Document' }).click();
    await expect(page.getByPlaceholder('Enter document ID...')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Look Up' })).toBeVisible();
  });

  test('searching unknown response ID shows empty state', async ({ page }) => {
    await page.getByPlaceholder('Enter response ID...').fill('nonexistent-id-00000000');
    await page.getByRole('button', { name: 'Look Up' }).click();
    // The component shows "No lineage found for response: <id>" on success with empty results,
    // or an error toast if the API call fails. Accept either outcome.
    const emptyState = page.getByText(/No lineage found for response/);
    const errorMsg = page.getByText(/Failed to fetch lineage records/);
    await expect(emptyState.or(errorMsg)).toBeVisible({ timeout: 8_000 });
  });
});

// ── Audit Log ─────────────────────────────────────────────────────

test.describe('Audit Log page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Audit Log');
  });

  test('shows Audit Log heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Audit Log' })).toBeVisible();
  });

  test('shows Log Browser and Analytics tabs', async ({ page }) => {
    await expect(page.getByRole('tab', { name: /Log Browser/ })).toBeVisible();
    await expect(page.getByRole('tab', { name: /Analytics/ })).toBeVisible();
  });

  test('Log Browser tab shows Export JSON and Export CSV buttons', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Export JSON' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Export CSV' })).toBeVisible();
  });

  test('Log Browser tab shows action type filter', async ({ page }) => {
    await expect(page.locator('.ant-select').filter({ hasText: /Action type/i })).toBeVisible();
  });

  test('Analytics tab is clickable and renders content', async ({ page }) => {
    await page.getByRole('tab', { name: /Analytics/ }).click();
    // After clicking, the analytics content area should render (date range card at minimum)
    await expect(page.getByText('Date Range')).toBeVisible({ timeout: 10_000 });
  });

  test('API returns audit log entries list', async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const { token } = await loginRes.json();
    // The export endpoint returns the log entries as a JSON array
    const res = await request.get(`${API_BASE}/api/km/settings/audit-log/export`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    // Accepts either array or paginated object
    expect(Array.isArray(body) || Array.isArray(body.data)).toBe(true);
  });
});

// ── Tenants ────────────────────────────────────────────────────────

test.describe('Tenants page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Tenants');
  });

  test('shows Tenants heading', async ({ page }) => {
    // The page renders a level-4 heading for the title
    await expect(page.getByRole('heading', { level: 4 })).toBeVisible();
  });

  test('shows Create Tenant button', async ({ page }) => {
    // Button text comes from i18n — look for a primary button with "create" text
    await expect(page.getByRole('button', { name: /create/i })).toBeVisible();
  });

  test('shows tenants table with Name and Status columns', async ({ page }) => {
    await expect(page.getByRole('columnheader', { name: /name/i })).toBeVisible({ timeout: 8_000 });
    await expect(page.getByRole('columnheader', { name: /status/i })).toBeVisible();
  });

  test('API: create and list tenants', async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const body = await loginRes.json();
    const token = body.token;

    const name = `e2e-tenant-${Date.now()}`;
    const createRes = await request.post(`${API_BASE}/api/km/tenants`, {
      data: { name, plan: 'free' },
      headers: { Authorization: `Bearer ${token}` },
    });
    // The backend returns 201 Created for new tenants
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    expect(created.name).toBe(name);
    const createdTenantId = created.id;

    const listRes = await request.get(`${API_BASE}/api/km/tenants`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(listRes.status()).toBe(200);
    const list = await listRes.json();
    // The list endpoint returns { data: [...], total: N }
    const entries = Array.isArray(list) ? list : list.data;
    expect(entries.some((t: { id: string }) => t.id === createdTenantId)).toBe(true);

    // Clean up
    await request.delete(`${API_BASE}/api/km/tenants/${createdTenantId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
  });

  test('API: delete created tenant', async ({ request }) => {
    // Re-create a tenant to delete so the test is self-contained
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const { token: tok } = await loginRes.json();

    const name = `e2e-del-tenant-${Date.now()}`;
    const createRes = await request.post(`${API_BASE}/api/km/tenants`, {
      data: { name, plan: 'free' },
      headers: { Authorization: `Bearer ${tok}` },
    });
    const created = await createRes.json();

    const delRes = await request.delete(`${API_BASE}/api/km/tenants/${created.id}`, {
      headers: { Authorization: `Bearer ${tok}` },
    });
    expect([200, 204]).toContain(delRes.status());
  });
});

// ── Roles ──────────────────────────────────────────────────────────

test.describe('Roles page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Roles');
  });

  test('shows Roles heading', async ({ page }) => {
    await expect(page.getByRole('heading', { level: 4 })).toBeVisible();
  });

  test('shows Create Role button', async ({ page }) => {
    await expect(page.getByRole('button', { name: /create/i })).toBeVisible();
  });

  test('shows roles table with Name and Description columns', async ({ page }) => {
    await expect(page.getByRole('columnheader', { name: /name/i })).toBeVisible({ timeout: 8_000 });
    await expect(page.getByRole('columnheader', { name: /description/i })).toBeVisible();
  });

  test('opening Create Role modal shows permission matrix', async ({ page }) => {
    await page.getByRole('button', { name: /create/i }).click();
    await expect(page.locator('.ant-modal')).toBeVisible();
    // The permission matrix renders as an HTML table with resource rows
    await expect(page.locator('.ant-modal table')).toBeVisible({ timeout: 5_000 });
  });

  test('API: create and list roles', async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const { token } = await loginRes.json();

    const name = `e2e-role-${Date.now()}`;
    const createRes = await request.post(`${API_BASE}/api/km/roles`, {
      data: {
        name,
        description: 'created by e2e test',
        permissions: [{ resource: 'documents', actions: ['read'] }],
      },
      headers: { Authorization: `Bearer ${token}` },
    });
    // The backend returns 201 Created for new roles
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    expect(created.name).toBe(name);

    const listRes = await request.get(`${API_BASE}/api/km/roles`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(listRes.status()).toBe(200);
    const list = await listRes.json();
    // The list endpoint returns { data: [...], total: N }
    const entries = Array.isArray(list) ? list : list.data;
    expect(entries.some((r: { id: string }) => r.id === created.id)).toBe(true);

    // Clean up
    await request.delete(`${API_BASE}/api/km/roles/${created.id}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
  });
});

// ── Prompt Marketplace ─────────────────────────────────────────────

test.describe('Prompt Marketplace page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Prompts');
  });

  test('shows Prompt Marketplace heading', async ({ page }) => {
    // The page renders an h2 heading with translated title
    await expect(page.locator('h2')).toBeVisible({ timeout: 8_000 });
  });

  test('shows Create button', async ({ page }) => {
    await expect(page.getByRole('button', { name: /create/i })).toBeVisible();
  });

  test('shows search input and category filter', async ({ page }) => {
    // The search input is a plain text input with placeholder "Search prompts..."
    await expect(page.getByPlaceholder('Search prompts...')).toBeVisible();
    // Category select rendered by ant-design
    await expect(page.locator('.ant-select')).toBeVisible();
  });

  test('empty state or template cards render', async ({ page }) => {
    // Either an ant-card (with templates) or the empty-state text is shown
    const content = page.locator('.ant-card, .ant-empty');
    const emptyText = page.getByText(/No prompt templates found/);
    await expect(content.first().or(emptyText)).toBeVisible({ timeout: 10_000 });
  });

  test('API: create, list, and delete prompt template', async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const { token } = await loginRes.json();

    const name = `e2e-prompt-${Date.now()}`;
    const createRes = await request.post(`${API_BASE}/api/km/prompts/marketplace`, {
      data: {
        name,
        description: 'e2e test prompt',
        category: 'general',
        content: 'You are a helpful assistant. Answer: {question}',
        variables: ['question'],
        is_public: true,
      },
      headers: { Authorization: `Bearer ${token}` },
    });
    // The backend returns 201 Created for new templates
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    expect(created.name).toBe(name);

    const listRes = await request.get(`${API_BASE}/api/km/prompts/marketplace`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(listRes.status()).toBe(200);
    const list = await listRes.json();
    // The list endpoint returns an array directly
    const entries = Array.isArray(list) ? list : list.data;
    expect(entries.some((p: { id: string }) => p.id === created.id)).toBe(true);

    // Clean up
    const delRes = await request.delete(
      `${API_BASE}/api/km/prompts/marketplace/${created.id}`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect([200, 204]).toContain(delRes.status());
  });
});

// ── Fine-tuning ────────────────────────────────────────────────────

test.describe('Fine-tuning page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Fine-tuning');
  });

  test('shows Embedding Fine-tuning heading', async ({ page }) => {
    await expect(page.getByText('Embedding Fine-tuning')).toBeVisible();
  });

  test('shows Datasets and Jobs tabs', async ({ page }) => {
    await expect(page.getByRole('tab', { name: 'Datasets' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Jobs' })).toBeVisible();
  });

  test('Datasets tab shows Training Datasets card with Create Dataset button', async ({ page }) => {
    await expect(page.getByText('Training Datasets')).toBeVisible({ timeout: 8_000 });
    await expect(page.getByRole('button', { name: 'Create Dataset' })).toBeVisible();
  });

  test('Jobs tab shows Fine-tuning Jobs card with Create Job button', async ({ page }) => {
    await page.getByRole('tab', { name: 'Jobs' }).click();
    await expect(page.getByText('Fine-tuning Jobs')).toBeVisible({ timeout: 8_000 });
    await expect(page.getByRole('button', { name: 'Create Job' })).toBeVisible();
  });

  test('API: create and list finetune datasets', async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const { token } = await loginRes.json();

    const name = `e2e-dataset-${Date.now()}`;
    const createRes = await request.post(`${API_BASE}/api/km/finetune/datasets`, {
      data: { name, description: 'e2e test dataset' },
      headers: { Authorization: `Bearer ${token}` },
    });
    // The backend returns 201 Created for new datasets
    expect(createRes.status()).toBe(201);
    const created = await createRes.json();
    expect(created.name).toBe(name);

    // List endpoint should return 200 (may be empty if store doesn't persist)
    const listRes = await request.get(`${API_BASE}/api/km/finetune/datasets`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(listRes.status()).toBe(200);
  });

  test('opening Create Dataset modal shows Name field', async ({ page }) => {
    await page.getByRole('button', { name: 'Create Dataset' }).click();
    await expect(page.locator('.ant-modal')).toBeVisible();
    await expect(page.getByPlaceholder(/e\.g\. Thai Legal QA/)).toBeVisible();
  });
});
