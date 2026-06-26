import { test, expect } from '@playwright/test';
import { login } from './helpers';

test('unauthenticated user is redirected to login', async ({ page }) => {
  await page.goto('/');
  await page.waitForURL('**/login', { timeout: 15_000 });
  await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
});

test('user can log in and reach the chat UI', async ({ page }) => {
  await login(page);
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();
  await expect(page.getByPlaceholder('Ask anything about your documents…')).toBeVisible();
});
