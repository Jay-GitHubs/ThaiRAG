import { test, expect } from '@playwright/test';
import { login } from './helpers';

const COMPOSER = 'Ask anything about your documents…';

// UX-audit regressions (2026-07-13):
//  1. A client disconnect mid-stream (refresh/navigation) used to lose the
//     WHOLE turn — the user row was only persisted at end-of-stream inside the
//     SSE generator, which the disconnect drops. The question must survive.
//  2. Conversations had no URL: refresh always landed on "/" showing the
//     empty hero, with no way to deep-link a conversation. The open
//     conversation must be reflected as /c/{id} and restored from it.

test('refresh mid-stream keeps the question and the conversation URL restores it', async ({
  page,
}) => {
  await login(page);

  await page.getByRole('button', { name: 'New chat' }).click();

  const prompt = `refresh-interrupt probe ${Date.now()}`;
  await page.getByPlaceholder(COMPOSER).fill(prompt);
  await page.getByRole('button', { name: 'Send' }).click();

  // The user's turn renders optimistically and the stream starts.
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible();
  await expect(page.getByPlaceholder(COMPOSER)).toBeDisabled({ timeout: 20_000 });

  // The URL now carries the (lazily created) conversation id.
  await expect.poll(() => page.url(), { timeout: 10_000 }).toMatch(/\/c\/[0-9a-fA-F-]{36}$/);
  const convUrl = page.url();

  // EDGE ACTION: refresh while the answer is still streaming.
  await page.reload();

  // The same conversation is restored from the URL — and the question
  // survived the interrupted stream (it is persisted before generation).
  await expect(page).toHaveURL(convUrl, { timeout: 15_000 });
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible({
    timeout: 15_000,
  });

  // Deep link from a cold navigation also lands in the conversation.
  await page.goto('/');
  await page.goto(convUrl);
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible({
    timeout: 15_000,
  });

  // An unknown conversation id falls back to "/" instead of erroring.
  await page.goto('/c/00000000-0000-0000-0000-000000000000');
  await expect
    .poll(() => new URL(page.url()).pathname, { timeout: 15_000 })
    .not.toMatch(/^\/c\/0{8}/);
});
