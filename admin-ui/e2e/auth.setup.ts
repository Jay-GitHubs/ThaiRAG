import { test as setup } from '@playwright/test';

const API = 'http://localhost:8080';

const users = [
  { email: 'playwright@test.com', name: 'Playwright Test User', password: 'Test1234!' },
  { email: 'playwright2@test.com', name: 'Playwright Second User', password: 'Test1234!' },
];

setup('register test users', async ({ request }) => {
  for (const user of users) {
    const res = await request.post(`${API}/api/auth/register`, {
      data: { email: user.email, name: user.name, password: user.password },
    });
    // 200/201 = created, 400/409 = already exists — both fine
    if (!res.ok() && res.status() !== 409 && res.status() !== 400) {
      const body = await res.text();
      throw new Error(`Failed to register ${user.email}: ${res.status()} ${body}`);
    }
  }
});
