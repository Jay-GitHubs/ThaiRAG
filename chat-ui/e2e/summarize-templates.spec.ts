import { test, expect } from '@playwright/test';
import { login } from './helpers';

/**
 * Batch-D features: conversation summarize and the composer prompt-template
 * picker. Live-stack gated. (The third batch-D item — thumbs feeding
 * inference_logs/auto-tuning — is backend-only, covered by integration tests.)
 */

test('summarize produces a summary modal for an existing conversation', async ({ page }) => {
  test.setTimeout(120_000);
  await login(page);

  // Open a conversation with history (seeded by earlier specs).
  await page.getByTestId('conversation-row').first().click();
  await expect(page.getByTestId('mode-bar')).toBeVisible();

  await page.getByTestId('summarize-conversation').click();
  const summary = page.getByTestId('summary-text');
  await expect(summary).toBeVisible({ timeout: 60_000 });
  expect(((await summary.innerText()) ?? '').trim().length).toBeGreaterThan(10);
});

test('prompt template from the marketplace fills the composer', async ({ page, request }) => {
  await login(page);

  // Seed a public template as the signed-in test user via the API.
  const token = await page.evaluate(() => sessionStorage.getItem('thairag-chat-token'));
  const created = await request.post('http://localhost:8080/api/km/prompts/marketplace', {
    headers: { Authorization: `Bearer ${token}` },
    data: {
      name: 'E2E loan checklist',
      description: 'e2e seeded',
      category: 'e2e',
      content: 'ต้องใช้เอกสารอะไรบ้างในการขอสินเชื่อ SME?',
      variables: [],
      is_public: true,
    },
  });
  expect(created.ok()).toBeTruthy();
  const tpl = await created.json();

  try {
    await page.reload();
    await page.getByTestId('prompt-templates').click();
    await page
      .getByTestId('prompt-template-option')
      .filter({ hasText: 'E2E loan checklist' })
      .click();
    await expect(page.getByTestId('composer-input')).toHaveValue(
      'ต้องใช้เอกสารอะไรบ้างในการขอสินเชื่อ SME?',
    );
  } finally {
    await request.delete(`http://localhost:8080/api/km/prompts/marketplace/${tpl.id}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
  }
});
