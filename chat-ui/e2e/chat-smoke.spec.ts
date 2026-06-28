import { test, expect } from '@playwright/test';
import { login } from './helpers';

const COMPOSER = 'Ask anything about your documents…';

test('send a message, stream an answer, and persist it across reload', async ({ page }) => {
  await login(page);

  await page.getByRole('button', { name: 'New chat' }).click();

  const prompt = 'Hello from the chat-ui e2e smoke test';
  await page.getByPlaceholder(COMPOSER).fill(prompt);
  await page.getByRole('button', { name: 'Send' }).click();

  // The user's turn renders immediately.
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible();

  // The assistant turn streams in. Generation can be slow on a cold model, so
  // the signal that the stream finished is the composer re-enabling.
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 90_000 });

  const assistant = page.getByTestId('msg-assistant').last();
  await expect(assistant).toBeVisible();
  await expect
    .poll(async () => (await assistant.innerText()).trim().length, { timeout: 5_000 })
    .toBeGreaterThan(0);

  // The user prompt has a copy button (revealed on hover) — clicking it doesn't
  // error. (Clipboard contents aren't asserted: headless clipboard perms vary.)
  const userTurn = page.getByTestId('msg-user').filter({ hasText: prompt });
  await userTurn.hover();
  const copyPrompt = page.getByTestId('copy-prompt').first();
  await expect(copyPrompt).toBeVisible();
  await copyPrompt.click();

  // Persistence: a reload restores the conversation (and both turns) from the
  // backend — the whole point of the Phase 1/2 work.
  await page.reload();
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByTestId('msg-assistant').first()).toBeVisible();
});
