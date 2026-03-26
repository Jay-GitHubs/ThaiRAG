import { test as setup } from '@playwright/test';

const API = 'http://localhost:8080';

const ADMIN_EMAIL = 'admin@thairag.local';
const ADMIN_PASSWORD = 'Admin123';

const users = [
  { email: 'playwright@test.com', name: 'Playwright Test User', password: 'Test1234!' },
  { email: 'playwright2@test.com', name: 'Playwright Second User', password: 'Test1234!' },
];

setup('register test users and promote to super_admin', async ({ request }) => {
  // Register test users
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

  // Login as seeded admin to promote test users
  const loginRes = await request.post(`${API}/api/auth/login`, {
    data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
  });
  if (!loginRes.ok()) {
    // Admin might not be seeded (no env vars) — skip promotion
    console.warn('Could not login as seeded admin; test users keep default role');
    return;
  }
  const { token } = await loginRes.json();
  const headers = { Authorization: `Bearer ${token}` };

  // Get user list to find test user IDs
  const usersRes = await request.get(`${API}/api/km/users`, { headers });
  if (!usersRes.ok()) return;
  const { data: userList } = await usersRes.json();

  // Promote each test user to super_admin
  for (const testUser of users) {
    const found = userList.find((u: { email: string }) => u.email === testUser.email);
    if (!found) continue;
    const roleRes = await request.put(`${API}/api/km/users/${found.id}/role`, {
      headers,
      data: { role: 'super_admin' },
    });
    if (roleRes.ok()) {
      console.log(`Promoted ${testUser.email} to super_admin`);
    }
  }
});
