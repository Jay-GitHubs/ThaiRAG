import { test, expect, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { TEST_EMAIL, TEST_PASSWORD, API_BASE, pinSharedModel, setSharedModel } from './helpers';

/**
 * Gap-2 regression spec (headed): an Open WebUI file upload on a topic that is
 * ABSENT from ThaiRAG's own knowledge base must still be answered.
 *
 * Why this exists: OWUI embeds an uploaded file with its OWN local embedder,
 * retrieves the top-k chunks, and injects them as a `system` message in the
 * request it streams to ThaiRAG. Before the fix, ThaiRAG's streaming pipeline
 * ran a pre-stream "empty knowledge base" guard (`context_insufficient_response`)
 * that short-circuited with a canned refusal whenever ThaiRAG's OWN KM retrieval
 * came back empty — which it always does for a topic only present in the
 * user-uploaded file. So the user uploaded a file, asked about it, and got
 * "I don't have enough information in the knowledge base" while the model never
 * even saw the injected context. The non-streaming path had no such guard, so
 * stream/non-stream disagreed.
 *
 * The fix suppresses that short-circuit when the inbound request carries
 * client-supplied context (a non-blank system message), restoring parity. This
 * spec proves it end-to-end through the real OWUI browser UI: upload a file
 * containing a unique invented fact about a made-up topic, ask for that fact,
 * and assert the answer is NOT the refusal (and ideally surfaces the fact).
 *
 * Run headed:
 *   npx playwright test e2e/owui-file-upload-novel-topic.spec.ts --headed --project=e2e
 */

const OWUI = process.env.OWUI_URL ?? 'http://localhost:3000';
const KC_USER = process.env.OWUI_USER ?? 'testuser';
const KC_PASS = process.env.OWUI_PASS ?? 'test123';
const MODEL = process.env.OWUI_MODEL ?? 'ThaiRAG-1.0';
const SHOTS = 'e2e/screenshots/owui-file-upload-novel';

// A topic that is guaranteed NOT to be in ThaiRAG's KM, with one unique fact the
// answer must reproduce. The token "Zorblax" and the number 42 make the answer
// unambiguously sourced from the uploaded file rather than general knowledge.
const TOPIC = 'Zorblax Protocol';
const UNIQUE_NUMBER = '42';
const FILE_TEXT = [
  'INTERNAL ENGINEERING MEMO — THE ZORBLAX PROTOCOL',
  '',
  'The Zorblax Protocol is a fictional internal calibration standard.',
  `Rule 1: Every flux capacitor governed by the Zorblax Protocol MUST be`,
  `recalibrated exactly every ${UNIQUE_NUMBER} hours.`,
  'Rule 2: The recalibration must be logged by the Zorblax Compliance Officer.',
  'Rule 3: A missed recalibration window voids the Zorblax warranty.',
  '',
  `In short: the mandatory Zorblax recalibration interval is ${UNIQUE_NUMBER} hours.`,
].join('\n');

const QUESTION =
  'According to the uploaded Zorblax Protocol memo, how many hours apart must each flux capacitor be recalibrated? Answer using only the document.';

const REFUSAL_MARKERS = [
  "don't have enough information in the knowledge base",
  "don’t have enough information in the knowledge base",
  "don't appear to be relevant to your question",
  "don’t appear to be relevant to your question",
  // Thai variants — the guard localizes its refusals to the query language.
  'ข้อมูลไม่เพียงพอในฐานความรู้',
  'ไม่พบข้อมูลที่เกี่ยวข้องกับคำถาม',
];

mkdirSync(SHOTS, { recursive: true });

async function shot(page: Page, name: string) {
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
  console.log(`[gap2] screenshot -> ${SHOTS}/${name}.png`);
}

function writeFixture(): string {
  const dir = path.join(os.tmpdir(), 'thairag-gap2');
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `zorblax-protocol-${Date.now()}.txt`);
  writeFileSync(file, FILE_TEXT, 'utf8');
  return file;
}

test.describe('OWUI file upload on a topic absent from ThaiRAG KM', () => {
  test.setTimeout(15 * 60_000);

  // ThaiRAG answers OWUI's request with the global chat model. Pin a known-pulled
  // one (via the admin API) so this spec is independent of suite ordering — an
  // earlier spec can leave a leaked/unpulled model that would 404 the call.
  let token: string;
  let prevModel: string | undefined;

  test.beforeAll(async ({ request }) => {
    token = (
      await (
        await request.post(`${API_BASE}/api/auth/login`, {
          data: { email: TEST_EMAIL, password: TEST_PASSWORD },
        })
      ).json()
    ).token;
    prevModel = await pinSharedModel(request, token);
  });

  test.afterAll(async ({ request }) => {
    if (prevModel) await setSharedModel(request, token, prevModel);
  });

  test('uploaded-file question is answered, not refused with empty-KB message', async ({ page }) => {
    const fixture = writeFixture();
    console.log(`[gap2] fixture -> ${fixture}`);

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

    // --- 3. Upload the file into the chat ----------------------------------
    // OWUI keeps a hidden <input type="file"> in the composer; setting files on
    // it triggers the upload + OWUI-side embedding without needing the menu.
    const fileInput = page.locator('input[type="file"]').first();
    await expect(fileInput, 'expected an OWUI file input in the composer').toBeAttached({
      timeout: 15_000,
    });
    await fileInput.setInputFiles(fixture);
    console.log('[gap2] file set on composer input');

    // Wait for OWUI to finish processing the attachment: the upload spinner
    // disappears and a file chip is shown. We poll for the chip and then give
    // OWUI a moment to finish embedding before sending.
    const fileChip = page.getByText(/zorblax-protocol/i).first();
    await expect(fileChip, 'expected the uploaded file chip to appear').toBeVisible({
      timeout: 60_000,
    });
    // Let any "uploading"/"processing" indicator settle.
    await page.waitForTimeout(5000);
    await shot(page, '02-file-attached');

    // --- 4. Ask the question -----------------------------------------------
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

    // --- 5. Wait for the answer to settle ----------------------------------
    const MSG_SEL = '[class*="message"], [data-message-role="assistant"], .chat-assistant';
    await expect
      .poll(async () => (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim().length, {
        timeout: 8 * 60_000,
        intervals: [2000],
      })
      .toBeGreaterThan(10);

    let prev = '';
    let stableCount = 0;
    const deadline = Date.now() + 8 * 60_000;
    while (Date.now() < deadline) {
      await page.waitForTimeout(1500);
      const cur = (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim();
      if (cur.length > 10 && cur === prev) {
        stableCount += 1;
        if (stableCount >= 4) break;
      } else {
        stableCount = 0;
      }
      prev = cur;
    }
    await page.waitForTimeout(2000);
    await shot(page, '03-answer');

    const answer = (await page.locator(MSG_SEL).last().innerText().catch(() => '')).trim();
    console.log(`[gap2] final answer:\n${answer}`);

    // --- 6. Assertions -----------------------------------------------------
    const lower = answer.toLowerCase();

    // The core regression assertion: it must NOT be the empty-KB refusal.
    for (const marker of REFUSAL_MARKERS) {
      expect(
        lower.includes(marker.toLowerCase()),
        `answer must not be the empty-KB refusal — found "${marker}"`,
      ).toBe(false);
    }

    // And it should actually be sourced from the uploaded file: the unique fact.
    expect(
      answer.length > 0,
      'expected a non-empty answer streamed from the injected file context',
    ).toBe(true);
    expect(
      lower.includes(UNIQUE_NUMBER) || lower.includes('zorblax'),
      `answer should reflect the uploaded file (expected "${UNIQUE_NUMBER}" or "Zorblax")`,
    ).toBe(true);

    console.log('===== GAP-2 CONFIRMED FIXED: file-upload question answered =====');
    console.log(`topic=${TOPIC} unique_number=${UNIQUE_NUMBER}`);
    console.log('===============================================================');
  });
});
