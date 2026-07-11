import { test, expect, type APIRequestContext } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Per-workspace chunk-size override (Tier 1).
 *
 * Proves that a per-scope `document.max_chunk_size` override actually governs
 * chunking: a ~5.8k-char prose document splits into many fragments at the
 * default size, but raising the size at org scope (inherited by the workspace)
 * collapses it to a single chunk on reprocess.
 *
 * Uses prose rather than a table on purpose — tables are reconstructed into one
 * atomic chunk per page regardless of chunk size (deterministic table
 * extraction), so a table can no longer demonstrate size-driven splitting.
 */
const PROSE_PATH = path.resolve(__dirname, '../../tests/fixtures/long_prose.md');

// Poll the documents list until the target doc leaves the 'processing' state.
interface DocState {
  chunkCount: number;
  updatedAt: string;
}

// Poll the documents list until the target doc leaves the 'processing' state.
// When `sinceUpdatedAt` is provided, also wait until `updated_at` advances past
// it — this avoids a race on reprocess, where the non-AI pipeline finishes in
// ~1s and a naive poll could read the stale (pre-reprocess) 'ready' state.
async function waitForReady(
  request: APIRequestContext,
  token: string,
  wsId: string,
  docId: string,
  sinceUpdatedAt?: string,
  // Gateway-era AI preprocessing takes minutes per pass (measured).
  timeoutMs = 900_000,
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
        if (!sinceUpdatedAt || doc.updated_at !== sinceUpdatedAt) {
          return { chunkCount: doc.chunk_count as number, updatedAt: doc.updated_at as string };
        }
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error('Timed out waiting for document to finish processing');
}

test.describe('Per-scope chunk size (Tier 1)', () => {
  const suffix = Date.now();
  const orgName = `ChunkOrg-${suffix}`;
  const deptName = `ChunkDept-${suffix}`;
  const wsName = `ChunkWS-${suffix}`;

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

    // Disable AI preprocessing globally for the duration of this test. The AI
    // path runs Ollama (orchestrator + optional vision + per-chunk enrichment),
    // which takes 2–4 min per pass and is highly variable — too slow/flaky for
    // a deterministic chunk-size assertion. With AI off the chunker uses the
    // raw max_chunk_size, so the table split is fast and reproducible. The
    // original value is restored in afterAll.
    const cfgRes = await request.get(`${API_BASE}/api/km/settings/document`, { headers });
    originalAiEnabled = (await cfgRes.json()).ai_preprocessing.enabled;
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: false } },
      headers,
    });
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    // Drop the org-scoped override so we don't leak state.
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
    // Restore the global AI preprocessing toggle to its original state.
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: originalAiEnabled } },
      headers,
    });
  });

  test('raising per-workspace max_chunk_size collapses a prose doc to one chunk', async ({ page }) => {
    // Two full processing passes (upload + reprocess) over the gateway.
    test.setTimeout(1_800_000);

    // ── 1. Upload the long prose document through the UI ───────────────────
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

    await expect(page.getByRole('button', { name: 'Upload File' })).toBeVisible({ timeout: 5000 });
    await page.getByRole('button', { name: 'Upload File' }).click();

    const modal = page.locator('.ant-modal', { hasText: 'Upload Document' });
    await expect(modal).toBeVisible();
    await modal.locator('input[type="file"]').setInputFiles(PROSE_PATH);
    await modal.getByRole('button', { name: 'Upload' }).click();
    // Upload now keeps the modal open as a live processing tracker; dismiss it.
    // 'Done' renders once the upload POST returns — which itself can take
    // >10s when the ingestion queue is busy.
    await page.getByRole('button', { name: 'Done' }).click({ timeout: 120_000 });
    await expect(modal).not.toBeVisible({ timeout: 15_000 });

    // Find the freshly-uploaded doc id via API (title defaults to filename).
    const listRes = await page.request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/documents`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    const list = (await listRes.json()).data as { id: string; title: string }[];
    const doc = list.find((d) => d.title.includes('long_prose'));
    expect(doc, 'uploaded document should appear in the workspace').toBeTruthy();
    docId = doc!.id;

    // ── 2. Baseline: default chunk size splits the prose into many chunks ──
    const baseline = await waitForReady(page.request, token, wsId, docId);
    console.log(`[per-scope-chunking] baseline chunk_count=${baseline.chunkCount}`);
    expect(baseline.chunkCount, 'prose should split into multiple chunks at default size').toBeGreaterThan(1);

    // ── 3. Set a scoped Max Chunk Size override in Settings ────────────────
    // The scope selector only cascades into workspace options when there is a
    // single org+dept; with a multi-org DB only org-level scope is selectable
    // in the UI. We set the override at org scope — the workspace inherits it
    // via the inheritance chain (workspace → dept → org → global), exercising
    // the same resolve_setting + ChunkOverrides path at ingest time.
    await navigateTo(page, 'Settings');
    await expect(page.getByText('Settings Scope:')).toBeVisible();

    const scopeSelect = page.locator('.ant-select').filter({ hasText: /Global/ });
    await scopeSelect.click();
    // Type-to-filter: the dropdown virtualizes with many orgs, so off-screen
    // options aren't in the DOM until the search narrows the list.
    await page.keyboard.type(orgName);
    await page.getByText(`Org: ${orgName}`).click();
    await expect(page.locator('.ant-tag').filter({ hasText: 'Organization' })).toBeVisible();

    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await expect(page.getByText('Max Chunk Size (chars)')).toBeVisible();

    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Pipeline Settings' });
    const maxChunkInput = pipelineCard.getByRole('spinbutton').first();
    await maxChunkInput.click();
    await maxChunkInput.fill('8000');
    await maxChunkInput.blur();

    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    // Confirm the override landed at org scope (not global).
    const scopedCfg = await page.request.get(
      `${API_BASE}/api/km/settings/document?scope_type=org&scope_id=${orgId}`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect((await scopedCfg.json()).max_chunk_size).toBe(8000);

    // Global scope must remain untouched (override is org-local).
    const globalCfg = await page.request.get(
      `${API_BASE}/api/km/settings/document`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect((await globalCfg.json()).max_chunk_size).not.toBe(8000);

    // ── 4. Reprocess and verify the prose is now a single chunk ────────────
    await navigateTo(page, 'Documents');
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

    const docRow = page.locator('tr', { hasText: 'long_prose' });
    await expect(docRow).toBeVisible({ timeout: 5000 });
    // Reprocess is an icon-only button (ReloadOutlined) that now opens the
    // Reprocess-with-options modal (the old Popconfirm flow was replaced).
    await docRow.locator('button:has(.anticon-reload)').click();
    const reprocessModal = page.locator('.ant-modal').filter({ hasText: 'Reprocess —' });
    await expect(reprocessModal).toBeVisible({ timeout: 10_000 });
    await reprocessModal.getByRole('button', { name: 'Reprocess', exact: true }).click();
    // Confirm the reprocess actually fired before we poll for the result.
    await expect(page.getByText('Reprocessing started')).toBeVisible({ timeout: 5000 });

    // Wait for the *re*processed result — updated_at must advance past baseline.
    const overridden = await waitForReady(page.request, token, wsId, docId, baseline.updatedAt);
    console.log(`[per-scope-chunking] overridden chunk_count=${overridden.chunkCount}`);

    // The override must produce strictly fewer chunks than the baseline split.
    expect(overridden.chunkCount).toBeLessThan(baseline.chunkCount);

    // And the whole prose document should now live in a single chunk.
    const chunksRes = await page.request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}/chunks`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect((await chunksRes.json()).total).toBe(1);
  });
});
