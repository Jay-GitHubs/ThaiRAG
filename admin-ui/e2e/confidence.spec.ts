import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

/**
 * Deterministic confidence scoring + refusal-gated citations in the admin Test
 * Chat. Live-stack gated (KMs workspace). A no-info/refusal answer scores LOW
 * and shows no source chips; a grounded answer scores higher and cites its
 * source. The confidence tag also exposes an explainable factor breakdown on
 * hover (the "show how it scored" feature).
 */
test('refusal vs grounded: confidence + citation gating', async ({ page }) => {
  test.setTimeout(420_000);
  await login(page);
  await navigateTo(page, 'Test Chat');
  await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
  await page.getByTitle('BTUDE').click();
  await page.locator('.ant-select', { hasText: /Select Department/i }).click();
  await page.getByTitle('BA101').click();
  await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
  await page.getByTitle('KMs').click();

  const ask = async (q: string, expectN: number) => {
    await page.getByPlaceholder('Ask a question').fill(q);
    await page.getByRole('button', { name: 'Send' }).click();
    await expect(page.getByTestId('confidence-tag')).toHaveCount(expectN, { timeout: 200_000 });
  };
  const num = (s: string) => Number(s.match(/(\d+)\s*\/\s*10/)?.[1] ?? '0');

  // Refusal: no source chips, low confidence.
  await ask('วิธีเข้าสู่ระบบ ของแอป Micro Pay ทำอย่างไร', 1);
  expect(await page.getByTestId('source-chip').count()).toBe(0);
  const refusalConf = num(await page.getByTestId('confidence-tag').nth(0).innerText());

  // Grounded: cites its source, higher confidence than the refusal.
  await ask('What were the Q1 and Q2 sales for the North and South regions?', 2);
  expect(await page.getByTestId('source-chip').count()).toBeGreaterThan(0);
  const relConf = num(await page.getByTestId('confidence-tag').nth(1).innerText());

  expect(refusalConf).toBeLessThan(relConf);
  expect(refusalConf).toBeLessThanOrEqual(4);

  // Explainable breakdown: hovering the grounded answer's tag shows the factors.
  await page.getByTestId('confidence-tag').nth(1).hover();
  await expect(page.getByText('Citation coverage', { exact: false })).toBeVisible({
    timeout: 10_000,
  });
});
