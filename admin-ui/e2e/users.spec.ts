import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

test.describe('Users page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Users');
  });

  test('shows users table with columns', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'User Management' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Email' })).toBeVisible();
  });

  test('test user is visible in table', async ({ page }) => {
    await expect(page.getByRole('cell', { name: 'playwright@test.com' })).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByRole('cell', { name: 'Playwright Test User' })).toBeVisible();
  });
});
