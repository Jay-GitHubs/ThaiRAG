import { test, expect } from '@playwright/test';
import { login } from './helpers';

/**
 * General (non-RAG) chat mode. Picking "General" on a new chat creates a
 * conversation that answers from the model's own knowledge and never searches
 * the KMs corpus — so there are no source chips, and the header makes clear it
 * isn't using the user's documents.
 */
const COMPOSER = 'Ask anything about your documents…';

test('general mode answers without retrieving the corpus (no sources)', async ({ page }) => {
  test.setTimeout(200_000);
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  // Switch the next chat to General mode.
  await page.getByTestId('mode-segmented').getByText('General', { exact: true }).click();
  // The scope selector is hidden in general mode (it never searches the corpus).
  await expect(page.getByText('Search in')).toHaveCount(0);

  await page.getByPlaceholder(COMPOSER).fill('Write a one-line Python function that adds two numbers.');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 180_000 });

  // The header clearly states it isn't using the user's documents.
  await expect(page.getByTestId('mode-bar')).toContainText('not using your documents');

  // A real answer streamed, and it cited NO corpus sources.
  const assistant = page.getByTestId('msg-assistant').last();
  await expect(assistant).toBeVisible();
  await expect
    .poll(async () => (await assistant.innerText()).trim().length, { timeout: 5_000 })
    .toBeGreaterThan(0);
  expect(await page.getByTestId('source-chip').count()).toBe(0);
});

test('image toggle stays hidden when no image model is configured', async ({ page }) => {
  // The stack's gateway has no text-to-image model, so /api/chat/features reports
  // image_generation_enabled:false and the Text/Image picker must never appear.
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByTestId('mode-segmented').getByText('General', { exact: true }).click();

  await expect(page.getByTestId('mode-segmented')).toBeVisible();
  await expect(page.getByTestId('image-mode-segmented')).toHaveCount(0);
});
