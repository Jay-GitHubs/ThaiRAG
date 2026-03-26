import { test, expect } from '@playwright/test';
import { login, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

// ─── Auth & Session ───────────────────────────────────────────────────

test.describe('Auth & Session', () => {
  test('login shows error on invalid credentials', async ({ page }) => {
    await page.goto('/login');
    await page.getByPlaceholder('Email').fill('bad@example.com');
    await page.getByPlaceholder('Password').fill('wrong');
    await page.getByRole('button', { name: 'Sign In' }).click();

    // Should see error message and remain on login page
    await expect(page.getByText(/Invalid email or password|Login failed/)).toBeVisible({
      timeout: 5000,
    });
    await expect(page).toHaveURL(/\/login/);
  });

  test('login shows validation errors for empty fields', async ({ page }) => {
    await page.goto('/login');
    await page.getByRole('button', { name: 'Sign In' }).click();
    await expect(page.getByText('Email required')).toBeVisible();
    await expect(page.getByText('Password required')).toBeVisible();
  });

  test('successful login redirects to dashboard', async ({ page }) => {
    await login(page);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
    await expect(page.getByText(`Logged in as ${TEST_EMAIL}`)).toBeVisible();
  });

  test('session survives page refresh', async ({ page }) => {
    await login(page);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();

    // Hard refresh
    await page.reload();
    await page.waitForTimeout(1000);

    // Should still be on dashboard, not redirected to login
    await expect(page).not.toHaveURL(/\/login/);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
    await expect(page.getByText(`Logged in as ${TEST_EMAIL}`)).toBeVisible();
  });

  test('session survives navigating to another page then refreshing', async ({ page }) => {
    await login(page);

    // Navigate to Users page
    await page.getByRole('menu').getByText('Users', { exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Users' })).toBeVisible();

    // Refresh on the Users page
    await page.reload();
    await page.waitForTimeout(1000);

    // Should still be on Users page
    await expect(page).toHaveURL(/\/users/);
    await expect(page.getByRole('heading', { name: 'Users' })).toBeVisible();
  });

  test('visiting /login when already authenticated redirects to dashboard', async ({ page }) => {
    await login(page);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();

    // Try navigating to /login directly
    await page.goto('/login');
    await page.waitForTimeout(500);

    // Should redirect back to dashboard
    await expect(page).not.toHaveURL(/\/login/);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
  });

  test('logout clears session and redirects to login', async ({ page }) => {
    await login(page);
    await page.getByRole('button', { name: 'Logout' }).click();
    await page.waitForURL('/login', { timeout: 5000 });
    await expect(page.getByText('ThaiRAG Admin')).toBeVisible();

    // After logout, refresh should stay on login (session cleared)
    await page.reload();
    await page.waitForTimeout(500);
    await expect(page).toHaveURL(/\/login/);
  });

  test('protected routes redirect to login when not authenticated', async ({ page }) => {
    // Clear any residual session
    await page.goto('/login');
    await page.evaluate(() => {
      sessionStorage.clear();
    });

    const protectedPaths = ['/km', '/documents', '/users', '/permissions', '/system'];
    for (const path of protectedPaths) {
      await page.goto(path);
      await page.waitForTimeout(500);
      await expect(page).toHaveURL(/\/login/, {
        timeout: 5000,
      });
    }
  });
});

// ─── Navigation ───────────────────────────────────────────────────────

test.describe('Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('sidebar navigates to all pages correctly', async ({ page }) => {
    const pages = [
      { menu: 'KM Hierarchy', heading: 'KM Hierarchy' },
      { menu: 'Documents', heading: 'Documents' },
      { menu: 'Users', heading: 'Users' },
      { menu: 'Permissions', heading: 'Permissions' },
      { menu: 'Health', heading: 'System Health' },
      { menu: 'Dashboard', heading: 'Dashboard' },
    ];

    for (const { menu, heading } of pages) {
      await page.getByRole('menu').getByText(menu, { exact: true }).click();
      await expect(page.getByRole('heading', { name: heading })).toBeVisible({ timeout: 5000 });
    }
  });

  test('sidebar highlights active menu item', async ({ page }) => {
    await page.getByRole('menu').getByText('Users', { exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Users' })).toBeVisible();

    const menuItem = page.getByRole('menu').locator('.ant-menu-item-selected');
    await expect(menuItem).toContainText('Users');
  });
});

// ─── Dashboard ────────────────────────────────────────────────────────

test.describe('Dashboard', () => {
  test('shows stats cards and health status', async ({ page }) => {
    await login(page);

    const main = page.getByRole('main');
    await expect(main.getByText('Organizations')).toBeVisible();
    await expect(main.getByText('Users')).toBeVisible();
    await expect(main.getByText('Active Sessions')).toBeVisible();
    await expect(main.getByText('HTTP Requests')).toBeVisible();
    await expect(main.getByText('LLM Tokens Used')).toBeVisible();
    await expect(main.getByText('Health Status')).toBeVisible();
  });
});

// ─── KM Hierarchy CRUD ───────────────────────────────────────────────

test.describe('KM Hierarchy', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.getByRole('menu').getByText('KM Hierarchy', { exact: true }).click();
    await expect(page.getByRole('heading', { name: 'KM Hierarchy' })).toBeVisible();
  });

  test('create org, dept, workspace and then delete', async ({ page }) => {
    const orgName = `TestOrg-${Date.now()}`;
    const deptName = `TestDept-${Date.now()}`;
    const wsName = `TestWS-${Date.now()}`;

    // Create org
    await page.getByRole('button', { name: 'New Org' }).click();
    await page.getByPlaceholder('Organization name').fill(orgName);
    await page.getByRole('dialog').getByRole('button', { name: 'OK' }).click();
    await page.waitForTimeout(1000);

    // Org should appear in tree
    await expect(page.getByText(orgName)).toBeVisible({ timeout: 5000 });

    // Click org in tree to select it
    await page.getByText(orgName).click();
    await expect(page.getByText(`Organization: ${orgName}`)).toBeVisible({ timeout: 5000 });

    // Create dept
    await page.getByRole('button', { name: 'New Department' }).click();
    await page.getByPlaceholder('Department name').fill(deptName);
    await page.getByRole('dialog').getByRole('button', { name: 'OK' }).click();
    await page.waitForTimeout(1000);

    // Dept should appear in table
    await expect(page.getByRole('cell', { name: deptName })).toBeVisible({ timeout: 5000 });

    // Expand the correct org node in tree — find the tree node containing our org name,
    // then click its switcher icon to load children
    const orgTreeNode = page.locator('.ant-tree-treenode', { hasText: orgName });
    await orgTreeNode.locator('.ant-tree-switcher').click();
    await page.waitForTimeout(1000);

    // Now click dept in tree
    await page.locator('.ant-tree-treenode', { hasText: deptName }).click();
    await expect(page.getByText(`Department: ${deptName}`)).toBeVisible({ timeout: 5000 });

    // Create workspace
    await page.getByRole('button', { name: 'New Workspace' }).click();
    await page.getByPlaceholder('Workspace name').fill(wsName);
    await page.getByRole('dialog').getByRole('button', { name: 'OK' }).click();
    await page.waitForTimeout(1000);

    // Workspace should appear in table
    await expect(page.getByRole('cell', { name: wsName })).toBeVisible({ timeout: 5000 });

    // Delete workspace via popconfirm
    const wsRow = page.getByRole('row').filter({ hasText: wsName });
    await wsRow.getByRole('button').click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await page.waitForTimeout(500);

    // Go back to org and delete dept
    await page.locator('.ant-tree-treenode', { hasText: orgName }).click();
    await expect(page.getByText(`Organization: ${orgName}`)).toBeVisible({ timeout: 5000 });
    const deptRow = page.getByRole('row').filter({ hasText: deptName });
    await deptRow.getByRole('button').click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await page.waitForTimeout(500);

    // Delete org
    await page.getByRole('button', { name: 'Delete Org' }).click();
    await page.locator('.ant-popconfirm').getByRole('button', { name: 'OK' }).click();
    await page.waitForTimeout(500);
  });

  test('create org with empty name is prevented', async ({ page }) => {
    await page.getByRole('button', { name: 'New Org' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'OK' }).click();
    // Modal should still be open (empty name check)
    await expect(page.getByPlaceholder('Organization name')).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();
  });
});

// ─── Users Page ───────────────────────────────────────────────────────

test.describe('Users Page', () => {
  test('shows user list with expected columns', async ({ page }) => {
    await login(page);
    await page.getByRole('menu').getByText('Users', { exact: true }).click();

    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Email' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'User ID' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Created' })).toBeVisible();

    // Current user should be in the list
    await expect(page.getByRole('cell', { name: TEST_EMAIL })).toBeVisible();
  });
});

// ─── Health Page ──────────────────────────────────────────────────────

test.describe('Health Page', () => {
  test('shows health status and metrics', async ({ page }) => {
    await login(page);
    await page.getByRole('menu').getByText('Health', { exact: true }).click();

    await expect(page.getByText('System Health')).toBeVisible();
    await expect(page.getByText('Health Check')).toBeVisible();
    await expect(page.getByText('Prometheus Metrics')).toBeVisible();

    // Health status should show ok
    await expect(page.getByText('ok', { exact: true })).toBeVisible({ timeout: 5000 });
  });

  test('deep health check works', async ({ page }) => {
    await login(page);
    await page.getByRole('menu').getByText('Health', { exact: true }).click();

    await page.getByRole('button', { name: 'Run Deep Check' }).click();
    await expect(page.getByText('ok')).toBeVisible({ timeout: 10000 });
  });
});

// ─── Documents Page ───────────────────────────────────────────────────

test.describe('Documents Page', () => {
  test('shows cascade selectors that filter correctly', async ({ page }) => {
    await login(page);
    await page.getByRole('menu').getByText('Documents', { exact: true }).click();

    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    // Department and Workspace selects should be disabled initially
    const deptSelect = page.locator('.ant-select').nth(1);
    await expect(deptSelect).toHaveClass(/ant-select-disabled/);
    const wsSelect = page.locator('.ant-select').nth(2);
    await expect(wsSelect).toHaveClass(/ant-select-disabled/);

    await expect(
      page.getByText('Select an organization, department, and workspace to view documents.'),
    ).toBeVisible();
  });
});

// ─── Permissions Page ─────────────────────────────────────────────────

test.describe('Permissions Page', () => {
  test('shows org selector and placeholder text', async ({ page }) => {
    await login(page);
    await page.getByRole('menu').getByText('Permissions', { exact: true }).click();

    await expect(page.getByRole('heading', { name: 'Permissions' })).toBeVisible();
    await expect(page.getByText('Select an organization to manage permissions.')).toBeVisible();
  });
});

// ─── Theme Toggle ─────────────────────────────────────────────────────

test.describe('Theme', () => {
  test('toggle on login page switches theme', async ({ page }) => {
    await page.goto('/login');
    await page.waitForTimeout(300);

    const toggleBtn = page.getByTitle(/Switch to dark mode|Switch to light mode/);
    await expect(toggleBtn).toBeVisible();

    await toggleBtn.click();
    await page.waitForTimeout(300);

    const bg = await page.evaluate(() => document.body.style.background);
    expect(bg).toBeTruthy();
  });

  test('toggle on dashboard persists across navigation', async ({ page }) => {
    await login(page);

    const initialBg = await page.evaluate(() => document.body.style.background);

    const toggleBtn = page.getByTitle(/Switch to dark mode|Switch to light mode/);
    await toggleBtn.click();
    await page.waitForTimeout(300);

    const newBg = await page.evaluate(() => document.body.style.background);
    expect(newBg).not.toBe(initialBg);

    // Navigate — theme should persist
    await page.getByRole('menu').getByText('Users', { exact: true }).click();
    await page.waitForTimeout(300);

    const afterNavBg = await page.evaluate(() => document.body.style.background);
    expect(afterNavBg).toBe(newBg);
  });

  test('theme preference persists across page refresh', async ({ page }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.setItem('thairag-theme', 'dark'));
    await page.reload();
    await page.waitForTimeout(500);

    const bg = await page.evaluate(() => document.body.style.background);
    expect(bg).toContain('20'); // #141414 contains "20" in decimal
  });
});

// ─── Error Handling ───────────────────────────────────────────────────

test.describe('Error Handling', () => {
  test('API error on login shows human-readable message', async ({ page }) => {
    await page.goto('/login');
    await page.getByPlaceholder('Email').fill('nonexistent@test.com');
    await page.getByPlaceholder('Password').fill('wrongpass');
    await page.getByRole('button', { name: 'Sign In' }).click();

    const errorMsg = page.locator('.ant-message-error');
    await expect(errorMsg).toBeVisible({ timeout: 5000 });
    const text = await errorMsg.textContent();
    expect(text).not.toContain('[object Object]');
    expect(text).toBeTruthy();
  });
});

// ─── Sidebar Collapse ─────────────────────────────────────────────────

test.describe('Sidebar', () => {
  test('sidebar collapses and expands', async ({ page }) => {
    await login(page);

    await expect(page.getByText('ThaiRAG Admin')).toBeVisible();

    const trigger = page.locator('.ant-layout-sider-trigger');
    await trigger.click();
    await page.waitForTimeout(500);

    await expect(page.getByText('TR')).toBeVisible();

    await trigger.click();
    await page.waitForTimeout(500);
    await expect(page.getByText('ThaiRAG Admin')).toBeVisible();
  });
});
