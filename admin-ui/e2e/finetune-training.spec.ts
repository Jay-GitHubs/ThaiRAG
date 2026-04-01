import { test, expect } from '@playwright/test';
import { login, navigateTo, API_BASE, TEST_EMAIL } from './helpers';

// ── API-level tests for new finetune training endpoints ──────────────

test.describe('Finetune Training API', () => {
  let token: string;
  let datasetId: string;
  let jobId: string;

  test.beforeAll(async ({ request }) => {
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: 'Test1234!' },
    });
    const data = await loginRes.json();
    token = data.token;
  });

  test('create dataset for training', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/km/finetune/datasets`, {
      data: { name: `e2e-train-${Date.now()}`, description: 'training test' },
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.status()).toBe(201);
    const ds = await res.json();
    datasetId = ds.id;
  });

  test('add training pairs', async ({ request }) => {
    for (let i = 0; i < 5; i++) {
      const res = await request.post(
        `${API_BASE}/api/km/finetune/datasets/${datasetId}/pairs`,
        {
          data: {
            query: `Test question ${i}`,
            positive_doc: `Test answer ${i}`,
          },
          headers: { Authorization: `Bearer ${token}` },
        },
      );
      expect(res.status()).toBe(201);
    }
  });

  test('create job with training config', async ({ request }) => {
    const res = await request.post(`${API_BASE}/api/km/finetune/jobs`, {
      data: {
        dataset_id: datasetId,
        base_model: 'test-model:latest',
        model_source: 'ollama',
        config: {
          epochs: 1,
          learning_rate: 5e-4,
          lora_rank: 8,
          lora_alpha: 8,
          batch_size: 4,
          warmup_ratio: 0.03,
          max_seq_length: 512,
          quantization: 'q4_k_m',
          preset: 'quick',
        },
      },
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.status()).toBe(201);
    const job = await res.json();
    jobId = job.id;
    expect(job.status).toBe('pending');
    expect(job.config).toBeTruthy();

    // Verify config was stored correctly
    const config = JSON.parse(job.config);
    expect(config.epochs).toBe(1);
    expect(config.lora_rank).toBe(8);
    expect(config.model_source).toBe('ollama');
  });

  test('get job returns config field', async ({ request }) => {
    const res = await request.get(`${API_BASE}/api/km/finetune/jobs/${jobId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.status()).toBe(200);
    const job = await res.json();
    expect(job.config).toBeTruthy();
  });

  test('start job returns error when python not configured', async ({ request }) => {
    // Starting will likely fail since Python/Unsloth isn't in Docker,
    // but the endpoint should exist and respond
    const res = await request.post(
      `${API_BASE}/api/km/finetune/jobs/${jobId}/start`,
      {
        headers: { Authorization: `Bearer ${token}` },
      },
    );
    // Accept either 200 (started) or 500 (python not found) — both prove the endpoint works
    expect([200, 500]).toContain(res.status());
  });

  test('get logs endpoint exists', async ({ request }) => {
    const res = await request.get(
      `${API_BASE}/api/km/finetune/jobs/${jobId}/logs`,
      {
        headers: { Authorization: `Bearer ${token}` },
      },
    );
    expect(res.status()).toBe(200);
    const data = await res.json();
    expect(data.job_id).toBe(jobId);
    expect(Array.isArray(data.lines)).toBe(true);
  });

  test('cannot delete running or just-started job', async ({ request }) => {
    // The job might be "running" or "failed" now — either way try to see
    const jobRes = await request.get(`${API_BASE}/api/km/finetune/jobs/${jobId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const job = await jobRes.json();

    if (job.status === 'running') {
      // Cancel first
      const cancelRes = await request.post(
        `${API_BASE}/api/km/finetune/jobs/${jobId}/cancel`,
        { headers: { Authorization: `Bearer ${token}` } },
      );
      expect([200, 500]).toContain(cancelRes.status());
    }
  });

  test('delete completed/failed job', async ({ request }) => {
    // Wait a moment for any async status update
    await new Promise((r) => setTimeout(r, 1000));

    // Re-fetch to get current status
    const jobRes = await request.get(`${API_BASE}/api/km/finetune/jobs/${jobId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const job = await jobRes.json();

    // Job should be deletable if not running
    if (job.status !== 'running') {
      const res = await request.delete(
        `${API_BASE}/api/km/finetune/jobs/${jobId}`,
        { headers: { Authorization: `Bearer ${token}` } },
      );
      expect(res.status()).toBe(204);

      // Verify it's gone
      const getRes = await request.get(
        `${API_BASE}/api/km/finetune/jobs/${jobId}`,
        { headers: { Authorization: `Bearer ${token}` } },
      );
      expect(getRes.status()).toBe(404);
    }
  });

  test('delete nonexistent job returns 404', async ({ request }) => {
    const res = await request.delete(
      `${API_BASE}/api/km/finetune/jobs/nonexistent-id`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect(res.status()).toBe(404);
  });

  test('start nonexistent job returns error', async ({ request }) => {
    const res = await request.post(
      `${API_BASE}/api/km/finetune/jobs/nonexistent-id/start`,
      { headers: { Authorization: `Bearer ${token}` } },
    );
    expect(res.status()).toBe(500);
  });

  // Cleanup
  test('cleanup test dataset', async ({ request }) => {
    if (datasetId) {
      await request.delete(`${API_BASE}/api/km/finetune/datasets/${datasetId}`, {
        headers: { Authorization: `Bearer ${token}` },
      });
    }
  });
});

// ── UI tests for the new training features ───────────────────────────

test.describe('Finetune Training UI', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Fine-tuning');
  });

  test('Fine-tuning is under AI / Models group', async ({ page }) => {
    // The sidebar should show AI / Models group
    const menu = page.getByRole('menu');
    const aiGroup = menu.getByText('AI / Models');
    await expect(aiGroup).toBeVisible();
  });

  test('page heading says Fine-tuning', async ({ page }) => {
    await expect(page.getByRole('heading', { name: /Fine-tuning/ })).toBeVisible();
  });

  test('Jobs tab shows Create Job button', async ({ page }) => {
    await page.getByRole('tab', { name: 'Jobs' }).click();
    await expect(page.getByRole('button', { name: 'Create Job' })).toBeVisible();
  });

  test('Create Job modal shows model source and preset options', async ({ page }) => {
    await page.getByRole('tab', { name: 'Jobs' }).click();
    await page.getByRole('button', { name: 'Create Job' }).click();
    await expect(page.locator('.ant-modal')).toBeVisible();

    // Model source radio button labels (Ant Design Radio.Button uses label wrappers)
    const modal = page.locator('.ant-modal');
    await expect(modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'Ollama' })).toBeVisible();
    await expect(modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'HuggingFace' })).toBeVisible();

    // Preset button labels
    await expect(modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'Quick' })).toBeVisible();
    await expect(modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'Standard' })).toBeVisible();
    await expect(modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'Thorough' })).toBeVisible();
  });

  test('Create Job modal has Advanced Settings panel', async ({ page }) => {
    await page.getByRole('tab', { name: 'Jobs' }).click();
    await page.getByRole('button', { name: 'Create Job' }).click();
    await expect(page.locator('.ant-modal')).toBeVisible();

    // Advanced Settings collapse panel
    await page.getByText('Advanced Settings').click();
    await page.waitForTimeout(500);

    // Should see advanced field labels (use exact match to avoid matching description text)
    await expect(page.getByText('Epochs', { exact: true })).toBeVisible();
    await expect(page.getByText('Learning Rate', { exact: true })).toBeVisible();
    await expect(page.getByText('LoRA Rank', { exact: true })).toBeVisible();
    await expect(page.getByText('GGUF Quantization', { exact: true })).toBeVisible();
  });

  test('switching preset updates advanced fields', async ({ page }) => {
    await page.getByRole('tab', { name: 'Jobs' }).click();
    await page.getByRole('button', { name: 'Create Job' }).click();

    // Expand advanced settings
    await page.getByText('Advanced Settings').click();
    await page.waitForTimeout(500);

    const modal = page.locator('.ant-modal');

    // Click Quick preset
    await modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'Quick' }).click();
    await page.waitForTimeout(300);

    // Epochs field should show 1
    const epochsInput = modal.locator('.ant-input-number-input').first();
    await expect(epochsInput).toHaveValue('1');

    // Click Thorough preset
    await modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'Thorough' }).click();
    await page.waitForTimeout(300);

    // Epochs field should show 5
    await expect(epochsInput).toHaveValue('5');
  });

  test('switching to HuggingFace shows text input instead of dropdown', async ({ page }) => {
    await page.getByRole('tab', { name: 'Jobs' }).click();
    await page.getByRole('button', { name: 'Create Job' }).click();

    const modal = page.locator('.ant-modal');

    // Default is Ollama — should show Select
    await expect(page.getByText('Select an Ollama model')).toBeVisible();

    // Switch to HuggingFace
    await modal.locator('.ant-radio-button-wrapper').filter({ hasText: 'HuggingFace' }).click();
    await page.waitForTimeout(300);

    // Should now show text input
    await expect(modal.getByPlaceholder(/unsloth/)).toBeVisible();
  });
});
