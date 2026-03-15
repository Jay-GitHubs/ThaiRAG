import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

test.describe('Quick Setup Presets', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.waitForTimeout(1000);
  });

  test('shows Chat and Document preset sections', async ({ page }) => {
    const panel = page.getByRole('tabpanel');

    // Dividers
    await expect(panel.locator('.ant-divider').filter({ hasText: 'Chat & Response Pipeline' })).toBeVisible();
    await expect(panel.locator('.ant-divider').filter({ hasText: 'Document Processing' })).toBeVisible();

    // Chat preset cards
    await expect(panel.getByText('Thai Basic')).toBeVisible();
    await expect(panel.getByText('Thai Recommended')).toBeVisible();
    await expect(panel.getByText('Thai Maximum')).toBeVisible();

    // Document preset cards
    await expect(panel.getByText('Thai Doc Basic')).toBeVisible();
    await expect(panel.getByText('Thai Doc Recommended')).toBeVisible();
  });

  test('preset cards show model table with status columns', async ({ page }) => {
    const panel = page.getByRole('tabpanel');
    // Use .ant-card-small to target preset cards (not the outer wrapper card)
    const basicCard = panel.locator('.ant-card-small').filter({ hasText: 'Thai Basic' });
    await expect(basicCard).toBeVisible();

    await expect(basicCard.getByRole('columnheader', { name: 'Status' })).toBeVisible();
    await expect(basicCard.getByRole('columnheader', { name: 'Model' })).toBeVisible();
    await expect(basicCard.getByRole('columnheader', { name: 'Role' })).toBeVisible();
    await expect(basicCard.getByRole('columnheader', { name: 'Weight' })).toBeVisible();
    await expect(basicCard.getByText('iapp/chinda-qwen3-4b')).toBeVisible();
  });

  test('Thai model names appear in presets', async ({ page }) => {
    const panel = page.getByRole('tabpanel');
    await expect(panel.getByText('iapp/chinda-qwen3-4b').first()).toBeVisible();
    await expect(panel.getByText('qwen3:14b').first()).toBeVisible();
  });

  test('apply chat preset updates Chat Pipeline settings via API', async ({ page }) => {
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok()).toBeTruthy();
    const { token } = await loginRes.json();
    const headers = { Authorization: `Bearer ${token}` };

    // Apply thai-recommended
    const applyRes = await page.request.post(`${API_BASE}/api/km/settings/presets/apply`, {
      headers, data: { preset_id: 'thai-recommended', ollama_url: 'http://host.docker.internal:11434' },
    });
    expect(applyRes.ok()).toBeTruthy();
    expect((await applyRes.json()).status).toBe('applied');

    // Verify chat pipeline
    const chatConfig = await (await page.request.get(`${API_BASE}/api/km/settings/chat-pipeline`, { headers })).json();
    expect(chatConfig.enabled).toBe(true);
    expect(chatConfig.llm_mode).toBe('shared');
    expect(chatConfig.llm?.model).toBe('qwen3:14b');

    // Verify embedding updated
    const provConfig = await (await page.request.get(`${API_BASE}/api/km/settings/providers`, { headers })).json();
    expect(provConfig.embedding.model).toBe('qwen3-embedding:8b');
    expect(provConfig.embedding.kind).toBe('Ollama');

    // Verify UI: pipeline should be enabled
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByTestId('chat-pipeline-switch')).toHaveAttribute('aria-checked', 'true');
  });

  test('apply doc preset updates Document Processing settings via API', async ({ page }) => {
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token } = await loginRes.json();
    const headers = { Authorization: `Bearer ${token}` };

    // Apply thai-doc-recommended
    const applyRes = await page.request.post(`${API_BASE}/api/km/settings/presets/apply`, {
      headers, data: { preset_id: 'thai-doc-recommended', ollama_url: 'http://host.docker.internal:11434' },
    });
    expect(applyRes.ok()).toBeTruthy();

    // Verify doc config
    const docConfig = await (await page.request.get(`${API_BASE}/api/km/settings/document`, { headers })).json();
    expect(docConfig.ai_preprocessing.enabled).toBe(true);
    expect(docConfig.ai_preprocessing.enricher_enabled).toBe(true);
    // Main LLM should be qwen3:14b
    expect(docConfig.ai_preprocessing.llm?.model).toBe('qwen3:14b');
    // Enricher LLM should be iapp/chinda-qwen3-4b (THIS was the key bug)
    expect(docConfig.ai_preprocessing.enricher_llm?.model).toBe('iapp/chinda-qwen3-4b');

    // Verify UI
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1500);
    const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
    await expect(aiSwitch).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByTestId('enricher-switch')).toHaveAttribute('aria-checked', 'true');
  });

  test('chat preset does NOT affect document settings', async ({ page }) => {
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token } = await loginRes.json();
    const headers = { Authorization: `Bearer ${token}` };

    // Set known doc state
    await page.request.post(`${API_BASE}/api/km/settings/presets/apply`, {
      headers, data: { preset_id: 'thai-doc-recommended', ollama_url: 'http://host.docker.internal:11434' },
    });
    const docBefore = await (await page.request.get(`${API_BASE}/api/km/settings/document`, { headers })).json();

    // Apply CHAT preset
    await page.request.post(`${API_BASE}/api/km/settings/presets/apply`, {
      headers, data: { preset_id: 'thai-basic', ollama_url: 'http://host.docker.internal:11434' },
    });
    const docAfter = await (await page.request.get(`${API_BASE}/api/km/settings/document`, { headers })).json();

    expect(docAfter.ai_preprocessing.enabled).toBe(docBefore.ai_preprocessing.enabled);
    expect(docAfter.ai_preprocessing.enricher_enabled).toBe(docBefore.ai_preprocessing.enricher_enabled);
    expect(docAfter.ai_preprocessing.enricher_llm?.model).toBe(docBefore.ai_preprocessing.enricher_llm?.model);
  });

  test('doc preset does NOT affect chat pipeline settings', async ({ page }) => {
    const loginRes = await page.request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    const { token } = await loginRes.json();
    const headers = { Authorization: `Bearer ${token}` };

    // Set known chat state
    await page.request.post(`${API_BASE}/api/km/settings/presets/apply`, {
      headers, data: { preset_id: 'thai-recommended', ollama_url: 'http://host.docker.internal:11434' },
    });
    const chatBefore = await (await page.request.get(`${API_BASE}/api/km/settings/chat-pipeline`, { headers })).json();

    // Apply DOC preset
    await page.request.post(`${API_BASE}/api/km/settings/presets/apply`, {
      headers, data: { preset_id: 'thai-doc-basic', ollama_url: 'http://host.docker.internal:11434' },
    });
    const chatAfter = await (await page.request.get(`${API_BASE}/api/km/settings/chat-pipeline`, { headers })).json();

    expect(chatAfter.enabled).toBe(chatBefore.enabled);
    expect(chatAfter.llm_mode).toBe(chatBefore.llm_mode);
    expect(chatAfter.llm?.model).toBe(chatBefore.llm?.model);
  });
});
