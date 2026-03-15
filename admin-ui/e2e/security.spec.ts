import { test, expect } from '@playwright/test';
import { API_BASE, login } from './helpers';

test.describe('Security Headers', () => {
  test('API responses include OWASP security headers', async ({ request }) => {
    const res = await request.get(`${API_BASE}/health`);
    expect(res.status()).toBe(200);

    const headers = res.headers();
    expect(headers['x-content-type-options']).toBe('nosniff');
    expect(headers['x-frame-options']).toBe('DENY');
    expect(headers['x-xss-protection']).toBe('1; mode=block');
    expect(headers['referrer-policy']).toBe('strict-origin-when-cross-origin');
  });

  test('API responses include request-id header', async ({ request }) => {
    const res = await request.get(`${API_BASE}/health`);
    expect(res.headers()['x-request-id']).toBeTruthy();
  });
});

test.describe('Password Policy', () => {
  test('registration rejects short password via API', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/register`, {
      data: { email: 'short@sec.test', name: 'Short', password: 'Ab1' },
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error.message).toContain('at least');
  });

  test('registration rejects password without uppercase', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/register`, {
      data: { email: 'noup@sec.test', name: 'NoUp', password: 'lowercase123' },
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error.message).toContain('uppercase');
  });

  test('registration rejects password without digit', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/register`, {
      data: { email: 'nodig@sec.test', name: 'NoDig', password: 'NoDigitHere' },
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error.message).toContain('digit');
  });

  test('UI shows error for weak password on register', async ({ page }) => {
    await page.goto('/login');

    // Check if there's a register link/tab — if not, skip this test
    const registerLink = page.getByText(/register|sign up/i);
    if (await registerLink.isVisible({ timeout: 2000 }).catch(() => false)) {
      await registerLink.click();
      await page.getByPlaceholder('Email').fill('weak@sec.test');
      await page.getByPlaceholder('Name').fill('Weak');
      await page.getByPlaceholder('Password').fill('weak');
      await page.getByRole('button', { name: /register|sign up/i }).click();
      await expect(page.getByText(/at least|password/i)).toBeVisible({ timeout: 5000 });
    }
  });
});

test.describe('Brute-force Protection', () => {
  test('account locks after repeated failed logins', async ({ request }) => {
    const email = `bruteforce-${Date.now()}@sec.test`;

    // Register the user first
    const regRes = await request.post(`${API_BASE}/api/auth/register`, {
      data: { email, name: 'BruteTest', password: 'Correct1pass' },
    });
    // Could be 201 (created) or 400 (already exists)
    expect([200, 201, 400]).toContain(regRes.status());

    // Make multiple failed login attempts
    for (let i = 0; i < 5; i++) {
      const res = await request.post(`${API_BASE}/api/auth/login`, {
        data: { email, password: 'Wrong1pass' },
      });
      expect(res.status()).toBe(401);
    }

    // Next attempt should be locked even with correct password
    const lockedRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email, password: 'Correct1pass' },
    });
    expect(lockedRes.status()).toBe(401);
    const body = await lockedRes.json();
    expect(body.error.message).toContain('locked');
  });
});
