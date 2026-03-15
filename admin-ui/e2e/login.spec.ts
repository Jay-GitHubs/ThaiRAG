import { test, expect } from '@playwright/test';
import { TEST_EMAIL, TEST_PASSWORD } from './helpers';

test.describe('Login page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/login');
  });

  test('shows login form elements', async ({ page }) => {
    await expect(page.getByText('ThaiRAG Admin')).toBeVisible();
    await expect(page.getByPlaceholder('Email')).toBeVisible();
    await expect(page.getByPlaceholder('Password')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Sign In' })).toBeVisible();
  });

  test('invalid credentials do not log in', async ({ page }) => {
    await page.getByPlaceholder('Email').fill('wrong@test.com');
    await page.getByPlaceholder('Password').fill('wrongpassword');
    await page.getByRole('button', { name: 'Sign In' }).click();

    // Should remain on login page — not redirect to dashboard
    await page.waitForTimeout(2000);
    await expect(page).toHaveURL(/\/login/);
    await expect(page.getByRole('button', { name: 'Sign In' })).toBeVisible();
  });

  test('successful login redirects to dashboard', async ({ page }) => {
    await page.getByPlaceholder('Email').fill(TEST_EMAIL);
    await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
    await page.getByRole('button', { name: 'Sign In' }).click();

    await page.waitForURL('/', { timeout: 10_000 });
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
    await expect(page.getByText(`Logged in as ${TEST_EMAIL}`)).toBeVisible();
  });

  test('logout returns to login page', async ({ page }) => {
    await page.getByPlaceholder('Email').fill(TEST_EMAIL);
    await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
    await page.getByRole('button', { name: 'Sign In' }).click();
    await page.waitForURL('/', { timeout: 10_000 });

    await page.getByRole('button', { name: 'Logout' }).click();
    await page.waitForURL('/login', { timeout: 5000 });
    await expect(page.getByText('ThaiRAG Admin')).toBeVisible();
  });
});
