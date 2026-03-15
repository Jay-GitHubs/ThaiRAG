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

  test('shows health status badge', async ({ page }) => {
    await expect(page.getByText('Status', { exact: true })).toBeVisible();
    await expect(page.locator('.ant-badge')).toBeVisible({ timeout: 10_000 });
  });

  test('has Run Deep Check button', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Run Deep Check' })).toBeVisible();
  });

  test('shows Prometheus metrics', async ({ page }) => {
    await expect(page.getByText('Prometheus Metrics')).toBeVisible();
    // Metrics rendered as <Typography.Text code> → <code> element
    const metricsBlock = page.locator('code');
    await expect(metricsBlock).toBeVisible({ timeout: 10_000 });
    // Wait until actual metrics load (not just "Loading...")
    await expect(metricsBlock).not.toHaveText('Loading...', { timeout: 10_000 });
    const text = await metricsBlock.textContent();
    expect(text?.length).toBeGreaterThan(0);
  });

  test('deep check works', async ({ page }) => {
    await page.getByRole('button', { name: 'Run Deep Check' }).click();
    // After deep check, the "Back to Shallow" button should appear
    await expect(page.getByRole('button', { name: 'Back to Shallow' })).toBeVisible({
      timeout: 15_000,
    });
  });
});
