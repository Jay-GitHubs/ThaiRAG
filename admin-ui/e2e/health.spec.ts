import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

test.describe('Health page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Health');
  });

  test('shows System Health heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'System Health' })).toBeVisible();
  });

  test('shows health status', async ({ page }) => {
    await expect(page.getByText('Status', { exact: true })).toBeVisible();
    await expect(page.getByText('ok', { exact: true })).toBeVisible({ timeout: 10_000 });
  });

  test('has Run Deep Check button', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Run Deep Check' })).toBeVisible();
  });

  test('shows Prometheus metrics', async ({ page }) => {
    await expect(page.getByText('Prometheus Metrics')).toBeVisible();
    // Metrics render in a themed <pre> code panel.
    const metricsBlock = page.locator('pre').last();
    await expect(metricsBlock).toBeVisible({ timeout: 10_000 });
    // Wait until actual metrics load (not just the loading placeholder).
    await expect(metricsBlock).toContainText('active_sessions_total', { timeout: 10_000 });
  });

  test('deep check works', async ({ page }) => {
    await page.getByRole('button', { name: 'Run Deep Check' }).click();
    // After deep check, the "Back to Shallow" button should appear
    await expect(page.getByRole('button', { name: 'Back to Shallow' })).toBeVisible({
      timeout: 15_000,
    });
  });
});
