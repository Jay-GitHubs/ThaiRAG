import { type Page, expect } from '@playwright/test';

export const TEST_EMAIL = 'playwright@test.com';
export const TEST_PASSWORD = 'Test1234!';
export const API_BASE = 'http://localhost:8080';

/** Login via the UI and wait for dashboard to load. */
export async function login(page: Page) {
  await page.goto('/login');
  await page.getByPlaceholder('Email').fill(TEST_EMAIL);
  await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
  await page.getByRole('button', { name: 'Sign In' }).click();
  await page.waitForURL('/', { timeout: 10_000 });
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
}

/**
 * Navigate to a page via the sidebar menu (client-side navigation).
 * This avoids full page reload which would lose the in-memory auth token.
 */
export async function navigateTo(page: Page, menuLabel: string) {
  await page.getByRole('menu').getByText(menuLabel, { exact: true }).click();
}
