import { test, expect } from '@playwright/test';
import { API_BASE, login, TEST_EMAIL, TEST_PASSWORD } from './helpers';

// Final OWUI-parity gate: exercises the full first-party feature set against the
// live stack. Streaming tests depend on a working chat model on the deployment.
const COMPOSER = 'Ask anything about your documents…';

/** Wait for a streamed answer to finish (the composer re-enables). */
async function waitForAnswer(page: import('@playwright/test').Page) {
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 120_000 });
}

test('login page offers SSO providers (G1)', async ({ page }) => {
  await page.goto('/login');
  await expect(page.getByRole('button', { name: /Continue with/ }).first()).toBeVisible({
    timeout: 15_000,
  });
});

test('scoped chat streams an answer with source citations (scope + citations)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  // Pin the conversation to the KMs workspace (which holds the Micro Pay manual).
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();

  await page
    .getByPlaceholder(COMPOSER)
    .fill('วิธีเข้าสู่ระบบ (log-in) ของแอป Micro Pay ทำอย่างไร');
  await page.getByRole('button', { name: 'Send' }).click();

  await waitForAnswer(page);
  await expect(page.getByTestId('msg-assistant').last()).toBeVisible();
  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });
});

test('scanned-doc answer renders inline source images (Phase 3)', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();
  await page.getByPlaceholder(COMPOSER).fill('สรุปสาระสำคัญของเอกสารนี้');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  // The cited page render shows in the Sources strip (page-image linkage).
  await expect(page.getByTestId('source-image').first()).toBeVisible({ timeout: 10_000 });
});

test('source drawer renders the original PDF (Phase 2)', async ({ page }) => {
  test.setTimeout(150_000);
  const errors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('requestfailed', (r) => errors.push(`reqfail ${r.url()} ${r.failure()?.errorText}`));

  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();
  // A born-digital PDF query, so the cited (first) source is a PDF.
  await page.getByPlaceholder(COMPOSER).fill('What were the Q1 and Q2 sales for the North and South regions?');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });

  await page.getByTestId('source-chip').first().click();
  // The drawer should default to the Document (PDF) view → the viewer mounts.
  await expect(page.getByTestId('pdf-viewer')).toBeVisible({ timeout: 10_000 });
  // The original PDF renders a canvas page (first render also cold-loads the
  // ~1.3MB PDF.js worker, so allow generous time).
  await expect(page.getByTestId('pdf-page').first(), `console/net errors: ${errors.join(' | ')}`).toBeVisible({
    timeout: 45_000,
  });
  // Toggling to Text shows the converted text instead.
  await page.getByText('Text', { exact: true }).click();
  await expect(page.getByTestId('source-content')).toBeVisible({ timeout: 10_000 });
});

test('PDF source highlights the cited passage on the page (Phase 2.1)', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();
  // Born-digital sales-table PDF (text layer) lives in KMs — its passage can be
  // located + highlighted on the PDF canvas.
  await page.getByPlaceholder(COMPOSER).fill('What were the Q1 and Q2 sales for the North and South regions?');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });

  await page.getByTestId('source-chip').first().click();
  await expect(page.getByTestId('pdf-page').first()).toBeVisible({ timeout: 45_000 });
  // The cited passage is highlighted directly on the rendered PDF page.
  await expect(page.getByTestId('pdf-highlight').first()).toBeVisible({ timeout: 15_000 });
  // The highlight must paint the dedicated --mark-overlay token — which every
  // theme defines as a translucent rgba — so the page text shows THROUGH it. An
  // opaque box (e.g. a regression back to --mark-bg) would hide the content.
  const inlineBg = await page
    .getByTestId('pdf-highlight')
    .first()
    .evaluate(
      (el) => (el as HTMLElement).style.background || (el as HTMLElement).style.backgroundColor,
    );
  expect(inlineBg).toContain('--mark-overlay');
});

test('non-PDF table source renders a table with the cited row highlighted', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();
  // complex_table.docx (a Thai/English table) lives in KMs.
  await page.getByPlaceholder(COMPOSER).fill('กลุ่ม A มีรายการอะไรบ้างและมูลค่าเท่าไร');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });

  await page.getByTestId('source-chip').first().click();
  // The converted document renders as a real table (not raw text)…
  await expect(page.locator('[data-testid="source-content"] table')).toBeVisible({ timeout: 10_000 });
  // …with the cited row highlighted.
  await expect(page.getByTestId('source-highlight').first()).toBeVisible({ timeout: 10_000 });
});

test('clicking a source opens the in-app viewer (no new tab)', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();
  await page
    .getByPlaceholder(COMPOSER)
    .fill('วิธีเข้าสู่ระบบ (log-in) ของแอป Micro Pay ทำอย่างไร');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });

  // Clicking a source chip opens the in-app drawer (PDF docs default to the
  // Document view, others to the text view) instead of navigating to a new tab.
  await page.getByTestId('source-chip').first().click();
  await expect(
    page.getByTestId('pdf-viewer').or(page.getByTestId('source-content')),
  ).toBeVisible({ timeout: 20_000 });
});

test('regenerate replaces the answer without duplicating the turn (G2)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('hello there');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  await page.getByRole('button', { name: 'Regenerate' }).click();
  await waitForAnswer(page);

  // Exactly one user + one assistant — regenerate replaced, didn't append.
  await expect(page.getByTestId('msg-user')).toHaveCount(1);
  await expect(page.getByTestId('msg-assistant')).toHaveCount(1);
});

test('first message with no conversation selected streams (regression: lazy-create abort)', async ({
  page,
  request,
}) => {
  // A brand-new account has zero conversations, so activeId is null and the
  // first send must lazily create the conversation. Regression for the bug where
  // creating it flipped activeId, firing the "abort on conversation switch"
  // effect, which aborted the just-started stream (HTTP 499) → blank forever.
  // (Clicking "New chat" first sidesteps it, which is why other tests passed.)
  const email = `lazy-${Date.now()}@test.com`;
  const password = 'Test1234!';
  const reg = await request.post(`${API_BASE}/api/auth/register`, {
    data: { email, name: 'Lazy Create', password },
  });
  expect(reg.ok() || reg.status() === 400 || reg.status() === 409).toBeTruthy();

  await page.goto('/login');
  await page.getByLabel('Email').fill(email);
  await page.getByLabel('Password').fill(password);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible({ timeout: 15_000 });

  // Send WITHOUT clicking "New chat" — exercises the lazy-create path.
  await page.getByPlaceholder(COMPOSER).fill('Hello, can you respond to this?');
  await page.getByRole('button', { name: 'Send' }).click();

  await waitForAnswer(page);
  const text = (await page.getByTestId('msg-assistant').last().innerText()).trim();
  expect(text.length, 'assistant answer must render on the first lazy-created conversation').toBeGreaterThan(0);
});

test('streaming shows pipeline progress before the answer (progress events)', async ({ page }) => {
  // The streamed answer can take >60s on a cold model; allow the full
  // waitForAnswer budget rather than hitting the default per-test timeout.
  test.setTimeout(150_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('สวัสดี ทำอะไรได้บ้าง');
  await page.getByRole('button', { name: 'Send' }).click();

  // While the pipeline runs (before any tokens), a progress indicator shows the
  // current stage instead of a blank bubble.
  await expect(page.getByTestId('msg-progress')).toBeVisible({ timeout: 15_000 });

  // It resolves into a real answer, and the progress indicator goes away.
  await waitForAnswer(page);
  await expect(page.getByTestId('msg-progress')).toHaveCount(0);
  const text = (await page.getByTestId('msg-assistant').last().innerText()).trim();
  expect(text.length).toBeGreaterThan(0);
});

test('deleting the active conversation clears the message pane', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('hello');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByTestId('msg-user')).toHaveCount(1);

  // Delete the (active) conversation from the sidebar.
  await page.locator('.anticon-delete').first().click();
  await page.getByRole('button', { name: 'Delete' }).click();

  // The active chat's messages must clear too (not just the sidebar entry).
  await expect(page.getByTestId('msg-user')).toHaveCount(0);
  await expect(page.getByTestId('msg-assistant')).toHaveCount(0);
});

test('finished answer offers a copy button (answer ergonomics)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('สวัสดี ตอบสั้น ๆ');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByTestId('copy-answer').last()).toBeVisible({ timeout: 10_000 });
});

test('edit & resend a user message replaces the turn (UX batch C)', async ({ page }) => {
  test.setTimeout(200_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  await page.getByPlaceholder(COMPOSER).fill('edittest-alpha สวัสดี ตอบสั้น ๆ');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByTestId('msg-user').filter({ hasText: 'edittest-alpha' })).toBeVisible();

  // Composer advertises drop/paste upload (batch C input power).
  await expect(page.getByText(/drop or paste files/)).toBeVisible();

  // Edit the last user message and resend the corrected prompt.
  await page.getByTestId('msg-user').last().hover();
  await page.getByTestId('edit-message').click();
  await page.getByTestId('edit-input').fill('edittest-beta สวัสดี ตอบสั้น ๆ');
  await page.getByTestId('edit-save').click();
  await waitForAnswer(page);

  // The edited prompt replaces the original — no duplicate user turn.
  await expect(page.getByTestId('msg-user').filter({ hasText: 'edittest-beta' })).toBeVisible();
  await expect(page.getByTestId('msg-user').filter({ hasText: 'edittest-alpha' })).toHaveCount(0);

  // Reload to prove the backend replaced (not orphaned) the rows: the edited
  // conversation is most-recent, so it's the top row.
  await page.reload();
  await page.getByTestId('conversation-row').first().click();
  await expect(page.getByTestId('msg-user').filter({ hasText: 'edittest-beta' })).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByTestId('msg-user').filter({ hasText: 'edittest-alpha' })).toHaveCount(0);
});

test('theme picker switches + persists, keyboard shortcuts (UX batch D)', async ({ page }) => {
  await login(page);

  // Pick a dark theme via the picker; reflected on <html> and survives reload.
  await page.getByTestId('theme-picker').click();
  await page.getByTestId('theme-option-synthwave').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'synthwave');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'synthwave');

  // "?" opens the shortcuts help.
  await page.locator('body').click();
  await page.keyboard.press('?');
  await expect(page.getByText('Keyboard shortcuts')).toBeVisible();
  await page.keyboard.press('Escape');

  // "/" focuses the message box.
  await page.locator('body').click();
  await page.keyboard.press('/');
  await expect(page.getByTestId('composer-input')).toBeFocused();

  // Restore the default theme for a clean slate.
  await page.getByTestId('theme-picker').click();
  await page.getByTestId('theme-option-celadon').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'celadon');
});

test('celadon theme variables actually apply (regression: dropped selector)', async ({ page }) => {
  await login(page);
  // Select the default theme explicitly — its vars must apply (a combined
  // `:root, :root[data-theme=celadon]` selector previously failed to, leaving the
  // whole theme's colors empty → transparent bubbles, black avatar).
  await page.getByTestId('theme-picker').click();
  await page.getByTestId('theme-option-celadon').click();
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('hello');
  await page.getByRole('button', { name: 'Send' }).click();
  // The user bubble paints var(--celadon); if the theme vars didn't apply it would
  // be transparent. (Optimistic bubble renders immediately — no need to await.)
  const bg = await page
    .getByTestId('msg-user')
    .first()
    .evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(bg).not.toBe('rgba(0, 0, 0, 0)');
  expect(bg).not.toBe('transparent');
});

test('citations stay legible under a dark theme (theme compatibility)', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);

  // Switch to a vivid dark theme, then produce a cited answer.
  await page.getByTestId('theme-picker').click();
  await page.getByTestId('theme-option-nebula').click();
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();
  await page
    .getByPlaceholder(COMPOSER)
    .fill('วิธีเข้าสู่ระบบ (log-in) ของแอป Micro Pay ทำอย่างไร');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId('source-chip').first()).toBeVisible();
  // The white-on-bright-accent bug is fixed: on bright accents, text-on-accent is dark.
  const onAccent = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue('--on-accent').trim(),
  );
  expect(onAccent).toBe('#04181d');
});

test('sidebar collapses (desktop) and the edit button shows without hover', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);

  // Collapse hides the rail and surfaces an expand control; persists across reload.
  await page.getByTestId('sidebar-collapse').click();
  await expect(page.getByTestId('sidebar-expand')).toBeVisible();
  await expect(page.getByTestId('conversation-search')).toBeHidden();
  await page.reload();
  await expect(page.getByTestId('sidebar-expand')).toBeVisible();
  await page.getByTestId('sidebar-expand').click();
  await expect(page.getByTestId('conversation-search')).toBeVisible();

  // The edit affordance is shown by default (not opacity:0) — touch users have no hover.
  await page.getByPlaceholder(COMPOSER).fill('สวัสดี ตอบสั้น ๆ');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  const opacity = await page
    .getByTestId('edit-message')
    .evaluate((el) => getComputedStyle(el).opacity);
  expect(Number(opacity)).toBeGreaterThan(0.3);
});

test('sidebar search + date grouping (UX batch B)', async ({ page }) => {
  test.setTimeout(150_000);
  await login(page);
  // Create a conversation with a unique title (robust against prior test runs).
  const tag = `srchtest-${Date.now()}`;
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill(`${tag} ตอบสั้น ๆ`);
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  // Date grouping: the just-created conversation is under a "Today" header.
  await expect(page.getByText('Today', { exact: true }).first()).toBeVisible({ timeout: 10_000 });

  // Search narrows the list to exactly the matching conversation.
  await page.getByTestId('conversation-search').fill(tag);
  await expect(page.getByTestId('conversation-row')).toHaveCount(1);
});

test('thumbs feedback persists across reload (G5)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('rate this answer');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  // Thumbs-up the answer; it survives a reload.
  await page.getByTestId('fb-up').last().click();
  await page.reload();
  await expect(page.getByTestId('msg-assistant').last()).toBeVisible({ timeout: 15_000 });
  // The filled (active) like icon renders with the celadon-deep accent colour.
  await expect(page.getByLabel('Remove positive feedback').last()).toBeVisible({ timeout: 10_000 });
});

test('stop halts a streaming answer (G2)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page
    .getByPlaceholder(COMPOSER)
    .fill('Explain this system in detail, step by step, with examples.');
  await page.getByRole('button', { name: 'Send' }).click();

  // Stop appears while streaming; clicking it ends the stream (composer re-enables).
  await page.getByRole('button', { name: 'Stop' }).click({ timeout: 30_000 });
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 30_000 });
});

test('file attachment is accepted and answered (file upload)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  await page.setInputFiles('input[type=file]', {
    name: 'note.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('The capital of the example country is Exampleville.'),
  });
  await expect(page.getByText('note.txt')).toBeVisible();

  await page.getByPlaceholder(COMPOSER).fill('What does the attached note say?');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByTestId('msg-user').filter({ hasText: 'What does the attached note say?' })).toBeVisible();
});

test('history persists across reload (persistence)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('remember this turn');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  await page.reload();
  await expect(page.getByTestId('msg-user').filter({ hasText: 'remember this turn' })).toBeVisible({
    timeout: 15_000,
  });
});

test.describe('mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('drawer navigation works on a phone viewport (G3)', async ({ page }) => {
    // Can't use login() here: on mobile the sidebar (and its New chat button) is
    // behind the drawer, so assert the composer instead as the post-login signal.
    await page.goto('/login');
    await page.getByLabel('Email').fill(TEST_EMAIL);
    await page.getByLabel('Password').fill(TEST_PASSWORD);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByPlaceholder(COMPOSER)).toBeVisible({ timeout: 15_000 });

    // The hamburger opens the conversation drawer.
    await page.getByRole('button', { name: 'Menu' }).click();
    await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();
  });
});
