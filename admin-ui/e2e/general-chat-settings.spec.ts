import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

/**
 * General (non-RAG) chat admin settings live in the Chat & Response Pipeline
 * tab. The card lets an operator toggle the mode, edit the system prompt, set an
 * optional dedicated model, and gate image generation — all hot-reloaded.
 */
test.describe('General Chat settings card', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);
  });

  test('renders the card and reveals dependent fields on toggle', async ({ page }) => {
    const card = page.locator('.ant-card', { hasText: 'General Chat (non-RAG)' });
    await card.scrollIntoViewIfNeeded();
    await expect(card).toBeVisible();

    // The system prompt is always shown.
    await expect(card.getByText('System prompt')).toBeVisible();

    // Dedicated-model fields are hidden until the operator opts in.
    await expect(card.getByText('Provider', { exact: true })).toHaveCount(0);
    await card.getByTestId('gc-dedicated').click();
    await expect(card.getByText('Provider', { exact: true })).toBeVisible();
  });
});
