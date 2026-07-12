import {
  test,
  expect,
  type Page,
  type APIRequestContext,
  request as pwRequest,
} from '@playwright/test';
import { login, navigateTo, API_BASE, TEST_EMAIL, TEST_PASSWORD } from './helpers';

// Phase-4 settings redesign: each "path" owns its own vision model.
//  - Document Processing  → global  doc_vision_llm   (PUT /api/km/settings/providers)
//  - Chat & Response Pipeline → scoped chat_vision_llm (PUT /api/km/settings/chat-pipeline)
// These specs prove the editors render, save the right request shape, and persist.
//
// IMPORTANT — these tests mutate GLOBAL production settings on the live stack.
// An earlier version ended by really sending clear_doc_vision_llm:true, which
// wiped the deployment's dedicated vision model every suite run (the measured
// silent-vision-degradation failure mode; api_key is not readable via GET, so
// nothing spec-side could put it back). The rules now:
//  - the ENABLE direction saves for real (merge keeps the stored api_key), and
//    real persistence is asserted on that direction;
//  - the CLEAR/REMOVE direction is asserted via route interception and NEVER
//    reaches the backend;
//  - afterEach restores the captured original config via the merge API.

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

async function apiSession(): Promise<{ ctx: APIRequestContext; headers: Record<string, string> }> {
  const ctx = await pwRequest.newContext();
  const resp = await ctx.post(`${API_BASE}/api/auth/login`, {
    data: { email: TEST_EMAIL, password: TEST_PASSWORD },
  });
  const token = (await resp.json()).token as string;
  return { ctx, headers: { Authorization: `Bearer ${token}` } };
}

test.describe('Document Vision LLM (Document Processing tab)', () => {
  let api: { ctx: APIRequestContext; headers: Record<string, string> };
  // Original doc_vision_llm (LlmProviderInfo | null), captured before any test.
  let savedDocVision: {
    kind: string;
    model: string;
    base_url?: string | null;
    supports_vision?: boolean | null;
    ollama_num_ctx_max?: number | null;
  } | null = null;

  test.beforeAll(async () => {
    api = await apiSession();
    const cfg = await (
      await api.ctx.get(`${API_BASE}${PROVIDERS_PUT}`, { headers: api.headers })
    ).json();
    savedDocVision = cfg.doc_vision_llm ?? null;
  });

  test.afterEach(async () => {
    // Restore the production doc_vision_llm. The clear direction is mocked and
    // never hit the backend, so the enable-step merge left the original
    // api_key intact; re-sending kind/model/base_url restores faithfully.
    if (savedDocVision) {
      await api.ctx.put(`${API_BASE}${PROVIDERS_PUT}`, {
        headers: api.headers,
        data: {
          doc_vision_llm: {
            kind: savedDocVision.kind,
            model: savedDocVision.model,
            ...(savedDocVision.base_url ? { base_url: savedDocVision.base_url } : {}),
            ...(savedDocVision.supports_vision != null
              ? { supports_vision: savedDocVision.supports_vision }
              : {}),
            ...(savedDocVision.ollama_num_ctx_max != null
              ? { ollama_num_ctx_max: savedDocVision.ollama_num_ctx_max }
              : {}),
          },
        },
      });
    } else {
      await api.ctx.put(`${API_BASE}${PROVIDERS_PUT}`, {
        headers: api.headers,
        data: { clear_doc_vision_llm: true },
      });
    }
  });

  test.afterAll(async () => {
    await api.ctx.dispose();
  });

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

    // Save → expect doc_vision_llm carrying the model. This write is REAL: the
    // handler merges into the existing config, so the stored api_key survives
    // and afterEach can restore the original model over it.
    const put = capturePut(page, PROVIDERS_PUT);
    await card.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    expect(put.count()).toBeGreaterThan(0);
    const enableBody = put.last();
    expect(enableBody.doc_vision_llm).toBeTruthy();
    expect(enableBody.doc_vision_llm.model).toBe('llava:7b');
    expect(enableBody.clear_doc_vision_llm).toBeFalsy();

    // Real persistence, asserted on the non-destructive direction: reload and
    // the dedicated toggle is still ON.
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
    await openDocVision(page);
    await expect(page.getByTestId('doc-vision-switch')).toHaveAttribute('aria-checked', 'true');

    // Now turn it OFF and save → expect clear_doc_vision_llm: true. The clear
    // must NOT reach the backend (it would wipe the deployment's vision model
    // and the api_key cannot be restored from a GET), so intercept the PUT,
    // assert its shape, and fulfill with the config minus doc_vision_llm — the
    // UI drives its post-save state from the PUT response, not a refetch.
    await page.getByTestId('doc-vision-switch').click();
    await page.waitForTimeout(300);

    const current = await (
      await api.ctx.get(`${API_BASE}${PROVIDERS_PUT}`, { headers: api.headers })
    ).json();
    let clearBody: { clear_doc_vision_llm?: boolean } | null = null;
    await page.route(`**${PROVIDERS_PUT}`, async (route) => {
      if (route.request().method() !== 'PUT') return route.fallback();
      clearBody = JSON.parse(route.request().postData() ?? '{}');
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ ...current, doc_vision_llm: null }),
      });
    });

    await page.locator('.ant-card').filter({ hasText: 'Document Vision LLM' })
      .getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await page.unroute(`**${PROVIDERS_PUT}`);

    expect(clearBody).toBeTruthy();
    expect(clearBody!.clear_doc_vision_llm).toBe(true);
    // The UI applied the (mocked) cleared response: toggle lands OFF in place.
    await expect(page.getByTestId('doc-vision-switch')).toHaveAttribute('aria-checked', 'false');
  });
});

test.describe('Chat Vision LLM (Chat & Response Pipeline tab)', () => {
  let api: { ctx: APIRequestContext; headers: Record<string, string> };
  let savedLlmMode: string | null = null;
  let savedChatVision: {
    kind: string;
    model: string;
    base_url?: string | null;
  } | null = null;

  test.beforeAll(async () => {
    api = await apiSession();
    const cp = await (
      await api.ctx.get(`${API_BASE}${CHAT_PUT}`, { headers: api.headers })
    ).json();
    savedLlmMode = cp.llm_mode ?? null;
    savedChatVision = cp.chat_vision_llm ?? null;
  });

  test.afterEach(async () => {
    // The pipeline-card save submits the WHOLE form, so the enable step also
    // persists an llm_mode change ("Use Chat LLM"). Restore both it and the
    // chat vision model to their captured originals.
    const restore: Record<string, unknown> = {};
    if (savedLlmMode != null) restore.llm_mode = savedLlmMode;
    if (savedChatVision) {
      restore.chat_vision_llm = {
        kind: savedChatVision.kind,
        model: savedChatVision.model,
        ...(savedChatVision.base_url ? { base_url: savedChatVision.base_url } : {}),
      };
    } else {
      restore.remove_chat_vision_llm = true;
    }
    await api.ctx.put(`${API_BASE}${CHAT_PUT}`, { headers: api.headers, data: restore });
  });

  test.afterAll(async () => {
    await api.ctx.dispose();
  });

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

    // Turn OFF and save → remove_chat_vision_llm: true. As with doc vision, the
    // remove direction is intercepted so the real (restored-by-afterEach) state
    // never depends on this destructive write landing.
    await page.getByTestId('chat-vision-switch').scrollIntoViewIfNeeded();
    await page.getByTestId('chat-vision-switch').click();
    await page.waitForTimeout(300);

    const currentCp = await (
      await api.ctx.get(`${API_BASE}${CHAT_PUT}`, { headers: api.headers })
    ).json();
    let removeBody: { remove_chat_vision_llm?: boolean } | null = null;
    await page.route(`**${CHAT_PUT}**`, async (route) => {
      if (route.request().method() !== 'PUT') return route.fallback();
      removeBody = JSON.parse(route.request().postData() ?? '{}');
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ ...currentCp, chat_vision_llm: null }),
      });
    });

    await pipelineCard.getByRole('button', { name: 'Save' }).first().click();
    await page.waitForTimeout(1500);
    await page.unroute(`**${CHAT_PUT}**`);

    expect(removeBody).toBeTruthy();
    expect(removeBody!.remove_chat_vision_llm).toBe(true);
    // The UI applied the (mocked) removed response: toggle lands OFF in place.
    await expect(page.getByTestId('chat-vision-switch')).toHaveAttribute('aria-checked', 'false');
  });
});
