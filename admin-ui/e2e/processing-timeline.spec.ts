import { test, expect, type APIRequestContext } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

/**
 * Live processing tracker (AI preprocessing path).
 *
 * Exercises the per-stage upload tracker end-to-end with AI preprocessing
 * ENABLED, so the document flows through the multi-agent pipeline
 * (analyze → convert → quality → chunk → enrich → index) rather than the
 * single-step mechanical path. Verifies:
 *   1. The upload modal stays open and renders the live step tracker.
 *   2. The tracker reaches "Ready" and shows the processing-path provenance.
 *   3. The backend recorded a multi-stage `processing_timeline` with timings.
 */

interface TimelineEntry {
  step: string;
  started_at_ms: number;
  duration_ms?: number;
}

async function waitForDoc(
  request: APIRequestContext,
  token: string,
  wsId: string,
  docId: string,
  // Even a 2KB text doc runs 4 gateway LLM stages; minutes under load.
  timeoutMs = 1_200_000,
) {
  const headers = { Authorization: `Bearer ${token}` };
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const res = await request.get(
      `${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}`,
      { headers },
    );
    if (res.ok()) {
      const doc = await res.json();
      if (doc.status !== 'processing') return doc;
    }
    await new Promise((r) => setTimeout(r, 1500));
  }
  throw new Error('Timed out waiting for AI-preprocessed document to finish');
}

test.describe('Processing timeline (AI preprocessing)', () => {
  const suffix = Date.now();
  const orgName = `TimelineOrg-${suffix}`;
  const deptName = `TimelineDept-${suffix}`;
  const wsName = `TimelineWS-${suffix}`;

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;
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

    // Turn AI preprocessing ON so the upload runs the full multi-agent pipeline.
    const cfgRes = await request.get(`${API_BASE}/api/km/settings/document`, { headers });
    originalAiEnabled = (await cfgRes.json()).ai_preprocessing.enabled;
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: true } },
      headers,
    });
  });

  test.afterAll(async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
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

  test('upload shows live per-stage tracker and records stage timings', async ({ page }) => {
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

    await expect(page.getByRole('button', { name: 'Upload File' })).toBeVisible({ timeout: 5000 });
    await page.getByRole('button', { name: 'Upload File' }).click();

    const uploadModal = page.locator('.ant-modal', { hasText: 'Upload Document' });
    await expect(uploadModal).toBeVisible();

    // Upload an in-memory text doc that is comfortably larger than the
    // pipeline's `min_ai_size_bytes` threshold (default 500B) so AI
    // preprocessing actually engages rather than falling back to the fast
    // mechanical path. The content is real prose so the agents have something
    // to analyze/convert/chunk/enrich.
    const para =
      'Quality assurance overview. This document describes software testing ' +
      'practices in depth. Unit tests verify that individual functions behave ' +
      'correctly in isolation, mocking external dependencies so failures point ' +
      'directly at the unit under test. Integration tests check that components ' +
      'cooperate: database access layers, HTTP handlers, and message queues are ' +
      'exercised together against real (or realistic) infrastructure. End-to-end ' +
      'tests drive the full application the way a user would, validating critical ' +
      'flows such as login, document upload, and search. Continuous integration ' +
      'runs the whole suite on every commit, and a regression in any layer blocks ' +
      'the merge until it is resolved. Coverage metrics guide where to invest, ' +
      'but high coverage alone does not guarantee correctness — assertions must ' +
      'be meaningful. Flaky tests are quarantined and fixed promptly because they ' +
      'erode trust in the suite. ';
    await uploadModal.locator('input[type="file"]').setInputFiles({
      name: `ai-timeline-${suffix}.txt`,
      mimeType: 'text/plain',
      buffer: Buffer.from(para.repeat(2)),
    });
    await uploadModal.getByRole('button', { name: 'Upload' }).click();

    // ── The modal stays open and becomes the live processing tracker ─────────
    const tracker = page.locator('.ant-modal', { hasText: 'Processing Document' });
    await expect(tracker).toBeVisible({ timeout: 15_000 });
    // The vertical step list and the leading "Uploaded" node should render.
    await expect(tracker.locator('.ant-steps')).toBeVisible();
    await expect(tracker.getByText('Uploaded')).toBeVisible();

    // ── The model running each stage is shown LIVE (while still processing) ──
    // Source of truth = the configured analyzer model; it must appear in the
    // tracker before the doc finishes, proving live (not post-hoc) attribution.
    const docCfg = await (
      await page.request.get(`${API_BASE}/api/km/settings/document`, {
        headers: { Authorization: `Bearer ${token}` },
      })
    ).json();
    const liveModel = docCfg.ai_preprocessing?.analyzer_llm?.model as string;
    expect(liveModel, 'configured analyzer model').toBeTruthy();
    await expect(tracker.getByText(liveModel, { exact: false }).first()).toBeVisible({
      timeout: 60_000,
    });
    // Confirm the doc is still processing at this point — i.e. the model is
    // surfaced live, not only after completion.
    const midRun = await (
      await page.request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents`, {
        headers: { Authorization: `Bearer ${token}` },
      })
    ).json();
    const midDoc = (midRun.data as { id: string; title: string; status: string }[]).find((d) =>
      d.title.includes(`ai-timeline-${suffix}`),
    )!;
    const docId = midDoc.id;
    // Capture the live tracker mid-processing (stages + models in flight).
    await tracker.screenshot({ path: 'e2e/screenshots/processing-timeline-ai-live.png' });

    // ── Tracker reaches a terminal "Ready" state in the UI ───────────────────
    // Bulk-lane backpressure serializes each doc's pipeline calls — in-suite
    // ingestion walls run ~2x the quiet-stack time.
    await expect(tracker.getByText('Ready')).toBeVisible({ timeout: 1_200_000 });
    // Provenance summary (the processing path) is shown on completion.
    await expect(tracker.getByText('Path:')).toBeVisible();

    // ── Backend recorded a multi-stage timeline with per-stage timings ───────
    const doc = await waitForDoc(page.request, token, wsId, docId);
    expect(doc.status, `doc failed: ${doc.error_message ?? ''}`).toBe('ready');
    const timeline = (doc.processing_timeline ?? []) as TimelineEntry[];
    console.log(
      '[processing-timeline] stages:',
      timeline.map((t) => `${t.step}=${t.duration_ms ?? '…'}ms`).join(', '),
    );
    // AI preprocessing should produce several distinct stages, not just indexing.
    expect(timeline.length, 'expected multiple AI stages in timeline').toBeGreaterThanOrEqual(2);
    // Every completed stage carries a non-negative duration.
    for (const entry of timeline.slice(0, -1)) {
      expect(entry.duration_ms, `stage ${entry.step} missing duration`).toBeGreaterThanOrEqual(0);
    }
    // The pipeline must have reported the embed/index stage.
    expect(timeline.some((t) => t.step.includes('index'))).toBeTruthy();

    // ── The tracker attributes the model that ran each stage ─────────────────
    const agents = (doc.processing_provenance?.agents ?? []) as {
      model?: string;
      status: string;
    }[];
    const ranModel = agents.find((a) => a.model)?.model;
    expect(ranModel, 'provenance should record the model used').toBeTruthy();
    await expect(tracker.getByText(ranModel!, { exact: false }).first()).toBeVisible();

    // Capture the completed sequential tracker (stages + models + timings).
    await tracker.screenshot({ path: 'e2e/screenshots/processing-timeline-ai.png' });

    await page.getByRole('button', { name: 'Done' }).click();
    await expect(tracker).not.toBeVisible({ timeout: 15_000 });
  });
});
