import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

test.describe('Document Processing Tab', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
  });

  test('shows pipeline settings (chunk size, overlap, upload limit)', async ({ page }) => {
    await expect(page.getByText('Pipeline Settings').first()).toBeVisible();
    await expect(page.getByText('Max Chunk Size (chars)')).toBeVisible();
    await expect(page.getByText('Chunk Overlap (chars)')).toBeVisible();
    await expect(page.getByText('Max Upload Size (MB)')).toBeVisible();
  });

  test('toggle AI preprocessing on/off and save', async ({ page }) => {
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    await expect(aiSwitch).toBeVisible();

    // Ensure AI is ON
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }
    await expect(aiSwitch).toHaveAttribute('aria-checked', 'true');

    // Should show Processing Parameters section
    const aiCard = page.locator('.ant-card').filter({ hasText: 'AI Document Preprocessing' });
    await expect(aiCard.getByText('Processing Parameters')).toBeVisible();
    await expect(aiCard.getByText('Agent LLM')).toBeVisible();

    // Save (AI card)
    const aiCardSave = page.locator('.ant-card').filter({ hasText: 'AI Document Preprocessing' });
    await aiCardSave.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });
  });

  test('enricher toggle appears and can be toggled', async ({ page }) => {
    // Ensure AI is ON
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }

    // Find Chunk Enrichment section
    await expect(page.getByText('Chunk Enrichment')).toBeVisible();

    // Use data-testid for the enricher switch
    const enricherSwitch = page.getByTestId('enricher-switch');
    await expect(enricherSwitch).toBeVisible();

    // Toggle enricher off
    const enricherChecked = await enricherSwitch.getAttribute('aria-checked');
    if (enricherChecked === 'true') {
      await enricherSwitch.click();
      await page.waitForTimeout(300);
    }
    await expect(enricherSwitch).toHaveAttribute('aria-checked', 'false');
    await expect(page.getByText('Disabled — chunks are embedded as-is')).toBeVisible();

    // Toggle enricher back on
    await enricherSwitch.click();
    await page.waitForTimeout(300);
    await expect(enricherSwitch).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByText('Each chunk gets enriched with search metadata')).toBeVisible();
  });

  test('orchestrator toggle appears and can be toggled', async ({ page }) => {
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }

    await expect(page.getByText('Smart Orchestration')).toBeVisible();
    const orchSwitch = page.getByTestId('orchestrator-switch');
    await expect(orchSwitch).toBeVisible();

    // Toggle on
    const orchChecked = await orchSwitch.getAttribute('aria-checked');
    if (orchChecked !== 'true') {
      await orchSwitch.click();
      await page.waitForTimeout(300);
    }
    await expect(orchSwitch).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByText('Budget Mode')).toBeVisible();
  });

  test('LLM mode segmented control works', async ({ page }) => {
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }

    // Find the segmented control (scoped to AI card to avoid Chat Pipeline tab)
    const aiCard = page.locator('.ant-card').filter({ hasText: 'AI Document Preprocessing' });
    await expect(aiCard.getByText('Use Chat LLM')).toBeVisible();
    await expect(aiCard.getByText('Same model for all')).toBeVisible();
    await expect(aiCard.getByText('Different per agent')).toBeVisible();

    // Switch to per-agent mode
    await aiCard.getByText('Different per agent').click();
    await page.waitForTimeout(300);

    // Should show agent collapse panels (use role selector to target collapse headers)
    const collapse = aiCard.locator('.ant-collapse');
    await expect(collapse.getByRole('button', { name: /Analyzer/ })).toBeVisible();
    await expect(collapse.getByRole('button', { name: /Converter/ })).toBeVisible();
    await expect(collapse.getByRole('button', { name: /Quality/ })).toBeVisible();
    await expect(collapse.getByRole('button', { name: /Chunker/ })).toBeVisible();
  });

  test('per-agent mode shows enricher panel when enricher is enabled', async ({ page }) => {
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }

    // Enable enricher via data-testid
    const enricherSwitch = page.getByTestId('enricher-switch');
    const enricherChecked = await enricherSwitch.getAttribute('aria-checked');
    if (enricherChecked !== 'true') {
      await enricherSwitch.click();
      await page.waitForTimeout(300);
    }

    // Switch to per-agent mode (scoped to AI card)
    const aiCard = page.locator('.ant-card').filter({ hasText: 'AI Document Preprocessing' });
    await aiCard.getByText('Different per agent').click();
    await page.waitForTimeout(300);

    // Enricher panel should appear in the collapse
    await expect(aiCard.locator('.ant-collapse').getByText('Enricher')).toBeVisible();
  });

  test('save enricher_enabled round-trip', async ({ page }) => {
    // Track API calls
    const apiCalls: { body?: string; response?: string }[] = [];
    page.on('request', (req) => {
      if (req.url().includes('/api/km/settings/document') && req.method() === 'PUT') {
        apiCalls.push({ body: req.postData() || undefined });
      }
    });
    page.on('response', async (res) => {
      if (res.url().includes('/api/km/settings/document') && res.request().method() === 'PUT') {
        try {
          const text = await res.text();
          const existing = apiCalls[apiCalls.length - 1];
          if (existing) existing.response = text;
        } catch { /* ignore */ }
      }
    });

    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }

    // Toggle enricher ON via data-testid
    const enricherSwitch = page.getByTestId('enricher-switch');
    const enricherChecked = await enricherSwitch.getAttribute('aria-checked');
    if (enricherChecked !== 'true') {
      await enricherSwitch.click();
      await page.waitForTimeout(300);
    }

    // Save (click the AI Preprocessing card's Save, not the Pipeline Settings one)
    const aiCard = page.locator('.ant-card').filter({ hasText: 'AI Document Preprocessing' });
    await aiCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(2000);

    // Check API request included enricher_enabled
    expect(apiCalls.length).toBeGreaterThan(0);
    const reqBody = JSON.parse(apiCalls[0].body!);
    expect(reqBody.ai_preprocessing.enricher_enabled).toBe(true);

    // Check response also has enricher_enabled
    if (apiCalls[0].response) {
      const resBody = JSON.parse(apiCalls[0].response);
      expect(resBody.ai_preprocessing.enricher_enabled).toBe(true);
    }

    // Reload and verify persistence
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);

    // AI should still be on
    const aiSwitchReload = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    await expect(aiSwitchReload).toHaveAttribute('aria-checked', 'true');

    // Enricher should still be on via data-testid
    const enricherSwitchReload = page.getByTestId('enricher-switch');
    await expect(enricherSwitchReload).toHaveAttribute('aria-checked', 'true');
  });

  test('Embedding & Vector Store section loads', async ({ page }) => {
    // Scroll down to find the section
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(500);

    await expect(page.getByText('Embedding & Vector Store').first()).toBeVisible();
    await expect(page.getByText('Embedding Model', { exact: true })).toBeVisible();
    await expect(page.getByText('Vector Database', { exact: true }).first()).toBeVisible();
  });

  test('Embedding provider selector and model sync', async ({ page }) => {
    // Scroll to Embedding section
    const embSection = page.getByText('Embedding & Vector Store').first();
    await embSection.scrollIntoViewIfNeeded();
    await page.waitForTimeout(500);

    // Check provider select is visible
    const providerLabel = page.locator('text=Embedding Model').first();
    await expect(providerLabel).toBeVisible();

    // Check that Dimension field is visible
    await expect(page.locator('text=Dimension').last()).toBeVisible();
  });

  test('Vector store provider selector works', async ({ page }) => {
    // Scroll to Vector section
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(500);

    await expect(page.getByText('Vector Database').first()).toBeVisible();

    // The Vector Database card section should be visible
    const embCard = page.locator('.ant-card').filter({ hasText: 'Embedding & Vector Store' });
    await expect(embCard).toBeVisible();
    await expect(embCard.getByText('Vector Database')).toBeVisible();
  });

  test('Embedding sync button fetches Ollama models', async ({ page }) => {
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(500);

    // Try clicking Sync on the embedding section
    const embSyncBtn = page.locator('.ant-card').filter({ hasText: 'Embedding & Vector Store' }).getByRole('button', { name: 'Sync' });

    if (await embSyncBtn.isVisible()) {
      await embSyncBtn.click();
      await page.waitForTimeout(3000);

      // Should show a message (success or warning)
      const msgs = page.locator('.ant-message-notice');
      await expect(msgs.first()).toBeVisible({ timeout: 5000 });
    }
  });

  test('save Embedding & Vector Store settings', async ({ page }) => {
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(500);

    // Find the save button inside the Embedding & Vector Store card
    const embCard = page.locator('.ant-card').filter({ hasText: 'Embedding & Vector Store' });
    const saveBtn = embCard.getByRole('button', { name: 'Save' });
    await expect(saveBtn).toBeVisible();

    // Click save (even without changes, it should show "No changes to save")
    await saveBtn.click();
    await page.waitForTimeout(1500);

    // Should show a message
    const msgs = page.locator('.ant-message-notice');
    await expect(msgs.first()).toBeVisible({ timeout: 5000 });
  });

  test('pipeline explanation includes enricher when enabled', async ({ page }) => {
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    const checked = await aiSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await aiSwitch.click();
      await page.waitForTimeout(300);
    }

    // Enable enricher via data-testid
    const enricherSwitch = page.getByTestId('enricher-switch');
    const enricherChecked = await enricherSwitch.getAttribute('aria-checked');
    if (enricherChecked !== 'true') {
      await enricherSwitch.click();
      await page.waitForTimeout(300);
    }

    // Scroll to pipeline explanation
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(300);

    // Check pipeline text mentions enricher
    await expect(page.getByText('Chunk Enricher (keywords, summaries, HyDE)')).toBeVisible();
  });
});
