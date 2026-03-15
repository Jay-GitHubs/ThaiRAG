import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Dashboard', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('shows dashboard heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
  });

  test('shows stat cards', async ({ page }) => {
    // Scope to main content area to avoid matching sidebar menu items
    const main = page.getByRole('main');
    await expect(main.getByText('Organizations')).toBeVisible();
    await expect(main.getByText('Users')).toBeVisible();
    await expect(main.getByText('Health Status')).toBeVisible();
  });

  test('shows health status badge', async ({ page }) => {
    const healthCard = page.locator('.ant-card', { hasText: 'Health Status' });
    await expect(healthCard).toBeVisible();
    await expect(healthCard.locator('.ant-badge')).toBeVisible({ timeout: 10_000 });
  });
});
