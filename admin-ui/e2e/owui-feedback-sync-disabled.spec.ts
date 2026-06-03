import { test, expect, request as pwRequest, type Page } from '@playwright/test';
import { mkdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { API_BASE, TEST_EMAIL, TEST_PASSWORD } from './helpers';

/**
 * OWUI → ThaiRAG feedback-sync DISABLED-BY-DEFAULT verification spec (headed).
 *
 * The mirror image of owui-feedback-sync.spec.ts. That spec proves the round-trip
 * works when the sync is ON; this one proves the strict no-op when it is OFF (the
 * default — `THAIRAG__OWUI_FEEDBACK_SYNC__ENABLED=false`):
 *
 *   1. A user gives a real thumbs-up in Open WebUI (localhost:3000).
 *   2. OWUI persists the rating with `thairag_response_id` in its snapshot —
 *      confirmed via OWUI's admin export endpoint (so we KNOW there is a live row
 *      that an enabled poller would import).
 *   3. ThaiRAG's poller never runs, so after well over one poll interval the
 *      response_id MUST NOT appear in ThaiRAG's feedback log.
 *
 * This guards the guarantee-or-drop property: flag off = no import, no matter what
 * ratings exist on the OWUI side.
 *
 * Requires the OWUI admin API key (export endpoint is admin-gated). Without it the
 * test skips rather than failing.
 *
 * Run headed:
 *   OWUI_ADMIN_API_KEY=<key> npx playwright test e2e/owui-feedback-sync-disabled.spec.ts --headed --project=e2e
 */

const OWUI = process.env.OWUI_URL ?? 'http://localhost:3000';
const KC_USER = process.env.OWUI_USER ?? 'testuser';
const KC_PASS = process.env.OWUI_PASS ?? 'test123';
const MODEL = process.env.OWUI_MODEL ?? 'ThaiRAG-1.0';
const QUESTION = process.env.OWUI_QUESTION ?? 'ธุรกิจต้องห้ามมีอะไรบ้าง';
const ADMIN_KEY = process.env.OWUI_ADMIN_API_KEY ?? '';
const SHOTS = 'e2e/screenshots/owui-feedback-disabled';

// Same VIEWER-scoped workspace the citations spec ingests into — a doc must be
// ingested HERE for OWUI retrieval (and thus a thairag_response_id) to exist.
const INGEST_WS = process.env.OWUI_INGEST_WS ?? 'b5ce5fad-ee71-4b8c-aebf-fa28302722d5';
const FIXTURE_PDF = path.resolve(process.cwd(), '../tests/fixtures/test-from-powerpoint.pdf');

// How long to wait, after OWUI has the rating, before asserting ThaiRAG never
// imported it. Generously larger than the test-compose interval (15s) so a
// running poller would have fired several times.
const NOOP_GUARD_MS = Number(process.env.OWUI_NOOP_GUARD_MS ?? 60_000);

mkdirSync(SHOTS, { recursive: true });

async function shot(page: Page, name: string) {
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
  console.log(`[fb-off] screenshot -> ${SHOTS}/${name}.png`);
}

type FeedbackEntry = {
  response_id: string;
  thumbs_up: boolean;
  query?: string | null;
  workspace_id?: string | null;
  doc_ids: string[];
  chunk_scores: number[];
  chunk_ids: string[];
};

test.describe('OWUI → ThaiRAG feedback sync (disabled by default)', () => {
  test.setTimeout(15 * 60_000);

  let adminToken = '';
  let provenanceDocId = '';

  test.beforeAll(async ({ request }) => {
    test.setTimeout(9 * 60_000);

    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok(), 'admin API login should succeed').toBeTruthy();
    adminToken = (await loginRes.json()).token;
    const headers = { Authorization: `Bearer ${adminToken}` };

    const uploadRes = await request.post(
      `${API_BASE}/api/km/workspaces/${INGEST_WS}/documents/upload`,
      {
        headers,
        multipart: {
          title: `owui-feedback-disabled-${Date.now()}`,
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
    console.log(`[fb-off] ingested doc=${provenanceDocId} status=${uploaded.status}`);

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
    console.log(`[fb-off] doc ready=${provenanceDocId}`);
  });

  test.afterAll(async ({ request }) => {
    if (!provenanceDocId || !adminToken) return;
    await request
      .delete(`${API_BASE}/api/km/workspaces/${INGEST_WS}/documents/${provenanceDocId}`, {
        headers: { Authorization: `Bearer ${adminToken}` },
      })
      .catch(() => {});
  });

  test('thumbs-up in OWUI is NOT imported while sync is disabled', async ({ page }) => {
    test.skip(
      !ADMIN_KEY,
      'OWUI_ADMIN_API_KEY not set — needed to read OWUI’s admin-gated feedback export',
    );

    const api = await pwRequest.newContext();
    const fetchEntries = async (): Promise<FeedbackEntry[]> => {
      const res = await api.get(
        `${API_BASE}/api/km/settings/feedback/entries?filter=all&limit=200&offset=0`,
        { headers: { Authorization: `Bearer ${adminToken}` } },
      );
      if (!res.ok()) return [];
      return ((await res.json()).entries ?? []) as FeedbackEntry[];
    };

    // --- 0. Baseline: which response_ids already exist on the ThaiRAG side ---
    const baseline = new Set((await fetchEntries()).map((e) => e.response_id));
    console.log(`[fb-off] baseline ThaiRAG feedback entries=${baseline.size}`);

    // --- 1. OWUI login (Keycloak SSO) --------------------------------------
    await page.goto(OWUI, { waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(2000);

    const alreadyIn = await page
      .locator('#chat-input, [contenteditable="true"], textarea')
      .first()
      .isVisible()
      .catch(() => false);

    if (!alreadyIn) {
      const ssoButton = page
        .getByRole('button', { name: /keycloak|continue with|sign in with|oauth|sso/i })
        .or(page.getByRole('link', { name: /keycloak|continue with|sign in with/i }))
        .first();
      await expect(ssoButton, 'expected an SSO/Keycloak login button').toBeVisible({ timeout: 15_000 });
      await ssoButton.click();

      await page.waitForSelector('#username', { timeout: 20_000 });
      await page.fill('#username', KC_USER);
      await page.fill('#password', KC_PASS);
      await Promise.all([
        page.waitForURL((url) => url.toString().startsWith(OWUI), { timeout: 30_000 }).catch(() => {}),
        page.click('#kc-login, button[type="submit"], input[type="submit"]'),
      ]);
      await page.waitForTimeout(3000);
    }

    await page
      .getByRole('button', { name: /okay|got it|close|dismiss|confirm/i })
      .first()
      .click({ timeout: 3000 })
      .catch(() => {});

    // --- 2. Ensure the ThaiRAG model is selected ---------------------------
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
    await shot(page, '01-chat-ready');

    // --- 3. Ask the question and wait for the answer to settle -------------
    const MSG_SEL = '[class*="message"], [data-message-role="assistant"], .chat-assistant';
    const chatInput = () =>
      page
        .locator('#chat-input')
        .or(page.locator('[contenteditable="true"]'))
        .or(page.getByRole('textbox'))
        .first();

    const input = chatInput();
    await expect(input, 'expected a chat input box').toBeVisible({ timeout: 15_000 });
    await input.click();
    await input.fill(QUESTION);
    await page.keyboard.press('Enter');

    await expect
      .poll(async () => (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim().length, {
        timeout: 8 * 60_000,
        intervals: [2000],
      })
      .toBeGreaterThan(20);

    let prev = '';
    let stableCount = 0;
    const deadline = Date.now() + 8 * 60_000;
    while (Date.now() < deadline) {
      await page.waitForTimeout(1500);
      const cur = (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim();
      if (cur.length > 20 && cur === prev) {
        stableCount += 1;
        if (stableCount >= 4) break;
      } else {
        stableCount = 0;
      }
      prev = cur;
    }
    await page.waitForTimeout(2000);
    await shot(page, '02-answer');

    // --- 4. Click thumbs-up on the last assistant message ------------------
    const lastMsg = page.locator(MSG_SEL).last();
    await lastMsg.hover().catch(() => {});
    await page.waitForTimeout(500);

    const goodResponse = page
      .getByRole('button', { name: /good response/i })
      .or(page.locator('button[aria-label*="good response" i]'))
      .or(page.locator('button[title*="good response" i]'))
      .last();

    await expect(goodResponse, 'expected an OWUI "Good Response" (thumbs-up) button').toBeVisible({
      timeout: 20_000,
    });
    await goodResponse.click();
    await page.waitForTimeout(1000);
    await page
      .getByRole('button', { name: /^save$|^submit$|^send$/i })
      .first()
      .click({ timeout: 2500 })
      .catch(() => {});
    await shot(page, '03-thumbs-up');
    console.log('[fb-off] thumbs-up registered in OWUI');

    // --- 5. Confirm OWUI persisted a NEW rating (the bait an enabled poller --
    //        would import). This proves the negative result below is the sync
    //        being OFF, not the absence of any importable rating.
    let owuiResponseId = '';
    await expect
      .poll(
        async () => {
          const res = await api.get(`${OWUI}/api/v1/evaluations/feedbacks/all/export`, {
            headers: { Authorization: `Bearer ${ADMIN_KEY}` },
          });
          if (!res.ok()) return '';
          const rows = (await res.json()) as Array<Record<string, any>>;
          for (const fb of rows) {
            if (fb?.data?.rating !== 1) continue;
            const messageId: string | undefined = fb?.meta?.message_id;
            if (!messageId) continue;
            const history = fb?.snapshot?.chat?.chat?.history ?? fb?.snapshot?.chat?.history;
            const rid = history?.messages?.[messageId]?.usage?.thairag_response_id;
            if (typeof rid === 'string' && rid.length > 0 && !baseline.has(rid)) {
              owuiResponseId = rid;
              return rid;
            }
          }
          return '';
        },
        { timeout: 90_000, intervals: [3000] },
      )
      .not.toBe('');
    console.log(`[fb-off] OWUI has an importable rating: response_id=${owuiResponseId}`);

    // --- 6. The assertion: across a window well over the poll interval, the --
    //        response_id must NEVER surface on the ThaiRAG side. If a disabled
    //        poller secretly ran, the entry would appear within ~15s.
    const guardDeadline = Date.now() + NOOP_GUARD_MS;
    while (Date.now() < guardDeadline) {
      const leaked = (await fetchEntries()).some((e) => e.response_id === owuiResponseId);
      expect(leaked, `sync is disabled — ${owuiResponseId} must not be imported`).toBe(false);
      await page.waitForTimeout(5000);
    }

    console.log('===== FEEDBACK SYNC CONFIRMED DISABLED (no import) =====');
    console.log(`owui_response_id=${owuiResponseId}`);
    console.log(`waited_ms=${NOOP_GUARD_MS} thairag_import=none`);
    console.log('========================================================');

    await api.dispose();
  });
});
