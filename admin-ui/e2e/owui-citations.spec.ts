import { test, expect, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';

/**
 * OBSERVATION spec (not a pass/fail regression gate).
 *
 * Drives the real Open WebUI chat surface (localhost:3000) end-to-end via
 * Keycloak SSO and captures how ThaiRAG citations currently render. Today the
 * backend appends a markdown "**Sources:**" footer as ordinary message content
 * (chat.rs build_source_footer), so OWUI shows it as plain text rather than its
 * native clickable citation references. These screenshots are the "before" half
 * of the citation-improvement work.
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

mkdirSync(SHOTS, { recursive: true });

async function shot(page: Page, name: string) {
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
  console.log(`[owui] screenshot -> ${SHOTS}/${name}.png`);
}

test.describe('Open WebUI citation rendering (observation)', () => {
  // The pipeline answer can be slow; give the whole flow room.
  test.setTimeout(10 * 60_000);

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

    // --- 4. Send the Thai question -----------------------------------------
    const input = page
      .locator('#chat-input')
      .or(page.locator('[contenteditable="true"]'))
      .or(page.getByRole('textbox'))
      .first();
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

    // Give OWUI a moment to render any post-completion citation panel.
    await page.waitForTimeout(2000);
    await shot(page, '06-answer-with-citations');

    // --- 6. Report what we actually got ------------------------------------
    const lastMsg = page
      .locator('[class*="message"], [data-message-role="assistant"], .chat-assistant')
      .last();
    const bodyText = await lastMsg.innerText().catch(() => '');

    const hasPlainTextSources = /\*\*?Sources:?\*\*?|Sources:|Response ID:/i.test(bodyText);
    // OWUI native citations render as small numbered reference buttons/links.
    const nativeCitationCount = await page
      .locator('button:has-text("[1]"), .citation, [id^="source-"], [data-citation], a[href^="#citation"]')
      .count()
      .catch(() => 0);

    console.log('===== OWUI CITATION OBSERVATION =====');
    console.log(`model=${MODEL} question="${QUESTION}"`);
    console.log(`answer length=${bodyText.trim().length}`);
    console.log(`plain-text "Sources:" footer present=${hasPlainTextSources}`);
    console.log(`OWUI native citation elements found=${nativeCitationCount}`);
    console.log('answer tail:', JSON.stringify(bodyText.slice(-400)));
    console.log('=====================================');

    // This is an observation test: assert only that we got an answer at all.
    expect(bodyText.trim().length).toBeGreaterThan(20);
  });
});
