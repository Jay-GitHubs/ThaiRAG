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
 * End-to-end RAG proof for the BORDERLESS (whitespace-stream) table path.
 *
 * Ingests the borderless table PDF (columns separated only by whitespace, no
 * ruling lines) into a fresh workspace, then drives the Test Chat UI to ask a
 * row-specific question and asserts that:
 *   1. a chunk containing a distinctive table cell ("Northeast") was retrieved
 *      from the vector store (proves the reconstructed table was embedded), and
 *   2. the generated answer carries the actual table figures (1100 / 1200) —
 *      i.e. the deterministic numbers flowed all the way through to the answer.
 */
const PDF_PATH = path.resolve(__dirname, '../../tests/fixtures/borderless_table.pdf');

// Distinctive cell from the Northeast row: Q1=1100, Q2=1200. ASCII-stable.
const ROW_LABEL = 'Northeast';
const Q1 = '1100';
const Q2 = '1200';

async function waitForReady(
  request: APIRequestContext,
  token: string,
  wsId: string,
  docId: string,
  timeoutMs = 120_000,
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
          throw new Error(`Document processing failed: ${doc.error_message ?? 'unknown'}`);
        }
        return doc.chunk_count as number;
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error('Timed out waiting for document to finish processing');
}

test.describe('Borderless table is embedded and answerable through RAG', () => {
  const suffix = Date.now();
  const orgName = `BorderlessRagOrg-${suffix}`;
  const deptName = `BorderlessRagDept-${suffix}`;
  const wsName = `BorderlessRagWS-${suffix}`;

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
        await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, {
          data: { name: deptName },
          headers,
        })
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

    // Pin a known-pulled chat model so this spec is independent of suite
    // ordering (an earlier spec can leave a leaked/unpulled model selected,
    // which would 404 the chat call). Restored in afterAll.
    prevModel = await pinSharedModel(request, token);

    // Disable AI preprocessing for deterministic, fast chunking (the table chunk
    // is produced by the deterministic smart-PDF path regardless). Restored after.
    const cfgRes = await request.get(`${API_BASE}/api/km/settings/document`, { headers });
    originalAiEnabled = (await cfgRes.json()).ai_preprocessing.enabled;
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: false } },
      headers,
    });
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, {
      headers,
    });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: originalAiEnabled } },
      headers,
    });
    if (prevModel) await setSharedModel(request, token, prevModel);
  });

  test('asking about a borderless table row retrieves its chunk and answers with the figures', async ({
    page,
  }) => {
    test.setTimeout(360_000);

    // ── 1. Upload the borderless PDF through the UI ────────────────────────
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(orgName).slice(0, 18));
    await page.getByTitle(orgName).click();
    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(deptName).slice(0, 18));
    await page.getByTitle(deptName).click();
    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(wsName).slice(0, 18));
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
    const doc = list.find((d) => d.title.includes('borderless_table'));
    expect(doc, 'uploaded PDF should appear in the workspace').toBeTruthy();
    docId = doc!.id;

    // ── 2. Confirm a table chunk carrying the row token was embedded ───────
    const chunkCount = await waitForReady(page.request, token, wsId, docId);
    expect(chunkCount, 'borderless table should produce at least one chunk').toBeGreaterThan(0);

    const chunks = (
      await (
        await page.request.get(
          `${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}/chunks`,
          { headers: { Authorization: `Bearer ${token}` } },
        )
      ).json()
    ).chunks as { text: string }[];
    const tableChunk = chunks.find((c) => c.text.includes(ROW_LABEL) && c.text.includes(Q1));
    expect(tableChunk, 'a chunk should contain the Northeast row with its Q1 figure').toBeTruthy();

    // ── 3. Ask a row-specific question through the Test Chat UI ────────────
    await navigateTo(page, 'Test Chat');
    await expect(page.getByRole('heading', { name: 'Test KM Chat' })).toBeVisible();

    const wsSelects = page.locator('[data-tour="chat-ws-select"] .ant-select');
    await wsSelects.nth(0).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(orgName).slice(0, 18));
    await page.getByTitle(orgName).click();
    await wsSelects.nth(1).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(deptName).slice(0, 18));
    await page.getByTitle(deptName).click();
    await wsSelects.nth(2).click();
    // Type-to-filter: dropdown virtualizes once many orgs exist.
    await page.keyboard.type(String(wsName).slice(0, 18));
    await page.getByTitle(wsName).click();

    const input = page
      .locator('[data-tour="chat-input"] textarea, textarea[data-tour="chat-input"]')
      .first();
    await input.fill('According to the table, what were the Q1 and Q2 sales for the Northeast region?');
    await page.locator('[data-tour="chat-send"]').click();

    // ── 4. Wait for retrieval + answer, assert table data flowed through ───
    const chunksLabel = page.getByText(/\d+ chunks? retrieved/);
    await expect(chunksLabel).toBeVisible({ timeout: 300_000 });

    // The actual table figures appear in the ANSWER itself. Scope to the
    // assistant message card (the last message card) BEFORE expanding the
    // retrieved-chunks panel — otherwise a match could come from the panel,
    // not the answer.
    const answerCard = page.locator('[data-tour="chat-response"] .ant-card').last();
    await expect(
      answerCard.getByText(new RegExp(`${Q1}|${Q2}`)).first(),
      'the answer should carry the deterministic table figures',
    ).toBeVisible({ timeout: 15_000 });

    // And the reconstructed table chunk was retrieved from the vector DB.
    await chunksLabel.click();
    await expect(
      page.getByText(ROW_LABEL).first(),
      'a retrieved chunk must contain the table row token',
    ).toBeVisible({ timeout: 10_000 });

    await page.screenshot({ path: 'e2e/screenshots/borderless-table-rag-answer.png' });
  });
});
