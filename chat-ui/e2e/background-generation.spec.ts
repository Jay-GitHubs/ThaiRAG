import { test, expect } from '@playwright/test';
import { login } from './helpers';

const COMPOSER = 'Ask anything about your documents…';

// Detached generation: an answer keeps generating server-side while the user
// opens another chat and asks something else; returning to the first
// conversation reattaches (or finds the finished answer). This is THE
// user-requested scenario the GenerationHub exists for.

test('answer keeps generating while the user chats elsewhere', async ({ page }) => {
  test.setTimeout(420_000);
  await login(page);

  // ── Conversation A: ask, wait for generation to actually start ──
  await page.getByRole('button', { name: 'New chat' }).click();
  const promptA = `background probe A ${Date.now()}`;
  await page.getByPlaceholder(COMPOSER).fill(promptA);
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByTestId('msg-user').filter({ hasText: promptA })).toBeVisible();
  await expect(page.getByPlaceholder(COMPOSER)).toBeDisabled({ timeout: 20_000 });

  // ── Switch away mid-generation: open a NEW chat and ask there ──
  await page.getByRole('button', { name: 'New chat' }).click();
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 15_000 });
  const promptB = `background probe B ${Date.now()}`;
  await page.getByPlaceholder(COMPOSER).fill(promptB);
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByTestId('msg-user').filter({ hasText: promptB })).toBeVisible();

  // A's row shows the busy dot while its answer generates in the background.
  const rowA = page.getByTestId('conversation-row').filter({ hasText: 'background probe A' });
  await expect(rowA.getByTestId('conv-busy')).toBeVisible({ timeout: 15_000 });

  // ── Return to A: reattach (streaming placeholder) or finished answer ──
  await rowA.click();
  await expect(page.getByTestId('msg-user').filter({ hasText: promptA })).toBeVisible({
    timeout: 15_000,
  });

  // The answer must arrive IN THIS CONVERSATION without re-asking: either we
  // reattached to the live stream, or generation already persisted. Poll the
  // last assistant bubble for real content.
  const assistant = page.getByTestId('msg-assistant').last();
  await expect
    .poll(async () => ((await assistant.innerText().catch(() => '')) ?? '').trim().length, {
      timeout: 300_000,
      intervals: [2_000],
    })
    .toBeGreaterThan(20);

  // And it must be durable: reload → both turns still there.
  await page.reload();
  await expect(page.getByTestId('msg-user').filter({ hasText: promptA })).toBeVisible({
    timeout: 15_000,
  });
  await expect
    .poll(async () => {
      const t = await page
        .getByTestId('msg-assistant')
        .last()
        .innerText()
        .catch(() => '');
      return (t ?? '').trim().length;
    }, { timeout: 30_000 })
    .toBeGreaterThan(20);
});

test('stop persists the partial answer instead of losing the turn', async ({ page }) => {
  test.setTimeout(240_000);
  await login(page);

  await page.getByRole('button', { name: 'New chat' }).click();
  const prompt = `stop probe ${Date.now()}`;
  await page.getByPlaceholder(COMPOSER).fill(prompt);
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByPlaceholder(COMPOSER)).toBeDisabled({ timeout: 20_000 });

  // Wait until at least one token has rendered so "partial" is non-empty,
  // then stop.
  const assistant = page.getByTestId('msg-assistant').last();
  await expect
    .poll(async () => ((await assistant.innerText().catch(() => '')) ?? '').trim().length, {
      timeout: 200_000,
      intervals: [1_000],
    })
    .toBeGreaterThan(0);
  await page.getByRole('button', { name: /stop/i }).click();
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 30_000 });

  // Reload FROM THE CONVERSATION URL (guard against racing the URL sync),
  // and give the cooperative cancel a beat to persist server-side.
  await expect(page).toHaveURL(/\/c\/[0-9a-fA-F-]{36}$/, { timeout: 10_000 });
  await page.waitForTimeout(2_000);
  await page.reload();
  await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible({
    timeout: 30_000,
  });
  await expect
    .poll(async () => {
      const t = await page
        .getByTestId('msg-assistant')
        .last()
        .innerText()
        .catch(() => '');
      return (t ?? '').trim().length;
    }, { timeout: 30_000 })
    .toBeGreaterThan(0);
});
