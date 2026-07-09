import { test, expect, type APIRequestContext } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import {
  login,
  navigateTo,
  TEST_EMAIL,
  TEST_PASSWORD,
  API_BASE,
  pinSharedModel,
  setSharedModel,
} from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * End-to-end proof of merged-cell answering (the dense-grid-to-LLM change).
 *
 * The real Revenue Department withholding-tax PDF has merged-cell tables. Its
 * reconstructed chunks are now stored as a dense markdown grid (merged values
 * repeated across their span) rather than colspan/rowspan HTML, so the answer
 * LLM can actually read across merges. This drives the Test Chat UI to ask a
 * question whose answer lives in merged cells (which form → which submission
 * deadline) and asserts the answer carries both.
 */
const PDF_PATH = path.resolve(__dirname, '../../tests/fixtures/thai-real/rd_withholding_table.pdf');
const QUESTION =
  'ตามตาราง แบบ ภ.ง.ด. ที่ใช้ในการนำส่งภาษีหัก ณ ที่จ่ายมีแบบใดบ้าง และต้องนำส่งภายในวันที่เท่าไร';

async function waitForReady(
  request: APIRequestContext,
  token: string,
  wsId: string,
  docId: string,
  // Gateway-era AI preprocessing: the 10-page withholding PDF measured ~7
  // minutes for a full pass (2026-07-08) — budget well above that.
  timeoutMs = 900_000,
): Promise<number> {
  const headers = { Authorization: `Bearer ${token}` };
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const res = await request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, { headers });
    if (res.ok()) {
      const { data } = await res.json();
      const doc = data.find((d: { id: string }) => d.id === docId);
      if (doc && doc.status !== 'processing') {
        if (doc.status === 'failed') {
          throw new Error(`processing failed: ${doc.error_message ?? '?'}`);
        }
        return doc.chunk_count as number;
      }
    }
    await new Promise((r) => setTimeout(r, 1500));
  }
  throw new Error('Timed out waiting for document');
}

test.describe('Merged-cell table is answerable through RAG (dense grid)', () => {
  const suffix = Date.now();
  const orgName = `MergedRagOrg-${suffix}`;
  const deptName = `MergedRagDept-${suffix}`;
  const wsName = `MergedRagWS-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;
  let docId: string;
  let originalAiEnabled = false;
  let prevModel: string | undefined;

  test.beforeAll(async ({ request }) => {
    token = (
      await (
        await request.post(`${API_BASE}/api/auth/login`, {
          data: { email: TEST_EMAIL, password: TEST_PASSWORD },
        })
      ).json()
    ).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (
      await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()
    ).id;
    deptId = (
      await (
        await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })
      ).json()
    ).id;
    wsId = (
      await (
        await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, {
          data: { name: wsName },
          headers,
        })
      ).json()
    ).id;

    prevModel = await pinSharedModel(request, token);
    const cfg = await (await request.get(`${API_BASE}/api/km/settings/document`, { headers })).json();
    originalAiEnabled = cfg.ai_preprocessing.enabled;
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: false } },
      headers,
    });
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: originalAiEnabled } },
      headers,
    });
    if (prevModel) await setSharedModel(request, token, prevModel);
  });

  test('answer carries merged-cell values (form + deadline)', async ({ page }) => {
    test.setTimeout(1_500_000);

    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();
    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    await page.getByTitle(orgName).click();
    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    await page.getByTitle(deptName).click();
    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    await page.getByTitle(wsName).click();

    await page.getByRole('button', { name: 'Upload File' }).click();
    const modal = page.locator('.ant-modal', { hasText: 'Upload Document' });
    await expect(modal).toBeVisible();
    await modal.locator('input[type="file"]').setInputFiles(PDF_PATH);
    await modal.getByRole('button', { name: 'Upload' }).click();
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(modal).not.toBeVisible({ timeout: 15_000 });

    const list = (
      await (
        await page.request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, {
          headers: { Authorization: `Bearer ${token}` },
        })
      ).json()
    ).data as { id: string; title: string }[];
    docId = list.find((d) => d.title.includes('rd_withholding_table'))!.id;
    const chunkCount = await waitForReady(page.request, token, wsId, docId);
    expect(chunkCount, 'tax PDF should reconstruct into table chunks').toBeGreaterThan(5);

    // A reconstructed table chunk must be a dense markdown grid, not HTML.
    const chunks = (
      await (
        await page.request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}/chunks`, {
          headers: { Authorization: `Bearer ${token}` },
        })
      ).json()
    ).chunks as { text: string }[];
    const md = chunks.find((c) => c.text.trimStart().startsWith('|'));
    expect(md, 'a chunk should be a markdown table').toBeTruthy();
    expect(chunks.some((c) => c.text.includes('<table')), 'no chunk should be raw HTML').toBeFalsy();

    // Ask a question whose answer spans merged cells (form ↔ deadline).
    await navigateTo(page, 'Test Chat');
    await expect(page.getByRole('heading', { name: 'Test KM Chat' })).toBeVisible();
    const wsSelects = page.locator('[data-tour="chat-ws-select"] .ant-select');
    await wsSelects.nth(0).click();
    await page.getByTitle(orgName).click();
    await wsSelects.nth(1).click();
    await page.getByTitle(deptName).click();
    await wsSelects.nth(2).click();
    await page.getByTitle(wsName).click();

    const input = page
      .locator('[data-tour="chat-input"] textarea, textarea[data-tour="chat-input"]')
      .first();
    await input.fill(QUESTION);
    await page.locator('[data-tour="chat-send"]').click();

    await expect(page.getByText(/\d+ chunks? retrieved/)).toBeVisible({ timeout: 300_000 });

    // Assert on the answer card (before expanding the chunk panel) so a match
    // can't come from the retrieved-chunks text.
    const answer = page.locator('[data-tour="chat-response"] .ant-card').last();
    await expect(answer.getByText(/ภ\.ง\.ด/).first(), 'answer should name the tax forms from the table').toBeVisible({
      timeout: 15_000,
    });
    await expect(
      answer.getByText(/1\s*[-–]\s*7|7 วัน|7 ของเดือน/).first(),
      'answer should carry the merged-cell submission deadline',
    ).toBeVisible({ timeout: 15_000 });

    await page.screenshot({ path: 'e2e/screenshots/merged-cell-rag-answer.png' });
  });
});
