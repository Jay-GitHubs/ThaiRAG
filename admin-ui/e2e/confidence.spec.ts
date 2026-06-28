import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

/**
 * No-context refusal + deterministic confidence in the admin Test Chat.
 * Live-stack gated (KMs workspace).
 *
 * - An out-of-domain query retrieves nothing semantically relevant (dense cosine
 *   below the floor), so the pipeline refuses: a neutral "No answer" marker (NO
 *   1–10 number — a refusal isn't an answer to score) and no source chips.
 * - A grounded query answers with a numeric confidence, cites its source, and
 *   exposes the factor breakdown on click.
 */
test('out-of-domain refuses with a No-answer marker; grounded scores + cites', async ({ page }) => {
  test.setTimeout(420_000);
  await login(page);
  await navigateTo(page, 'Test Chat');
  await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
  await page.getByTitle('BTUDE').click();
  await page.locator('.ant-select', { hasText: /Select Department/i }).click();
  await page.getByTitle('BA101').click();
  await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
  await page.getByTitle('KMs').click();

  const send = async (q: string) => {
    await page.getByPlaceholder('Ask a question').fill(q);
    await page.getByRole('button', { name: 'Send' }).click();
  };
  const num = (s: string) => Number(s.match(/(\d+)\s*\/\s*10/)?.[1] ?? '0');

  // Out-of-domain (Thai cooking, absent from an SME/sales KB) → refusal:
  // a "No answer" marker, no numeric confidence, no citations.
  await send('วิธีทำต้มยำกุ้งที่อร่อยต้องทำอย่างไร');
  await expect(page.getByTestId('no-answer-tag')).toHaveCount(1, { timeout: 200_000 });
  expect(await page.getByTestId('confidence-tag').count()).toBe(0);
  expect(await page.getByTestId('source-chip').count()).toBe(0);

  // Grounded → numeric confidence above the floor + cited source.
  await send('What were the Q1 and Q2 sales for the North and South regions?');
  await expect(page.getByTestId('confidence-tag')).toHaveCount(1, { timeout: 200_000 });
  expect(await page.getByTestId('source-chip').count()).toBeGreaterThan(0);
  const relConf = num(await page.getByTestId('confidence-tag').innerText());
  expect(relConf).toBeGreaterThanOrEqual(4);

  // Explainable breakdown opens on click (not hover, which blocked source clicks).
  await page.getByTestId('confidence-tag').click();
  await expect(page.getByText('Citation coverage', { exact: false })).toBeVisible({
    timeout: 10_000,
  });
});
