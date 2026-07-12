import { test, expect, type APIRequestContext } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE, GOOD_CHAT_MODEL , snapshotSettings, restoreSettingsSnapshot } from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Chat-pipeline speed benchmark (headed).
 *
 * The default chat pipeline runs every agent (query analyzer/rewriter, context
 * curator, orchestrator, response generator, quality guard, language adapter) on
 * one big shared model (qwen3.6:35b, 23 GB) — so a single answer fans out into
 * many slow LLM calls. This spec sweeps several configurations across all three
 * `llm_mode` values (shared / per-agent / lean) and measures real end-to-end
 * latency via the Test Chat UI's "Total: Xms" timing tag, while checking the
 * answer is still correct (the atomic table chunk is retrieved + a non-empty
 * answer renders).
 *
 * It changes GLOBAL chat-pipeline settings (hot-reloaded by the API) and restores
 * the original configuration in afterAll.
 */
const PDF_PATH = path.resolve(
  __dirname,
  '../../tests/fixtures/micro_sme_prohibited_business.pdf',
);
const TABLE_TOKEN = 'KYC/CDD';

// Installed Ollama models we benchmark (see `ollama list`).
const M_35B = 'qwen3.6:35b'; // current shared model (baseline)
const M_CHINDA = 'iapp/chinda-qwen3-4b'; // 2.5 GB, Thai-tuned (smallest)
const M_VL8 = 'qwen3-vl:8b'; // 6.1 GB

const QUESTION =
  'According to the prohibited-business table, what cautions apply to money laundering (การฟอกเงิน) businesses?';

const OLLAMA = (model: string) => ({ kind: 'Ollama', model });

interface Scenario {
  name: string;
  mode: 'shared' | 'per-agent';
  body: Record<string, unknown>;
}

// Each scenario is a complete chat-pipeline override. Shared scenarios clear any
// per-agent overrides (remove_*_llm) and re-enable all agents unless stated.
const REMOVE_ALL_AGENT_LLMS = {
  remove_query_analyzer_llm: true,
  remove_query_rewriter_llm: true,
  remove_context_curator_llm: true,
  remove_response_generator_llm: true,
  remove_quality_guard_llm: true,
  remove_language_adapter_llm: true,
  remove_orchestrator_llm: true,
};
const ENABLE_ALL_AGENTS = {
  query_analyzer_enabled: true,
  query_rewriter_enabled: true,
  context_curator_enabled: true,
  quality_guard_enabled: true,
  language_adapter_enabled: true,
  orchestrator_enabled: true,
};

// Ordered fastest-likely first so the most useful data lands early; the slow
// 35b baseline runs last.
const SCENARIOS: Scenario[] = [
  {
    name: `shared / ${M_CHINDA} (smallest)`,
    mode: 'shared',
    body: { llm_mode: 'shared', llm: OLLAMA(M_CHINDA), ...REMOVE_ALL_AGENT_LLMS, ...ENABLE_ALL_AGENTS },
  },
  {
    name: `lean shared / ${M_CHINDA} (orchestrator+quality+rewriter off)`,
    mode: 'shared',
    body: {
      llm_mode: 'shared',
      llm: OLLAMA(M_CHINDA),
      ...REMOVE_ALL_AGENT_LLMS,
      ...ENABLE_ALL_AGENTS,
      orchestrator_enabled: false,
      quality_guard_enabled: false,
      query_rewriter_enabled: false,
    },
  },
  {
    name: `per-agent / light=${M_CHINDA}, generate=${M_VL8}`,
    mode: 'per-agent',
    body: {
      llm_mode: 'per-agent',
      llm: OLLAMA(M_CHINDA),
      query_analyzer_llm: OLLAMA(M_CHINDA),
      query_rewriter_llm: OLLAMA(M_CHINDA),
      context_curator_llm: OLLAMA(M_CHINDA),
      quality_guard_llm: OLLAMA(M_CHINDA),
      language_adapter_llm: OLLAMA(M_CHINDA),
      orchestrator_llm: OLLAMA(M_CHINDA),
      response_generator_llm: OLLAMA(M_VL8),
      ...ENABLE_ALL_AGENTS,
    },
  },
  {
    name: `shared / ${M_VL8}`,
    mode: 'shared',
    body: { llm_mode: 'shared', llm: OLLAMA(M_VL8), ...REMOVE_ALL_AGENT_LLMS, ...ENABLE_ALL_AGENTS },
  },
  {
    name: `shared / ${M_35B} (baseline)`,
    mode: 'shared',
    body: { llm_mode: 'shared', llm: OLLAMA(M_35B), ...REMOVE_ALL_AGENT_LLMS, ...ENABLE_ALL_AGENTS },
  },
];

interface Result {
  scenario: string;
  cold_ms: number | null;
  warm_ms: number | null;
  retrievedToken: boolean;
  answeredNonEmpty: boolean;
  note: string;
}

interface DocState {
  chunkCount: number;
}

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
    const res = await request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, { headers });
    if (res.ok()) {
      const { data } = await res.json();
      const doc = data.find((d: { id: string }) => d.id === docId);
      if (doc && doc.status !== 'processing') {
        if (doc.status === 'failed') throw new Error(`processing failed: ${doc.error_message ?? '?'}`);
        return { chunkCount: doc.chunk_count as number };
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error('Timed out waiting for document to finish processing');
}

// Benchmarks a sweep of INSTALLED LOCAL OLLAMA models — meaningless (and hangs
// for the full timeout) on an all-gateway deployment where Ollama isn't running.
// Skipped by default; set RUN_OLLAMA_BENCH=1 to run it with local Ollama.
const benchDescribe = process.env.RUN_OLLAMA_BENCH ? test.describe : test.describe.skip;
benchDescribe('Chat pipeline speed benchmark (model + mode sweep)', () => {
  const suffix = Date.now();
  const orgName = `SpeedOrg-${suffix}`;
  const deptName = `SpeedDept-${suffix}`;
  const wsName = `SpeedWS-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;
  let docId: string;
  let snapId: string;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    token = (await loginRes.json()).token;
    const headers = { Authorization: `Bearer ${token}` };

    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()).id;
    deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: wsName }, headers })).json()).id;

    // Exact server-side settings snapshot (replaces the old hand-built restore
    // which hardcoded kind Ollama and wiped pre-existing per-agent overrides).
    snapId = await snapshotSettings(request, token, 'e2e-chat-speed-bench-baseline');

    // Disable AI preprocessing for fast deterministic ingest; raise chunk size so
    // the table is one atomic chunk. Snapshot-restored in afterAll.
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

  test('sweep models + modes and measure latency vs correctness', async ({ page }) => {
    test.setTimeout(2_700_000); // 45 min — many slow LLM passes

    // ── Seed: upload the table PDF, confirm one atomic chunk ───────────────
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
    const ready = await waitForReady(page.request, token, wsId, docId);
    expect(ready.chunkCount).toBe(1);

    // ── UX check: the Chat & Response Pipeline settings tab renders ────────
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await expect(page.getByText('Agent LLM Configuration')).toBeVisible({ timeout: 10_000 });

    // ── Open Test Chat and select the workspace ────────────────────────────
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

    // Send the question once; return the server-reported total_ms from the newest
    // assistant message, or null on timeout/failure. `priorCount` is the number of
    // "Total:" tags already present so we can wait for a new one.
    async function askOnce(priorCount: number): Promise<number | null> {
      await input.fill(QUESTION);
      await page.locator('[data-tour="chat-send"]').click();
      const totals = page.getByText(/Total: \d+ms/);
      try {
        await expect.poll(async () => await totals.count(), { timeout: 600_000, intervals: [2000] }).toBeGreaterThan(priorCount);
      } catch {
        return null;
      }
      const txt = (await totals.last().textContent()) ?? '';
      const m = txt.match(/Total: (\d+)ms/);
      return m ? Number(m[1]) : null;
    }

    const results: Result[] = [];

    for (const sc of SCENARIOS) {
      // Apply the scenario config globally (hot-reloaded by the API).
      const put = await page.request.put(`${API_BASE}/api/km/settings/chat-pipeline`, {
        data: sc.body,
        headers: { Authorization: `Bearer ${token}` },
      });
      expect(put.ok(), `applying scenario "${sc.name}"`).toBeTruthy();

      const before = await page.getByText(/Total: \d+ms/).count();
      const cold = await askOnce(before); // first call also loads the model
      const mid = await page.getByText(/Total: \d+ms/).count();
      const warm = cold === null ? null : await askOnce(mid); // second call = warm

      // Correctness: inspect the newest retrieved-chunks panel for the table token.
      let retrievedToken = false;
      const chunkLabels = page.getByText(/\d+ chunks? retrieved/);
      if ((await chunkLabels.count()) > 0) {
        await chunkLabels.last().click().catch(() => {});
        retrievedToken = (await page.getByText(new RegExp(TABLE_TOKEN.replace('/', '\\/'))).count()) > 0;
      }
      const answeredNonEmpty = (cold ?? warm) !== null;

      results.push({
        scenario: sc.name,
        cold_ms: cold,
        warm_ms: warm,
        retrievedToken,
        answeredNonEmpty,
        note: cold === null ? 'no response (timeout/error)' : '',
      });
      console.log(`[bench] ${sc.name}: cold=${cold}ms warm=${warm}ms token=${retrievedToken} ok=${answeredNonEmpty}`);
    }

    // Print a compact summary table to the test output.
    console.log('\n===== CHAT SPEED BENCHMARK =====');
    console.table(
      results.map((r) => ({
        scenario: r.scenario,
        cold_ms: r.cold_ms ?? 'FAIL',
        warm_ms: r.warm_ms ?? 'FAIL',
        token: r.retrievedToken ? 'yes' : 'no',
        answered: r.answeredNonEmpty ? 'yes' : 'no',
      })),
    );

    // The benchmark is informational, but at least one small-model scenario must
    // have produced a correct, non-empty answer — proving a faster config works.
    const anySmallOk = results.some(
      (r) => r.scenario.includes(M_CHINDA) && r.answeredNonEmpty && r.retrievedToken,
    );
    expect(anySmallOk, 'at least one small-model config should answer correctly').toBeTruthy();
  });
});
