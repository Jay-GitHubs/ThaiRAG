import { test, expect, type Page } from '@playwright/test';
import { mkdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { API_BASE, TEST_EMAIL, TEST_PASSWORD } from './helpers';

/**
 * INLINE-CITATION verification spec.
 *
 * Drives the real Open WebUI chat surface (localhost:3000) end-to-end via
 * Keycloak SSO and asserts that ThaiRAG citations render as OWUI's native,
 * one-click INLINE source modals (snippet text shown in-app), NOT as
 * title-only links that open a new tab.
 *
 * Backend mechanism (chat.rs, PR #130): when the request carries the OWUI
 * user header, the stream emits content-bearing
 * `{"event":{"type":"source",...}}` chunks whose `document` array holds the
 * real retrieved snippets and whose `metadata.source` is a non-URL doc id.
 * OWUI v0.9.6 renders these as a "{N} Sources" toggle → numbered source
 * entries (`#source-...`) → CitationModal showing the snippet text.
 *
 * Run headed:
 *   npx playwright test e2e/owui-citations.spec.ts --headed --project=e2e
 *
 * Login uses the documented Keycloak test account (docs/OIDC_TESTING.md).
 */

const OWUI = process.env.OWUI_URL ?? 'http://localhost:3000';
const KC_USER = process.env.OWUI_USER ?? 'testuser';
const KC_PASS = process.env.OWUI_PASS ?? 'test123';
const MODEL = process.env.OWUI_MODEL ?? 'ThaiRAG-1.0';
const QUESTION = process.env.OWUI_QUESTION ?? 'ธุรกิจต้องห้ามมีอะไรบ้าง';
const SHOTS = 'e2e/screenshots/owui';

// The Keycloak test user (testuser → test@thairag.local) is a VIEWER scoped to
// this single workspace, so a doc must be ingested HERE for OWUI retrieval to
// surface it. Override via env if the seeded workspace id changes.
const INGEST_WS = process.env.OWUI_INGEST_WS ?? 'b5ce5fad-ee71-4b8c-aebf-fa28302722d5';
// Proven producer of section-bearing chunks ("Table: Prohibit Business and
// Cautions", etc.) whose Thai prohibited-business content matches QUESTION.
const FIXTURE_PDF = path.resolve(process.cwd(), '../tests/fixtures/test-from-powerpoint.pdf');

mkdirSync(SHOTS, { recursive: true });

async function shot(page: Page, name: string) {
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
  console.log(`[owui] screenshot -> ${SHOTS}/${name}.png`);
}

test.describe('Open WebUI citation rendering (observation)', () => {
  // The pipeline answer can be slow and we may probe a few follow-up questions
  // to surface section provenance; give the whole flow generous room.
  test.setTimeout(15 * 60_000);

  // Doc ingested freshly so its chunks carry persisted ChunkMetadata
  // (section_title) — the provenance the citation modal must render.
  let adminToken = '';
  let provenanceDocId = '';
  // Restore the global AI-preprocessing flag in afterAll (this spec needs it on
  // for section-provenance, but it's off by default for fast ingest elsewhere).
  let prevAiPreprocEnabled = false;

  test.beforeAll(async ({ request }) => {
    // Ingest can run the slow AI smart-chunker; allow plenty of headroom.
    test.setTimeout(9 * 60_000);

    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok(), 'admin API login should succeed').toBeTruthy();
    adminToken = (await loginRes.json()).token;
    const headers = { Authorization: `Bearer ${adminToken}` };

    // Section provenance comes from the AI smart-chunker's `section_title`,
    // which requires AI preprocessing ON and a model capable of structured
    // output (the fast 7B vision model doesn't emit it; the 27B does). Enable it
    // for this spec's ingest regardless of the global default (off for speed
    // elsewhere); restored in afterAll. Model-only update preserves the
    // configured gateway provider + key.
    const docCfg = await (await request.get(`${API_BASE}/api/km/settings/document`, { headers })).json();
    prevAiPreprocEnabled = docCfg.ai_preprocessing?.enabled ?? false;
    await request.put(`${API_BASE}/api/km/settings/document`, {
      data: { ai_preprocessing: { enabled: true, chunker_llm: { model: 'qwen3.6-27b-fast' } } },
      headers,
    });

    const uploadRes = await request.post(
      `${API_BASE}/api/km/workspaces/${INGEST_WS}/documents/upload`,
      {
        headers,
        multipart: {
          title: `owui-citation-provenance-${Date.now()}`,
          file: {
            name: 'test-from-powerpoint.pdf',
            mimeType: 'application/pdf',
            buffer: readFileSync(FIXTURE_PDF),
          },
        },
      },
    );
    expect(uploadRes.ok(), `upload should succeed (status ${uploadRes.status()})`).toBeTruthy();
    const uploaded = await uploadRes.json();
    provenanceDocId = uploaded.doc_id;
    console.log(`[owui] ingested provenance doc=${provenanceDocId} status=${uploaded.status}`);

    // Background processing returns "processing"; poll until ready.
    if (uploaded.status !== 'ready') {
      await expect
        .poll(
          async () => {
            const res = await request.get(
              `${API_BASE}/api/km/workspaces/${INGEST_WS}/documents/${provenanceDocId}`,
              { headers },
            );
            if (!res.ok()) return 'unknown';
            return (await res.json()).status as string;
          },
          { timeout: 8 * 60_000, intervals: [3000] },
        )
        .toBe('ready');
    }
    console.log(`[owui] provenance doc ready=${provenanceDocId}`);
  });

  test.afterAll(async ({ request }) => {
    if (!adminToken) return;
    const headers = { Authorization: `Bearer ${adminToken}` };
    // Restore the global AI-preprocessing flag so we don't slow down other specs.
    await request
      .put(`${API_BASE}/api/km/settings/document`, {
        data: { ai_preprocessing: { enabled: prevAiPreprocEnabled } },
        headers,
      })
      .catch(() => {});
    if (provenanceDocId) {
      await request
        .delete(`${API_BASE}/api/km/workspaces/${INGEST_WS}/documents/${provenanceDocId}`, {
          headers,
        })
        .catch(() => {});
    }
  });

  test('capture how ThaiRAG citations render in Open WebUI today', async ({ page }) => {
    // --- 1. Landing / login screen -----------------------------------------
    await page.goto(OWUI, { waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(2000);
    await shot(page, '01-landing');

    // If we land straight in the chat (existing session), skip SSO.
    const alreadyIn = await page
      .locator('#chat-input, [contenteditable="true"], textarea')
      .first()
      .isVisible()
      .catch(() => false);

    if (!alreadyIn) {
      // Click the Keycloak/OAuth SSO button. OWUI labels it with the provider
      // name ("Keycloak") or a generic "Continue with ..." button.
      const ssoButton = page
        .getByRole('button', { name: /keycloak|continue with|sign in with|oauth|sso/i })
        .or(page.getByRole('link', { name: /keycloak|continue with|sign in with/i }))
        .first();
      await expect(ssoButton, 'expected an SSO/Keycloak login button on the OWUI landing page').toBeVisible({
        timeout: 15_000,
      });
      await ssoButton.click();

      // --- 2. Keycloak login form ------------------------------------------
      // Standard Keycloak selectors are stable across versions.
      await page.waitForSelector('#username', { timeout: 20_000 });
      await shot(page, '02-keycloak-login');
      await page.fill('#username', KC_USER);
      await page.fill('#password', KC_PASS);
      await Promise.all([
        page.waitForURL((url) => url.toString().startsWith(OWUI), { timeout: 30_000 }).catch(() => {}),
        page.click('#kc-login, button[type="submit"], input[type="submit"]'),
      ]);
      await page.waitForTimeout(3000);
      await shot(page, '03-post-login');
    }

    // Dismiss any first-run modal / changelog if present.
    await page
      .getByRole('button', { name: /okay|got it|close|dismiss|confirm/i })
      .first()
      .click({ timeout: 3000 })
      .catch(() => {});

    // --- 3. Ensure the ThaiRAG model is selected ---------------------------
    // OWUI shows a model dropdown near the top of the chat. If only one model
    // is wired it may already be selected; try to set it explicitly anyway.
    const modelButton = page
      .getByRole('button', { name: new RegExp(MODEL, 'i') })
      .or(page.locator('button[aria-label*="model" i]'))
      .first();
    if (await modelButton.isVisible().catch(() => false)) {
      await modelButton.click().catch(() => {});
      const option = page.getByRole('option', { name: new RegExp(MODEL, 'i') }).first();
      if (await option.isVisible().catch(() => false)) {
        await option.click().catch(() => {});
      } else {
        await page.keyboard.press('Escape').catch(() => {});
      }
    }
    await shot(page, '04-chat-ready');

    const MSG_SEL = '[class*="message"], [data-message-role="assistant"], .chat-assistant';

    // Wait for the LAST assistant message to appear and its text to settle.
    // OWUI renders native citations only at completion, so we must not proceed
    // mid-stream. The plain-text "Sources:" footer is the last chunk on the
    // wire, so require several consecutive unchanged polls before continuing.
    const waitForAnswerStable = async () => {
      await expect
        .poll(
          async () => (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim().length,
          { timeout: 8 * 60_000, intervals: [2000] },
        )
        .toBeGreaterThan(20);
      let prev = '';
      let stableCount = 0;
      const deadline = Date.now() + 8 * 60_000;
      while (Date.now() < deadline) {
        await page.waitForTimeout(1500);
        const cur = (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim();
        if (cur.length > 20 && cur === prev) {
          stableCount += 1;
          if (stableCount >= 4) break; // ~6s with no change → stream settled
        } else {
          stableCount = 0;
        }
        prev = cur;
      }
      await page.waitForTimeout(2000);
    };

    const chatInput = () =>
      page
        .locator('#chat-input')
        .or(page.locator('[contenteditable="true"]'))
        .or(page.getByRole('textbox'))
        .first();

    const askQuestion = async (q: string) => {
      const input = chatInput();
      await expect(input, 'expected a chat input box').toBeVisible({ timeout: 15_000 });
      await input.click();
      await input.fill(q);
      await page.keyboard.press('Enter');
      await waitForAnswerStable();
    };

    // Expand the LAST answer's sources (scoped to that message so we don't pick
    // up an earlier question's citations) and click through every source modal,
    // returning the first modal text that renders "Section:" provenance.
    const scanLastSourcesForSection = async (): Promise<string> => {
      const block = page.locator(MSG_SEL).last();
      const entries = block.locator('[id^="source-"]');
      if (!(await entries.first().isVisible({ timeout: 3000 }).catch(() => false))) {
        const toggle = block
          .getByRole('button', { name: /toggle \d+ sources?|^\s*\d+ sources?\s*$|1 source/i })
          .first();
        if (!(await toggle.isVisible({ timeout: 20_000 }).catch(() => false))) return '';
        await toggle.click().catch(() => {});
      }
      const n = await entries.count();
      for (let i = 0; i < n; i += 1) {
        await entries.nth(i).click().catch(() => {});
        const m = page
          .locator('[role="dialog"], dialog, [id^="citation-"]')
          .filter({ hasText: /.+/ })
          .first();
        let t = '';
        if (await m.isVisible({ timeout: 5000 }).catch(() => false)) {
          t = await m.innerText().catch(() => '');
        }
        await page.keyboard.press('Escape').catch(() => {});
        await page.waitForTimeout(300);
        if (/Section:/i.test(t)) return t;
      }
      return '';
    };

    // --- 4. Send the Thai question -----------------------------------------
    const input = chatInput();
    await expect(input, 'expected a chat input box').toBeVisible({ timeout: 15_000 });
    await input.click();
    await input.fill(QUESTION);
    await shot(page, '05-question-typed');
    await page.keyboard.press('Enter');

    // --- 5. Wait for the assistant answer to finish ------------------------
    // First wait for non-trivial text to appear, then wait for generation to
    // actually finish: OWUI shows a "stop generating" button while streaming and
    // swaps it back to the send button when done. We must not screenshot
    // mid-stream or native citations (rendered at completion) will be missed.
    await expect
      .poll(
        async () => {
          const text = await page
            .locator('[class*="message"], [data-message-role="assistant"], .chat-assistant')
            .last()
            .innerText()
            .catch(() => '');
          return text.trim().length;
        },
        { timeout: 8 * 60_000, intervals: [2000] },
      )
      .toBeGreaterThan(20);

    // Now block until the assistant message text stabilizes — robust across
    // OWUI versions (button selectors drift). The plain-text "Sources:" footer
    // is the LAST chunk on the wire, so we must not screenshot until streaming
    // has fully settled or we'd miss it. Require the text to be unchanged across
    // several consecutive polls before proceeding.
    const lastAssistant = () =>
      page
        .locator('[class*="message"], [data-message-role="assistant"], .chat-assistant')
        .last()
        .innerText()
        .catch(() => '');
    let prev = '';
    let stableCount = 0;
    const deadline = Date.now() + 8 * 60_000;
    while (Date.now() < deadline) {
      await page.waitForTimeout(1500);
      const cur = (await lastAssistant()).trim();
      if (cur.length > 20 && cur === prev) {
        stableCount += 1;
        if (stableCount >= 4) break; // ~6s with no change → stream settled
      } else {
        stableCount = 0;
      }
      prev = cur;
    }

    // Give OWUI a moment to render the post-completion citation panel.
    await page.waitForTimeout(2000);
    await shot(page, '06-answer-with-citations');

    // --- 6. Verify NATIVE inline citations rendered ------------------------
    const lastMsg = page
      .locator('[class*="message"], [data-message-role="assistant"], .chat-assistant')
      .last();
    const bodyText = await lastMsg.innerText().catch(() => '');
    expect(bodyText.trim().length, 'expected a non-empty assistant answer').toBeGreaterThan(20);

    // The OWUI source events should NOT surface as a plain-text "Sources:"
    // footer in the message body (that footer is suppressed for OWUI).
    const hasPlainTextSources = /\*\*?Sources:?\*\*?|Response ID:/i.test(bodyText);
    console.log(`plain-text "Sources:" footer in body=${hasPlainTextSources}`);

    // (a) The "{N} Sources" toggle button proves the source events were
    // received and rendered as native citations (Citations.svelte).
    const sourcesToggle = page
      .getByRole('button', { name: /toggle \d+ sources?|toggle 1 source/i })
      .or(page.getByRole('button', { name: /^\s*\d+ sources?\s*$|^\s*1 source\s*$/i }))
      .first();
    await expect(sourcesToggle, 'expected an OWUI "{N} Sources" citation toggle').toBeVisible({
      timeout: 30_000,
    });
    await shot(page, '07-sources-toggle');

    // (b) Expand it → numbered per-source entries (id="source-...").
    await sourcesToggle.click();
    const sourceEntry = page.locator('[id^="source-"]').first();
    await expect(sourceEntry, 'expected numbered source entries after expanding').toBeVisible({
      timeout: 10_000,
    });
    await shot(page, '08-sources-expanded');

    // (c) Click a source → CitationModal opens INLINE (same tab) with the
    // snippet text. Capture any popup to assert no new tab opens.
    const pagesBefore = page.context().pages().length;
    let popupOpened = false;
    page.context().once('page', () => {
      popupOpened = true;
    });

    await sourceEntry.click();

    // The modal is an in-app dialog rendering the snippet via <Markdown>.
    const modal = page
      .locator('[role="dialog"], dialog, [id^="citation-"]')
      .filter({ hasText: /.+/ })
      .first();
    await expect(modal, 'expected an in-app citation modal (inline, not a new tab)').toBeVisible({
      timeout: 10_000,
    });
    await page.waitForTimeout(800);
    await shot(page, '09-citation-modal');

    const modalText = await modal.innerText().catch(() => '');
    const pagesAfter = page.context().pages().length;

    console.log('===== OWUI INLINE CITATION VERIFICATION =====');
    console.log(`model=${MODEL} question="${QUESTION}"`);
    console.log(`answer length=${bodyText.trim().length}`);
    console.log(`Sources toggle visible=true`);
    console.log(`modal opened inline=true; modal text length=${modalText.trim().length}`);
    console.log(`new tab opened=${popupOpened} (pages ${pagesBefore} -> ${pagesAfter})`);
    console.log('modal head:', JSON.stringify(modalText.slice(0, 300)));
    console.log('=============================================');

    // (d) The modal must show real snippet content, and clicking the citation
    // must NOT have opened a new browser tab.
    expect(modalText.trim().length, 'citation modal should show snippet text').toBeGreaterThan(20);
    expect(popupOpened, 'clicking a citation must not open a new tab').toBe(false);
    expect(pagesAfter, 'no extra browser tab should be created').toBe(pagesBefore);

    // --- 7. Verify SECTION PROVENANCE renders in a citation modal ----------
    // The fresh ingest (beforeAll) carries persisted ChunkMetadata, so a
    // section-bearing chunk's snippet markdown includes a "**Section:** …" line
    // that OWUI renders as a bold "Section:" label. Only the fresh doc has
    // metadata, so any "Section:" we see proves the persist→hydrate→render
    // round-trip. Whether the ONE retrieved chunk happens to carry a
    // section_title is non-deterministic (LLM chunker + BM25 ranking), so we
    // probe a few section-targeted questions until a section-bearing source
    // surfaces. If the feature regresses, none will and this fails.
    await page.keyboard.press('Escape').catch(() => {});
    await page.waitForTimeout(400);

    let provenanceModalText = await scanLastSourcesForSection();
    const followups = [
      'Quiz Time',
      'ตารางธุรกิจต้องห้ามและพึงระมัดระวังอย่างยิ่ง',
    ];
    for (const q of followups) {
      if (provenanceModalText) break;
      await askQuestion(q);
      await page.waitForTimeout(1500);
      provenanceModalText = await scanLastSourcesForSection();
    }
    await shot(page, '10-section-provenance');

    const sawSection = /Section:/i.test(provenanceModalText);
    console.log('===== OWUI SECTION PROVENANCE VERIFICATION =====');
    console.log(`section provenance rendered=${sawSection}`);
    console.log('provenance modal head:', JSON.stringify(provenanceModalText.slice(0, 300)));
    console.log('================================================');

    expect(
      sawSection,
      'expected a citation modal to render "Section:" provenance from the freshly-ingested doc',
    ).toBe(true);
  });
});
