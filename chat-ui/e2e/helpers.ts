import { type Page, expect } from '@playwright/test';

export const TEST_EMAIL = 'playwright@test.com';
export const TEST_PASSWORD = 'Test1234!';
export const API_BASE = process.env.E2E_API_BASE ?? 'http://localhost:8080';

/** Log in through the UI and land on the chat page. */
export async function login(page: Page) {
  await page.goto('/login');
  await page.getByPlaceholder('Email').fill(TEST_EMAIL);
  await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  // The chat page is the only one with a "New chat" button — its presence is the
  // reliable post-login signal (more robust than matching the root URL).
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible({ timeout: 15_000 });
}
