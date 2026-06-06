import { test, expect, type Page } from '@playwright/test';
import { login, navigateTo } from './helpers';

// Phase-4 settings redesign: each "path" owns its own vision model.
//  - Document Processing  → global  doc_vision_llm   (PUT /api/km/settings/providers)
//  - Chat & Response Pipeline → scoped chat_vision_llm (PUT /api/km/settings/chat-pipeline)
// These specs prove the editors render, save the right request shape, and persist.

const PROVIDERS_PUT = '/api/km/settings/providers';
const CHAT_PUT = '/api/km/settings/chat-pipeline';

/** Capture the body of the next matching PUT, returning a getter resolved after the request fires. */
function capturePut(page: Page, urlFragment: string) {
  const bodies: string[] = [];
  page.on('request', (req) => {
    if (req.url().includes(urlFragment) && req.method() === 'PUT') {
      const data = req.postData();
      if (data) bodies.push(data);
    }
  });
  return {
    last: () => (bodies.length ? JSON.parse(bodies[bodies.length - 1]) : null),
    count: () => bodies.length,
  };
}

test.describe('Document Vision LLM (Document Processing tab)', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
  });

  async function openDocVision(page: Page) {
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(300);
    const header = page.getByRole('button', { name: /Document Vision LLM/ });
    await header.scrollIntoViewIfNeeded();
    // Expand the collapse if not already open.
    const expanded = await header.getAttribute('aria-expanded');
    if (expanded !== 'true') {
      await header.click();
      await page.waitForTimeout(300);
    }
  }

  test('section renders with a dedicated/primary toggle', async ({ page }) => {
    await openDocVision(page);
    const card = page.locator('.ant-card').filter({ hasText: 'Document Vision LLM' });
    await expect(card.first()).toBeVisible();
    await expect(page.getByTestId('doc-vision-switch')).toBeVisible();
  });

  test('enabling a dedicated model saves doc_vision_llm, then clearing removes it', async ({ page }) => {
    await openDocVision(page);
    const card = page.locator('.ant-card').filter({ hasText: 'Document Vision LLM' });
    const toggle = page.getByTestId('doc-vision-switch');

    // Turn ON the dedicated vision model.
    if ((await toggle.getAttribute('aria-checked')) !== 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }
    await expect(card.getByText('Vision-capable models')).toBeVisible();

    // The model field is an antd AutoComplete (combobox). Two comboboxes exist in
    // the form: [0] = Provider Select, [1] = Model. Free-text entry is allowed.
    const modelInput = card.getByRole('combobox').nth(1);
    await modelInput.click();
    await modelInput.fill('llava:7b');
    await page.waitForTimeout(200);

    // Save → expect doc_vision_llm carrying the model.
    const put = capturePut(page, PROVIDERS_PUT);
    await card.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    expect(put.count()).toBeGreaterThan(0);
    const enableBody = put.last();
    expect(enableBody.doc_vision_llm).toBeTruthy();
    expect(enableBody.doc_vision_llm.model).toBe('llava:7b');
    expect(enableBody.clear_doc_vision_llm).toBeFalsy();

    // Now turn it OFF and save → expect clear_doc_vision_llm: true (back to fallback).
    await openDocVision(page);
    await page.getByTestId('doc-vision-switch').click();
    await page.waitForTimeout(300);

    const put2 = capturePut(page, PROVIDERS_PUT);
    await page.locator('.ant-card').filter({ hasText: 'Document Vision LLM' })
      .getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);

    expect(put2.count()).toBeGreaterThan(0);
    expect(put2.last().clear_doc_vision_llm).toBe(true);

    // Persistence: reload, the toggle should be OFF.
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
    await openDocVision(page);
    await expect(page.getByTestId('doc-vision-switch')).toHaveAttribute('aria-checked', 'false');
  });
});

test.describe('Chat Vision LLM (Chat & Response Pipeline tab)', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);
  });

  async function ensurePipelineOn(page: Page) {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    if ((await pipelineSwitch.getAttribute('aria-checked')) !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }
  }

  test('vision switch renders and defaults to "Use Chat LLM"', async ({ page }) => {
    await ensurePipelineOn(page);
    const toggle = page.getByTestId('chat-vision-switch');
    await toggle.scrollIntoViewIfNeeded();
    await expect(toggle).toBeVisible();
    // When off, image questions reuse the main chat LLM.
    if ((await toggle.getAttribute('aria-checked')) === 'false') {
      await expect(page.getByText('Image questions reuse the main Chat LLM')).toBeVisible();
    }
  });

  test('enabling a dedicated chat vision model saves chat_vision_llm, then clearing removes it', async ({ page }) => {
    await ensurePipelineOn(page);
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Response Pipeline' });

    // Force "Use Chat LLM" agent mode so the only *visible* model Input is the
    // chat-vision one (shared form hidden, per-agent panels collapsed).
    await page.getByTitle('Use Chat LLM').click();
    await page.waitForTimeout(300);

    const toggle = page.getByTestId('chat-vision-switch');
    await toggle.scrollIntoViewIfNeeded();

    // Turn ON dedicated chat vision.
    if ((await toggle.getAttribute('aria-checked')) !== 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    // The LlmConfigForm appears right after the toggle. Before any sync, the model
    // field is a plain Input with placeholder "Model name"; only the vision one is visible.
    const modelInput = pipelineCard.locator('input[placeholder="Model name"]:visible');
    await modelInput.click();
    await modelInput.fill('llava:7b');
    await page.waitForTimeout(200);

    const put = capturePut(page, CHAT_PUT);
    await pipelineCard.getByRole('button', { name: 'Save' }).first().click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    expect(put.count()).toBeGreaterThan(0);
    const enableBody = put.last();
    expect(enableBody.chat_vision_llm).toBeTruthy();
    expect(enableBody.chat_vision_llm.model).toBe('llava:7b');

    // Turn OFF and save → remove_chat_vision_llm: true.
    await page.getByTestId('chat-vision-switch').scrollIntoViewIfNeeded();
    await page.getByTestId('chat-vision-switch').click();
    await page.waitForTimeout(300);

    const put2 = capturePut(page, CHAT_PUT);
    await pipelineCard.getByRole('button', { name: 'Save' }).first().click();
    await page.waitForTimeout(1500);

    expect(put2.count()).toBeGreaterThan(0);
    expect(put2.last().remove_chat_vision_llm).toBe(true);

    // Persistence: reload → toggle OFF.
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);
    await ensurePipelineOn(page);
    await page.getByTestId('chat-vision-switch').scrollIntoViewIfNeeded();
    await expect(page.getByTestId('chat-vision-switch')).toHaveAttribute('aria-checked', 'false');
  });
});
