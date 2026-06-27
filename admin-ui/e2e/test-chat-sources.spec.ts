import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

/**
 * Test-Chat citation parity: an answer renders as markdown with a Sources strip,
 * and clicking a source chip opens the in-app document viewer with the cited
 * passage highlighted (full-document context). Live-stack gated; uses the KMs
 * workspace (BTUDE / BA101 / KMs) which holds ingested Thai documents.
 */
test('test-chat answer shows sources that open an in-app viewer with highlight', async ({
  page,
}) => {
  test.setTimeout(220_000);
  await login(page);
  await navigateTo(page, 'Test Chat');

  await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
  await page.getByTitle('BTUDE').click();
  await page.locator('.ant-select', { hasText: /Select Department/i }).click();
  await page.getByTitle('BA101').click();
  await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
  await page.getByTitle('KMs').click();

  await page.getByPlaceholder('Ask a question').fill('กลุ่ม A มีรายการอะไรบ้างและมูลค่าเท่าไร');
  await page.getByRole('button', { name: 'Send' }).click();

  // The answer renders as markdown and parses a Sources strip of citation chips.
  await expect(page.getByTestId('source-chip').first()).toBeVisible({ timeout: 180_000 });

  // Clicking a source chip opens the in-app viewer with the cited passage shown
  // and the matching block highlighted.
  await page.getByTestId('source-chip').first().click();
  await expect(page.getByTestId('source-drawer-title')).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId('source-content')).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId('source-highlight').first()).toBeVisible({ timeout: 15_000 });
});
