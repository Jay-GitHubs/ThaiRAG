import { test, expect } from '@playwright/test';
import { login } from './helpers';

/**
 * Batch-B features: UI locale switch (EN ⇄ ไทย) and TeX math rendering.
 * Live-stack gated like the rest of the suite. Each test runs in a fresh
 * browser context, so a locale toggled here never leaks into other specs.
 */

const COMPOSER_EN = 'Ask anything about your documents…';
const COMPOSER_TH = 'ถามอะไรก็ได้เกี่ยวกับเอกสารของคุณ…';

test('locale switcher swaps the UI chrome to Thai and persists across reload', async ({
  page,
}) => {
  await login(page);

  // Baseline: English chrome.
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();

  await page.getByTestId('locale-switcher').click();

  // Chrome flips to Thai — sidebar button, composer placeholder, search box.
  await expect(page.getByRole('button', { name: 'แชทใหม่' })).toBeVisible();
  await expect(page.getByPlaceholder(COMPOSER_TH)).toBeVisible();
  await expect(page.getByPlaceholder('ค้นหาบทสนทนา')).toBeVisible();

  // Persists across a reload.
  await page.reload();
  await expect(page.getByRole('button', { name: 'แชทใหม่' })).toBeVisible();

  // And switches back.
  await page.getByTestId('locale-switcher').click();
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();
  await expect(page.getByPlaceholder(COMPOSER_EN)).toBeVisible();
});

test('TeX math in an answer renders as KaTeX, not raw delimiters', async ({ page }) => {
  test.setTimeout(300_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  // General mode (no retrieval variance): ask the model to echo display math.
  const modePicker = page.getByTestId('mode-segmented');
  if (await modePicker.isVisible().catch(() => false)) {
    await modePicker.getByText('General').click();
  }

  await page
    .getByPlaceholder(COMPOSER_EN)
    .fill('Reply with exactly this markdown and nothing else: $$E = mc^2$$');
  await page.getByRole('button', { name: 'Send' }).click();

  await expect(page.getByPlaceholder(COMPOSER_EN)).toBeDisabled({ timeout: 20_000 });
  await expect(page.getByPlaceholder(COMPOSER_EN)).toBeEnabled({ timeout: 200_000 });

  // KaTeX output present, raw $$ absent.
  const answer = page.getByTestId('msg-assistant').last();
  await expect(answer.locator('.katex').first()).toBeVisible({ timeout: 10_000 });
  await expect(answer).not.toContainText('$$');
});
