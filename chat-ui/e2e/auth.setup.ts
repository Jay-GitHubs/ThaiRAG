import { test as setup, expect } from '@playwright/test';
import { API_BASE, TEST_EMAIL, TEST_PASSWORD } from './helpers';

// Ensure the chat test user exists on the backend. Idempotent: a 400/409 means
// the user already exists (e.g. created by the admin-ui suite on the same stack).
setup('register chat test user', async ({ request }) => {
  const res = await request.post(`${API_BASE}/api/auth/register`, {
    data: { email: TEST_EMAIL, name: 'Playwright Chat User', password: TEST_PASSWORD },
  });
  if (!res.ok() && res.status() !== 409 && res.status() !== 400) {
    throw new Error(`Failed to register ${TEST_EMAIL}: ${res.status()} ${await res.text()}`);
  }

  // Sanity: the user can authenticate.
  const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
    data: { email: TEST_EMAIL, password: TEST_PASSWORD },
  });
  expect(loginRes.ok()).toBeTruthy();
});
