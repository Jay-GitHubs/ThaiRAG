import { test, expect } from '@playwright/test';
import { login } from './helpers';

/**
 * Deterministic confidence scoring in the first-party chat UI. Live-stack gated
 * (KMs workspace). A grounded answer renders a confidence score, and the score
 * exposes its explainable factor breakdown on hover (the "show how it scored"
 * feature).
 *
 * The refusal-gating side (no-info answer → low score, no citations) is covered
 * on the backend by admin-ui/e2e/confidence.spec.ts; the same backend serves
 * both UIs, so this spec focuses on the chat-ui-specific rendering.
 */
const COMPOSER = 'Ask anything about your documents…';

test('grounded answer shows a confidence score with an explainable breakdown', async ({ page }) => {
  test.setTimeout(300_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  // Pin the conversation to the KMs workspace (holds the sales-table PDF).
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();

  await page
    .getByPlaceholder(COMPOSER)
    .fill('What were the Q1 and Q2 sales for the North and South regions?');
  await page.getByRole('button', { name: 'Send' }).click();

  // The stream finished when the composer re-enables; confidence rides the done
  // event, so it's present once the answer renders.
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 200_000 });
  await expect(page.getByTestId('confidence')).toHaveCount(1, { timeout: 30_000 });

  // Grounded: it cites at least one source and scores above the refusal floor.
  expect(await page.getByTestId('source-chip').count()).toBeGreaterThan(0);
  const score = Number(
    (await page.getByTestId('confidence').innerText()).match(/(\d+)\s*\/\s*10/)?.[1] ?? '0',
  );
  expect(score).toBeGreaterThanOrEqual(4);
  expect(score).toBeLessThanOrEqual(10);

  // Explainable breakdown: hovering the score shows the factors behind it (the
  // deterministic "show how" feature).
  await page.getByTestId('confidence').hover();
  await expect(page.getByText('Citation coverage', { exact: false })).toBeVisible({
    timeout: 10_000,
  });
});

test('out-of-domain query refuses with a No-answer marker (not a confidence score)', async ({
  page,
}) => {
  test.setTimeout(300_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();

  // Thai cooking is absent from the SME/sales KB → dense cosine below the floor
  // → the pipeline refuses. The turn shows a neutral "No answer" marker, NOT a
  // 1–10 confidence number, and cites nothing.
  await page
    .getByPlaceholder(COMPOSER)
    .fill('วิธีทำต้มยำกุ้งที่อร่อยต้องทำอย่างไร');
  await page.getByRole('button', { name: 'Send' }).click();

  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 200_000 });
  await expect(page.getByTestId('no-answer')).toHaveCount(1, { timeout: 30_000 });
  expect(await page.getByTestId('confidence').count()).toBe(0);
  expect(await page.getByTestId('source-chip').count()).toBe(0);
});
