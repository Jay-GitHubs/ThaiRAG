import { test, expect, type APIRequestContext } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE, GOOD_CHAT_MODEL , snapshotSettings, restoreSettingsSnapshot } from './helpers';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Guard: the configured default chat model must NOT return an empty answer in
 * lean (shared) mode.
 *
 * Why this exists: under the "Balanced"/lean preset, quality_guard is OFF, so
 * there is no retry loop. If the answer LLM emits a response that is entirely a
 * <think>...</think> block (it reasons but never writes an answer, or runs out
 * mid-thought), the stream's think-suppression (ollama.rs) yields nothing and
 * the user sees a BLANK answer. This is model-dependent and intermittent —
 * gemma4:e4b-it-bf16 trips it; qwen3:14b does not. A scripted single-shot misses
 * it, so this drives the real streaming UI N times and fails if any answer is
 * blank.
 *
 * Uses the image-bearing PowerPoint export with enrichment ON — the exact path
 * the bug was found on (chunks carry images → response_generator vision path).
 */
const PDF_PATH = path.resolve(__dirname, '../../tests/fixtures/test-from-powerpoint.pdf');
const QUESTION = 'ธุรกิจต้องห้ามมีอะไรบ้าง';
const REPEATS = 6;

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
// Lean shared: the failing structure. Heavy/guard agents off, light agents on.
const LEAN_TOGGLES = {
  query_analyzer_enabled: true,
  context_curator_enabled: true,
  language_adapter_enabled: true,
  orchestrator_enabled: false,
  quality_guard_enabled: false,
  query_rewriter_enabled: false,
};

async function waitForReady(request: APIRequestContext, token: string, wsId: string, docId: string, timeoutMs = 300_000) {
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
    await new Promise((r) => setTimeout(r, 1500));
  }
  throw new Error('Timed out waiting for document');
}

// Exercises Ollama-specific think-suppression behavior (ollama.rs strips
// `<think>` output; gemma4:e4b trips the blank-answer guard, qwen3:14b doesn't).
// Meaningless on an all-gateway deployment where Ollama isn't running. Skipped
// by default; set RUN_OLLAMA_BENCH=1 to run it with local Ollama.
const guardDescribe = process.env.RUN_OLLAMA_BENCH ? test.describe : test.describe.skip;
guardDescribe('Thai empty-answer guard (lean shared, default model)', () => {
  const suffix = Date.now();
  const orgName = `GuardOrg-${suffix}`;
  const deptName = `GuardDept-${suffix}`;
  const wsName = `GuardWS-${suffix}`;

  let token: string;
  let orgId: string, deptId: string, wsId: string, docId: string;
  let defaultModel = GOOD_CHAT_MODEL;
  let snapId: string;

  test.beforeAll(async ({ request }) => {
    token = (await (await request.post(`${API_BASE}/api/auth/login`, { data: { email: TEST_EMAIL, password: TEST_PASSWORD } })).json()).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()).id;
    deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: wsName }, headers })).json()).id;

    // Vet the known-good default model; set GUARD_MODEL to vet a specific one.
    // Not the ambient model — under the full suite that can be a leaked/unpulled
    // model from an earlier spec, which would time out instead of testing intent.
    defaultModel = process.env.GUARD_MODEL || GOOD_CHAT_MODEL;
    // Exact server-side settings snapshot (replaces the old hand-built restore
    // which hardcoded kind Ollama and wiped pre-existing per-agent overrides).
    snapId = await snapshotSettings(request, token, 'e2e-thai-guard-baseline');

    // Enrichment ON + big chunk — mirror the real ingested doc that hit the bug.
    await request.put(`${API_BASE}/api/km/settings/document`, { data: { ai_preprocessing: { enabled: true } }, headers });
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

  test('default model never returns a blank Thai answer in lean shared', async ({ page }) => {
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
    docId = list.find((d) => d.title.includes('test-from-powerpoint'))!.id;
    const chunks = await waitForReady(page.request, token, wsId, docId);
    expect(chunks).toBeGreaterThan(0);

    // Force lean shared with the model under test (GOOD_CHAT_MODEL, or GUARD_MODEL).
    const put = await page.request.put(`${API_BASE}/api/km/settings/chat-pipeline`, {
      data: { llm_mode: 'shared', llm: OLLAMA(defaultModel), ...REMOVE_ALL_AGENT_LLMS, ...LEAN_TOGGLES },
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(put.ok(), 'applying lean-shared default model').toBeTruthy();

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

    // Read the answer text rendered for the latest completed assistant message.
    // Each completed assistant turn renders a `[data-tour="chat-pipeline"]` block
    // as a sibling of its answer Card inside the same wrapper div.
    async function latestAnswerText(): Promise<string> {
      return page.evaluate(() => {
        const blocks = document.querySelectorAll('[data-tour="chat-pipeline"]');
        const last = blocks[blocks.length - 1];
        const wrapper = last?.parentElement;
        const body = wrapper?.querySelector('.ant-card-body');
        return (body?.textContent ?? '').trim();
      });
    }

    const lengths: number[] = [];
    const empties: number[] = [];
    for (let i = 0; i < REPEATS; i++) {
      const before = await page.locator('[data-tour="chat-pipeline"]').count();
      await input.fill(QUESTION);
      await page.locator('[data-tour="chat-send"]').click();
      await expect
        .poll(async () => page.locator('[data-tour="chat-pipeline"]').count(), { timeout: 480_000, intervals: [2000] })
        .toBeGreaterThan(before);

      const ans = await latestAnswerText();
      lengths.push(ans.length);
      if (ans.length === 0) empties.push(i + 1);
      console.log(`[empty-guard] ${defaultModel} run ${i + 1}/${REPEATS}: len=${ans.length}`);
    }

    console.log('\n===== THAI EMPTY-ANSWER GUARD (lean shared) =====');
    console.log(`model=${defaultModel} lengths=[${lengths.join(', ')}] empty_runs=[${empties.join(', ')}]`);

    expect(empties, `model "${defaultModel}" returned a BLANK answer on run(s) ${empties.join(', ')} of ${REPEATS} in lean-shared — it emits answer-less <think> output. Switch the default chat model (qwen3:14b is known-good).`).toEqual([]);
  });
});
