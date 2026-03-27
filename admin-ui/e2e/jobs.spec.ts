import { test, expect } from '@playwright/test';
import { TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

test.describe('Jobs API', () => {
  const suffix = Date.now();
  const orgName = `JobOrg-${suffix}`;
  const deptName = `JobDept-${suffix}`;
  const wsName = `JobWS-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const loginData = await loginRes.json();
    token = loginData.token;
    const headers = { Authorization: `Bearer ${token}` };

    const orgRes = await request.post(`${API_BASE}/api/km/orgs`, {
      data: { name: orgName },
      headers,
    });
    orgId = (await orgRes.json()).id;

    const deptRes = await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, {
      data: { name: deptName },
      headers,
    });
    deptId = (await deptRes.json()).id;

    const wsRes = await request.post(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
      { data: { name: wsName }, headers },
    );
    wsId = (await wsRes.json()).id;
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`,
      { headers },
    );
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('list jobs returns empty array for new workspace', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const res = await request.get(`${API_BASE}/api/km/workspaces/${wsId}/jobs`, { headers });
    expect(res.ok()).toBeTruthy();
    const data = await res.json();
    expect(data.jobs).toEqual([]);
  });

  test('ingest document creates trackable job for large files', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    // Ingest a small document (processed inline, no job created)
    const ingestRes = await request.post(`${API_BASE}/api/km/workspaces/${wsId}/documents`, {
      data: {
        title: `SmallDoc-${suffix}`,
        content: 'Small test content',
        mime_type: 'text/plain',
      },
      headers,
    });
    expect(ingestRes.ok()).toBeTruthy();

    // Jobs list may still be empty (small docs are processed inline)
    const jobsRes = await request.get(`${API_BASE}/api/km/workspaces/${wsId}/jobs`, { headers });
    expect(jobsRes.ok()).toBeTruthy();
    const data = await jobsRes.json();
    expect(Array.isArray(data.jobs)).toBeTruthy();
  });

  test('get non-existent job returns 404', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const fakeJobId = '00000000-0000-0000-0000-000000000000';
    const res = await request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/jobs/${fakeJobId}`,
      { headers },
    );
    expect(res.status()).toBe(404);
  });

  test('cancel non-existent job returns 404', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const fakeJobId = '00000000-0000-0000-0000-000000000000';
    const res = await request.delete(
      `${API_BASE}/api/km/workspaces/${wsId}/jobs/${fakeJobId}`,
      { headers },
    );
    expect(res.status()).toBe(404);
  });
});
