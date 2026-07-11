import { test, expect, type Page } from '@playwright/test';
import { login } from './helpers';

const COMPOSER = 'Ask anything about your documents…';

// Live-stack specs for the pre-retrieval document-operations path: a bare
// "สรุปเอกสารนี้ให้หน่อย" must never dead-end in the low-relevance refusal.
// Requires the stack up with the standard e2e corpus:
//   - workspace "E2E-Scanned"  → exactly 1 ready document (summarize directly)
//   - workspace "Table-Eval"   → several ready documents (clarify, list titles)

async function pickScope(page: Page, label: string) {
  // The scope selector is an antd Select showing the current scope label.
  await page.getByText('All my workspaces', { exact: true }).click();
  // Type-to-filter: the dropdown virtualizes once many workspaces exist.
  await page.keyboard.type(label);
  await page
    .locator('.ant-select-dropdown:visible')
    .getByText(label, { exact: true })
    .click();
}

async function newChat(page: Page) {
  await page.getByRole('button', { name: 'New chat' }).click();
}

async function send(page: Page, prompt: string) {
  await page.getByPlaceholder(COMPOSER).fill(prompt);
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible();
}

async function waitForAnswer(page: Page, timeout = 200_000): Promise<string> {
  // Stream end = composer re-enables (same signal as chat-smoke).
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout });
  const assistant = page.getByTestId('msg-assistant').last();
  await expect(assistant).toBeVisible();
  await expect
    .poll(async () => (await assistant.innerText()).trim().length, { timeout: 10_000 })
    .toBeGreaterThan(0);
  return (await assistant.innerText()).trim();
}

test('bare Thai summarize in a single-doc scope returns a real summary, not a refusal', async ({
  page,
}) => {
  test.setTimeout(240_000);
  await login(page);
  await newChat(page);
  await pickScope(page, 'E2E-Scanned');

  await send(page, 'สรุปเอกสารนี้ให้หน่อย');
  const answer = await waitForAnswer(page);

  // The old dead end — must be gone.
  expect(answer).not.toContain('ไม่พบข้อมูล');
  expect(answer).not.toContain('ไม่เพียงพอ');
  // A summary of a real document is substantive.
  expect(answer.length).toBeGreaterThan(50);
});

test('bare summarize in a multi-doc scope asks which document, listing titles', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await login(page);
  await newChat(page);
  await pickScope(page, 'Table-Eval');

  await send(page, 'สรุปเอกสารนี้ให้หน่อย');
  const answer = await waitForAnswer(page);

  // Deterministic clarification (no LLM): asks which one and lists real titles.
  expect(answer).toContain('ฉบับไหน');
  expect(answer).toContain('rd_tp4_table.pdf');
  expect(answer).not.toContain('ไม่พบข้อมูล');
});

test('content questions still route through ordinary RAG (no doc-op hijack)', async ({
  page,
}) => {
  // Full-RAG generation can queue behind bulk-lane ingestion in-suite.
  test.setTimeout(600_000);
  await login(page);
  await newChat(page);
  await pickScope(page, 'Table-Eval');

  // Carries an op token ("สรุป") but real content terms — must NOT clarify.
  await send(page, 'สรุปอัตราภาษีร้อยละของ ภ.ง.ด.53 ให้หน่อย');
  const answer = await waitForAnswer(page, 400_000);

  expect(answer).not.toContain('ฉบับไหน');
});
