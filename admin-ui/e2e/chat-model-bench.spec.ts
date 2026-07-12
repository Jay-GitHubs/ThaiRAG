import { test, expect, type APIRequestContext } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import {
  login,
  navigateTo,
  TEST_EMAIL,
  TEST_PASSWORD,
  API_BASE,
  GOOD_CHAT_MODEL,
  snapshotSettings,
  restoreSettingsSnapshot,
} from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Model-only speed sweep (headed), all in the fastest "lean shared" structure.
 *
 * Holds the pipeline structure fixed (lean shared: orchestrator + quality-guard +
 * query-rewriter off) and varies only the model, to find the fastest model that
 * still answers a Thai/English table question correctly. Compares the current
 * pick (chinda-4b) against the latest SCB10X Typhoon small models — including the
 * non-reasoning Llama3.2-Typhoon2 1B/3B which skip the <think> overhead.
 *
 * Captures + restores the live chat-pipeline config (model + agent toggles).
 */
const PDF_PATH = path.resolve(__dirname, '../../tests/fixtures/micro_sme_prohibited_business.pdf');
const TABLE_TOKEN = 'KYC/CDD';

const QUESTION =
  'According to the prohibited-business table, what cautions apply to money laundering (การฟอกเงิน) businesses?';

// Lean shared structure is fixed; only this list varies.
const MODELS = [
  'iapp/chinda-qwen3-4b', // current pick (reasoning, Qwen3-4b) — reference
  'scb10x/llama3.2-typhoon2-1b-instruct', // Thai-tuned 1B, non-reasoning (fastest bet)
  'scb10x/llama3.2-typhoon2-3b-instruct', // Thai-tuned 3B, non-reasoning
  'scb10x/typhoon2.5-qwen3-4b', // latest Thai-tuned 4B (reasoning, Qwen3)
];

const OLLAMA = (model: string) => ({ kind: 'Ollama', model });
const REMOVE_ALL_AGENT_LLMS = {
  remove_query_analyzer_llm: true,
  remove_query_rewriter_llm: true,
  remove_context_curator_llm: true,
  remove_response_generator_llm: true,
  remove_quality_guard_llm: true,
  remove_language_adapter_llm: true,
  remove_orchestrator_llm: true,
};
// Lean: heavy agents off, light agents on.
const LEAN_TOGGLES = {
  query_analyzer_enabled: true,
  context_curator_enabled: true,
  language_adapter_enabled: true,
  orchestrator_enabled: false,
  quality_guard_enabled: false,
  query_rewriter_enabled: false,
};

interface Result {
  model: string;
  cold_ms: number | null;
  warm_ms: number | null;
  token: boolean;
  ok: boolean;
}

async function waitForReady(request: APIRequestContext, token: string, wsId: string, docId: string, timeoutMs = 120_000) {
  const headers = { Authorization: `Bearer ${token}` };
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const res = await request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, { headers });
    if (res.ok()) {
      const doc = (await res.json()).data.find((d: { id: string }) => d.id === docId);
      if (doc && doc.status !== 'processing') {
        if (doc.status === 'failed') throw new Error(`processing failed: ${doc.error_message ?? '?'}`);
        return doc.chunk_count as number;
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error('Timed out waiting for document');
}

// Benchmarks a sweep of INSTALLED LOCAL OLLAMA models — meaningless (and hangs
// for the full timeout) on an all-gateway deployment where Ollama isn't running.
// Skipped by default; set RUN_OLLAMA_BENCH=1 to run it with local Ollama.
const benchDescribe = process.env.RUN_OLLAMA_BENCH ? test.describe : test.describe.skip;
benchDescribe('Chat model speed sweep (lean shared, latest Thai models)', () => {
  const suffix = Date.now();
  const orgName = `ModelOrg-${suffix}`;
  const deptName = `ModelDept-${suffix}`;
  const wsName = `ModelWS-${suffix}`;

  let token: string;
  let orgId: string, deptId: string, wsId: string, docId: string;
  let snapId: string;

  test.beforeAll(async ({ request }) => {
    token = (await (await request.post(`${API_BASE}/api/auth/login`, { data: { email: TEST_EMAIL, password: TEST_PASSWORD } })).json()).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()).id;
    deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: wsName }, headers })).json()).id;

    // Exact server-side settings snapshot: the old hand-built restore
    // hardcoded kind Ollama + wiped any pre-existing per-agent LLM overrides
    // (unreadable api keys made a spec-side copy impossible to get right).
    snapId = await snapshotSettings(request, token, 'e2e-chat-model-bench-baseline');

    await request.put(`${API_BASE}/api/km/settings/document`, { data: { ai_preprocessing: { enabled: false } }, headers });
    await request.put(`${API_BASE}/api/km/settings/document?scope_type=org&scope_id=${orgId}`, { data: { max_chunk_size: 8000 }, headers });
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    // Snapshot restore rewrites ALL settings rows exactly (chat-pipeline,
    // document, and it clears the org-scoped override created above).
    await restoreSettingsSnapshot(request, token, snapId);
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('find the fastest lean-shared model that still answers correctly', async ({ page }) => {
    test.setTimeout(1_800_000);

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
    // Upload now keeps the modal open as a live processing tracker; dismiss it.
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(modal).not.toBeVisible({ timeout: 15_000 });

    const list = (await (await page.request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, { headers: { Authorization: `Bearer ${token}` } })).json()).data as { id: string; title: string }[];
    docId = list.find((d) => d.title.includes('micro_sme_prohibited_business'))!.id;
    expect(await waitForReady(page.request, token, wsId, docId)).toBe(1);

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

    const input = page.locator('[data-tour="chat-input"] textarea, textarea[data-tour="chat-input"]').first();

    async function askOnce(priorCount: number): Promise<number | null> {
      await input.fill(QUESTION);
      await page.locator('[data-tour="chat-send"]').click();
      const totals = page.getByText(/Total: \d+ms/);
      try {
        await expect.poll(async () => await totals.count(), { timeout: 480_000, intervals: [2000] }).toBeGreaterThan(priorCount);
      } catch {
        return null;
      }
      const m = ((await totals.last().textContent()) ?? '').match(/Total: (\d+)ms/);
      return m ? Number(m[1]) : null;
    }

    const results: Result[] = [];
    for (const model of MODELS) {
      const put = await page.request.put(`${API_BASE}/api/km/settings/chat-pipeline`, {
        data: { llm_mode: 'shared', llm: OLLAMA(model), ...REMOVE_ALL_AGENT_LLMS, ...LEAN_TOGGLES },
        headers: { Authorization: `Bearer ${token}` },
      });
      expect(put.ok(), `applying model ${model}`).toBeTruthy();

      const before = await page.getByText(/Total: \d+ms/).count();
      const cold = await askOnce(before);
      const mid = await page.getByText(/Total: \d+ms/).count();
      const warm = cold === null ? null : await askOnce(mid);

      let tokenSeen = false;
      const chunkLabels = page.getByText(/\d+ chunks? retrieved/);
      if ((await chunkLabels.count()) > 0) {
        await chunkLabels.last().click().catch(() => {});
        tokenSeen = (await page.getByText(new RegExp(TABLE_TOKEN.replace('/', '\\/'))).count()) > 0;
      }
      results.push({ model, cold_ms: cold, warm_ms: warm, token: tokenSeen, ok: (cold ?? warm) !== null });
      console.log(`[model-bench] ${model}: cold=${cold}ms warm=${warm}ms token=${tokenSeen} ok=${(cold ?? warm) !== null}`);
    }

    console.log('\n===== MODEL SPEED SWEEP (lean shared) =====');
    console.table(results.map((r) => ({ model: r.model, cold_ms: r.cold_ms ?? 'FAIL', warm_ms: r.warm_ms ?? 'FAIL', token: r.token ? 'yes' : 'no', ok: r.ok ? 'yes' : 'no' })));

    // At least one Typhoon candidate must answer correctly (proves a valid alternative exists).
    expect(results.some((r) => r.model.includes('typhoon') && r.ok && r.token), 'a Typhoon model should answer correctly').toBeTruthy();
  });
});
