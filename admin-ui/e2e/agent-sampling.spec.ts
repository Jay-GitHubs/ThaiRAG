import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

/**
 * Verifies the per-agent "Advanced sampling" controls (temperature + max tokens)
 * added to LlmConfigForm. Exercises the shared LLM form because it always has a
 * model (a per-agent LLM only persists when a model is set).
 *
 * Round-trips TEMPERATURE through save (GET -> UI load, edit -> persist -> reload,
 * reset -> clear). Max tokens is set-only in the API (no clear flag), so it is
 * only exercised in the UI without persisting, to avoid an unrestorable mutation.
 * Original llm config is snapshotted and restored via the API in afterAll.
 */
test.describe('Agent LLM Advanced Sampling (temperature + max tokens)', () => {
  let token: string;
  let orig: { llm_mode: string; kind?: string; model?: string; temperature?: number };

  test.beforeAll(async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    token = (await res.json()).token;
    const cfg = await (
      await request.get(`${API_BASE}/api/km/settings/chat-pipeline`, {
        headers: { Authorization: `Bearer ${token}` },
      })
    ).json();
    orig = {
      llm_mode: cfg.llm_mode,
      kind: cfg.llm?.kind,
      model: cfg.llm?.model,
      temperature: cfg.llm?.temperature,
    };
    console.log(`Snapshot: mode=${orig.llm_mode} model=${orig.model} temp=${orig.temperature}`);
    expect(orig.model, 'shared LLM needs a model for this test').toBeTruthy();
  });

  test.afterAll(async ({ request }) => {
    const llm: Record<string, unknown> = { kind: orig.kind, model: orig.model };
    if (orig.temperature != null) llm.temperature = orig.temperature;
    else llm.clear_temperature = true;
    await request.put(`${API_BASE}/api/km/settings/chat-pipeline`, {
      data: { llm_mode: orig.llm_mode, llm },
      headers: { Authorization: `Bearer ${token}` },
    });
    console.log(`Restored: mode=${orig.llm_mode} temp=${orig.temperature ?? '(cleared)'}`);
  });

  async function openSharedAdvanced(
    page: import('@playwright/test').Page,
    opts: { relogin?: boolean } = {},
  ) {
    const { relogin = true } = opts;
    if (relogin) {
      await login(page);
    } else {
      // Already authenticated (in-memory token). Force a remount of the
      // pipeline card by navigating away and back, so it re-fetches config
      // from the server — this proves the value persisted, without losing
      // the in-memory token a full page reload would drop.
      await navigateTo(page, 'Dashboard');
      await page.waitForTimeout(300);
    }
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    if ((await pipelineSwitch.getAttribute('aria-checked')) !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Force Shared mode so the shared LLM form (with a model) renders.
    await page.getByText('Shared', { exact: true }).click();
    await page.waitForTimeout(300);
    await expect(page.getByText('Dedicated LLM shared by all')).toBeVisible();

    // Expand the shared form's "Advanced sampling" (first one in the DOM).
    const advHeader = page.getByText('Advanced sampling').first();
    await advHeader.scrollIntoViewIfNeeded();
    await advHeader.click();
    await page.waitForTimeout(300);

    const advItem = page.locator('.ant-collapse-item', { hasText: 'Advanced sampling' }).first();
    return {
      tempInput: advItem.locator('.ant-input-number-input').first(),
      maxInput: advItem.locator('.ant-input-number-input').last(),
      sliderHandle: advItem.locator('.ant-slider-handle').first(),
      resetBtn: advItem.getByRole('button', { name: 'Reset' }),
      pipelineCard: page.locator('.ant-card').filter({ hasText: 'Response Pipeline' }),
    };
  }

  test('loads existing temperature, edits + persists, then resets to default', async ({ page }) => {
    const expected = orig.temperature != null ? String(orig.temperature) : '';
    // The shared "Save" button can leave a lingering "saved" toast, so reading
    // the API right after a toast match can race the in-flight PUT. Wait on the
    // actual PUT /chat-pipeline response to know the write committed.
    const waitForSave = () =>
      page.waitForResponse(
        (r) => r.url().includes('/chat-pipeline') && r.request().method() === 'PUT' && r.ok(),
      );
    const { tempInput, maxInput, sliderHandle, resetBtn, pipelineCard } = await openSharedAdvanced(page);

    // 1. GET -> UI: the saved temperature shows in the control.
    await expect(tempInput).toHaveValue(expected);
    if (expected) await expect(sliderHandle).toHaveAttribute('aria-valuenow', expected);

    // 2. Max-tokens field is editable (exercise only — don't persist, no API clear).
    await maxInput.fill('1024');
    await expect(maxInput).toHaveValue('1024');
    await maxInput.fill('');

    // 3. Edit temperature to a new value.
    await tempInput.fill('0.7');
    await tempInput.blur();
    await expect(sliderHandle).toHaveAttribute('aria-valuenow', '0.7');

    // 4. Save and wait for the write to commit.
    {
      const saved = waitForSave();
      await pipelineCard.getByRole('button', { name: 'Save' }).click();
      await saved;
    }

    // 5. Server-side round-trip: API reflects the new temperature.
    const afterSave = await (
      await page.request.get(`${API_BASE}/api/km/settings/chat-pipeline`, {
        headers: { Authorization: `Bearer ${token}` },
      })
    ).json();
    expect(afterSave.llm.temperature).toBe(0.7);
    expect(afterSave.llm.max_tokens ?? null).toBeNull(); // not persisted

    // 6. Reload -> the edited value is still shown (persisted, not just in-memory).
    {
      const re = await openSharedAdvanced(page, { relogin: false });
      await expect(re.tempInput).toHaveValue('0.7');

      // 7. Reset clears it back to the model default.
      await re.resetBtn.click();
      await expect(re.tempInput).toHaveValue('');
      const saved = waitForSave();
      await re.pipelineCard.getByRole('button', { name: 'Save' }).click();
      await saved;
    }

    const afterReset = await (
      await page.request.get(`${API_BASE}/api/km/settings/chat-pipeline`, {
        headers: { Authorization: `Bearer ${token}` },
      })
    ).json();
    expect(afterReset.llm.temperature ?? null).toBeNull();
  });
});
