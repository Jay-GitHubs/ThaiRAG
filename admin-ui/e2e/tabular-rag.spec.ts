import { test, expect, type APIRequestContext } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * End-to-end tabular RAG proof.
 *
 * Ingests the bilingual Thai/English "Prohibited Business & Cautions" table PDF
 * into a fresh workspace with a per-org `max_chunk_size` override that keeps the
 * whole table in a single atomic chunk. Then it drives the Test Chat UI to ask a
 * table-specific question and asserts that:
 *   1. the table chunk was retrieved from the vector store, and
 *   2. that retrieved chunk literally contains a distinctive table cell token
 *      ("KYC/CDD", from the money-laundering row), and
 *   3. a non-empty answer was generated.
 * Together this proves the tabular content was embedded into the vector DB and
 * is retrievable through the full RAG pipeline (search → rerank → generate).
 */
const PDF_PATH = path.resolve(
  __dirname,
  '../../tests/fixtures/micro_sme_prohibited_business.pdf',
);

// Distinctive cell from the money-laundering row — ASCII so it is stable to
// match in the DOM regardless of Thai font/encoding round-tripping.
const TABLE_TOKEN = 'KYC/CDD';

interface DocState {
  chunkCount: number;
  updatedAt: string;
}

// Poll the documents list until the target doc leaves the 'processing' state.
async function waitForReady(
  request: APIRequestContext,
  token: string,
  wsId: string,
  docId: string,
  timeoutMs = 120_000,
): Promise<DocState> {
  const headers = { Authorization: `Bearer ${token}` };
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const res = await request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/documents`,
      { headers },
    );
    if (res.ok()) {
      const { data } = await res.json();
      const doc = data.find((d: { id: string }) => d.id === docId);
      if (doc && doc.status !== 'processing') {
        if (doc.status === 'failed') {
          throw new Error(`Document processing failed: ${doc.error_message ?? 'unknown'}`);
        }
        return { chunkCount: doc.chunk_count as number, updatedAt: doc.updated_at as string };
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error('Timed out waiting for document to finish processing');
}

test.describe('Tabular content is embedded in the vector DB and retrievable (RAG)', () => {
  const suffix = Date.now();
  const orgName = `TableRagOrg-${suffix}`;
  const deptName = `TableRagDept-${suffix}`;
  const wsName = `TableRagWS-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;
  let docId: string;
  let originalAiEnabled = false;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    token = (await loginRes.json()).token;
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

    // Disable AI preprocessing globally for deterministic, fast non-AI chunking
    // (the AI path runs Ollama and takes minutes per pass). Restored in afterAll.
    const cfgRes = await request.get(`${API_BASE}/api/km/settings/document`, { headers });
    originalAiEnabled = (await cfgRes.json()).ai_preprocessing.enabled;
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: false } },
      headers,
    });

    // Raise max_chunk_size at org scope so the whole table stays in one atomic
    // chunk. The workspace inherits it (workspace → dept → org → global).
    await request.put(
      `${API_BASE}/api/km/settings/document?scope_type=org&scope_id=${orgId}`,
      { data: { max_chunk_size: 8000 }, headers },
    );
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(
      `${API_BASE}/api/km/settings/scoped?scope_type=org&scope_id=${orgId}`,
      { headers },
    );
    await request.delete(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`,
      { headers },
    );
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: originalAiEnabled } },
      headers,
    });
  });

  test('asking about a table row retrieves the atomic table chunk from the vector DB', async ({ page }) => {
    test.setTimeout(360_000);

    // ── 1. Upload the table PDF through the UI ─────────────────────────────
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    await page.getByTitle(orgName).click();
    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    await page.getByTitle(deptName).click();
    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    await page.getByTitle(wsName).click();

    await expect(page.getByRole('button', { name: 'Upload File' })).toBeVisible({ timeout: 5000 });
    await page.getByRole('button', { name: 'Upload File' }).click();

    const modal = page.locator('.ant-modal', { hasText: 'Upload Document' });
    await expect(modal).toBeVisible();
    await modal.locator('input[type="file"]').setInputFiles(PDF_PATH);
    await modal.getByRole('button', { name: 'Upload' }).click();
    // Upload now keeps the modal open as a live processing tracker; dismiss it.
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(modal).not.toBeVisible({ timeout: 15_000 });

    const listRes = await page.request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/documents`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    const list = (await listRes.json()).data as { id: string; title: string }[];
    const doc = list.find((d) => d.title.includes('micro_sme_prohibited_business'));
    expect(doc, 'uploaded PDF should appear in the workspace').toBeTruthy();
    docId = doc!.id;

    // ── 2. Confirm the table was ingested as a single atomic chunk ─────────
    const ready = await waitForReady(page.request, token, wsId, docId);
    console.log(`[tabular-rag] chunk_count=${ready.chunkCount}`);
    expect(ready.chunkCount, 'override should keep the table in one atomic chunk').toBe(1);

    const chunksRes = await page.request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}/chunks`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    const chunksBody = await chunksRes.json();
    expect(chunksBody.total).toBe(1);
    expect(
      chunksBody.chunks[0].text as string,
      'the atomic chunk should contain the money-laundering row token',
    ).toContain(TABLE_TOKEN);

    // ── 3. Ask a table-specific question through the Test Chat UI ──────────
    await navigateTo(page, 'Test Chat');
    await expect(page.getByRole('heading', { name: 'Test KM Chat' })).toBeVisible();

    const wsSelects = page.locator('[data-tour="chat-ws-select"] .ant-select');
    await wsSelects.nth(0).click();
    await page.getByTitle(orgName).click();
    await wsSelects.nth(1).click();
    await page.getByTitle(deptName).click();
    await wsSelects.nth(2).click();
    await page.getByTitle(wsName).click();

    const input = page.locator('[data-tour="chat-input"] textarea, textarea[data-tour="chat-input"]').first();
    await input.fill(
      'According to the table, what cautions apply to money laundering businesses?',
    );
    await page.locator('[data-tour="chat-send"]').click();

    // ── 4. Wait for the streamed answer + retrieved chunks ─────────────────
    // Inference is slow (~60–120s); allow a generous window.
    const chunksLabel = page.getByText(/\d+ chunks? retrieved/);
    await expect(chunksLabel).toBeVisible({ timeout: 300_000 });

    // Expand the retrieved-chunks panel and assert the table chunk is present.
    await chunksLabel.click();
    const retrievedToken = page.getByText(new RegExp(TABLE_TOKEN.replace('/', '\\/')));
    await expect(
      retrievedToken.first(),
      'a retrieved chunk must contain the table cell token, proving it came from the vector DB',
    ).toBeVisible({ timeout: 10_000 });

    // A non-empty answer must have been generated (completion signal: Total timing tag).
    await expect(page.getByText(/Total: \d+ms/).first()).toBeVisible({ timeout: 10_000 });

    console.log('[tabular-rag] retrieved table chunk and generated an answer — end-to-end OK');
  });
});
