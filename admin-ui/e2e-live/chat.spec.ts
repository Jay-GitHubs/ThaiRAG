import { test, expect, CHAT_ORIGIN, API_ORIGIN, tokenFor } from './fixtures';

const COMPOSER = 'Ask anything about your documents…';

test.describe('chat UI (live)', () => {
  test('new chat → streamed answer → persists across reload → cleanup', async ({
    page,
    request,
  }) => {
    test.setTimeout(300_000);
    let conversationId: string | undefined;
    try {
      await page.goto(`${CHAT_ORIGIN}/`);
      await page.getByRole('button', { name: 'New chat' }).click();

      const prompt = `Live smoke ${Date.now().toString(36)}: what documents do you have access to?`;
      await page.getByPlaceholder(COMPOSER).fill(prompt);
      await page.getByRole('button', { name: 'Send' }).click();

      await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible({
        timeout: 20_000,
      });

      // Streaming: composer disables during generation, re-enables when done.
      await expect(page.getByPlaceholder(COMPOSER)).toBeDisabled({ timeout: 30_000 });
      await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 240_000 });

      const assistant = page.getByTestId('msg-assistant').last();
      await expect(assistant).toBeVisible();
      await expect
        .poll(async () => (await assistant.innerText()).trim().length, { timeout: 5_000 })
        .toBeGreaterThan(0);
      // RAG answers vary, so no content match — but reject provider-error
      // bubbles, which also render as assistant turns and previously passed
      // the length-only assertion.
      const ragBubble = (await assistant.innerText()).toLowerCase();
      for (const marker of ['provider error', 'request failed', 'unreachable', 'localhost']) {
        expect(ragBubble).not.toContain(marker);
      }

      // Conversation URL (post-#352 /c/{id} routes) → id for API cleanup.
      const m = page.url().match(/\/c\/([0-9a-f-]{36})/);
      conversationId = m?.[1];

      // Reload restores the conversation from the backend.
      await page.reload();
      await expect(page.getByTestId('msg-user').filter({ hasText: prompt })).toBeVisible({
        timeout: 15_000,
      });
      await expect(page.getByTestId('msg-assistant').first()).toBeVisible();
    } finally {
      const token = tokenFor(CHAT_ORIGIN);
      if (token && conversationId) {
        await request.delete(`${API_ORIGIN}/api/chat/conversations/${conversationId}`, {
          headers: { Authorization: `Bearer ${token}` },
        });
      }
    }
  });

  test('general mode (if enabled) answers without retrieval', async ({ page, request }) => {
    test.setTimeout(240_000);
    await page.goto(`${CHAT_ORIGIN}/`);
    await page.getByRole('button', { name: 'New chat' }).click();
    // The mode toggle only exists when general chat is enabled server-side.
    // It's an antd Segmented (data-testid, not radios) and the label is
    // localized — match English and Thai.
    const modeSegmented = page.getByTestId('mode-segmented');
    test.skip(
      !(await modeSegmented.isVisible().catch(() => false)),
      'general mode not enabled',
    );
    await modeSegmented.getByText(/general|ทั่วไป/i).click();

    const prompt = 'Reply with the single word: pong';
    await page.getByPlaceholder(COMPOSER).fill(prompt);
    await page.getByRole('button', { name: 'Send' }).click();
    await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 200_000 });
    const assistant = page.getByTestId('msg-assistant').last();
    // Demand the actual answer, not merely non-empty text: an LLM/provider
    // error also renders as an assistant bubble, and a length>0 assertion
    // green-lit exactly that on production ("OpenAI stream request failed:
    // ... http://localhost:11435 ..."). Never again.
    await expect
      .poll(async () => (await assistant.innerText()).toLowerCase(), { timeout: 10_000 })
      .toContain('pong');
    const bubble = (await assistant.innerText()).toLowerCase();
    for (const marker of ['provider error', 'request failed', 'unreachable', 'localhost']) {
      expect(bubble).not.toContain(marker);
    }

    const m = page.url().match(/\/c\/([0-9a-f-]{36})/);
    const token = tokenFor(CHAT_ORIGIN);
    if (token && m?.[1]) {
      await request.delete(`${API_ORIGIN}/api/chat/conversations/${m[1]}`, {
        headers: { Authorization: `Bearer ${token}` },
      });
    }
  });
});
