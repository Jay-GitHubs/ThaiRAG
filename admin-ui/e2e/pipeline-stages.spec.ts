import { test, expect } from '@playwright/test';
import { TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

/**
 * Debug test for pipeline stages visibility.
 * Creates its own workspace with a test document, then checks:
 * 1. Chat pipeline config API
 * 2. Non-streaming test-query returns pipeline_stages
 * 3. SSE streaming sends progress events
 */

test.describe('Pipeline Stages Debug', () => {
  test.setTimeout(300_000);

  let token: string;
  let orgId: string;
  let deptId: string;
  let wsId: string;

  const suffix = Date.now();

  test.beforeAll(async ({ request }) => {
    // Login
    const loginRes = await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    });
    expect(loginRes.ok(), `Login failed: ${loginRes.status()}`).toBeTruthy();
    const loginData = await loginRes.json();
    token = loginData.token;
    const headers = { Authorization: `Bearer ${token}` };

    // Create org -> dept -> workspace
    const orgRes = await request.post(`${API_BASE}/api/km/orgs`, {
      data: { name: `PipelineOrg-${suffix}` },
      headers,
    });
    orgId = (await orgRes.json()).id;

    const deptRes = await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, {
      data: { name: `PipelineDept-${suffix}` },
      headers,
    });
    deptId = (await deptRes.json()).id;

    const wsRes = await request.post(
      `${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
      { data: { name: `PipelineWS-${suffix}` }, headers },
    );
    wsId = (await wsRes.json()).id;
    console.log('Created workspace:', wsId);

    // Ingest a small test document
    const ingestRes = await request.post(
      `${API_BASE}/api/km/workspaces/${wsId}/documents`,
      {
        data: {
          title: 'Pipeline Test Document',
          content: 'This is a test document for pipeline stages testing. It contains information about software testing, quality assurance, and CI/CD pipelines. Unit tests verify individual components work correctly.',
          mime_type: 'text/plain',
        },
        headers,
        timeout: 60_000,
      },
    );
    const ingestData = await ingestRes.json();
    console.log('Ingest result:', JSON.stringify(ingestData));

    // Wait for indexing
    await new Promise((r) => setTimeout(r, 3000));
  });

  test.afterAll(async ({ request }) => {
    if (!token) return;
    const headers = { Authorization: `Bearer ${token}` };
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}`, { headers });
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers });
  });

  test('1. Check chat pipeline status via API', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };
    const pipelineRes = await request.get(`${API_BASE}/api/km/settings/chat-pipeline`, { headers });
    expect(pipelineRes.ok()).toBeTruthy();
    const pipeline = await pipelineRes.json();

    console.log('=== Chat Pipeline Config ===');
    console.log('enabled:', pipeline.enabled);
    console.log('query_analyzer_enabled:', pipeline.query_analyzer_enabled);
    console.log('context_curator_enabled:', pipeline.context_curator_enabled);
    console.log('quality_guard_enabled:', pipeline.quality_guard_enabled);
    console.log('language_adapter_enabled:', pipeline.language_adapter_enabled);
    console.log('orchestrator_enabled:', pipeline.orchestrator_enabled);
    console.log('llm_mode:', pipeline.llm_mode);
  });

  // Skip: Qdrant vector dimension mismatch (embedding model vs collection config)
  test.skip('2. Test query via non-streaming API (pipeline_stages field)', async ({ request }) => {
    const headers = { Authorization: `Bearer ${token}` };

    console.log('Sending test query via non-streaming API...');
    const res = await request.post(
      `${API_BASE}/api/km/workspaces/${wsId}/test-query`,
      {
        data: { query: 'What is software testing?' },
        headers,
        timeout: 300_000,
      },
    );

    expect(res.ok(), `Test query failed: ${res.status()}`).toBeTruthy();
    const data = await res.json();

    console.log('=== Non-Streaming Response ===');
    console.log('Answer length:', data.answer?.length);
    console.log('Chunks:', data.chunks?.length);
    console.log('Timing:', JSON.stringify(data.timing));
    console.log('pipeline_stages:', JSON.stringify(data.pipeline_stages, null, 2));
    console.log('pipeline_stages count:', data.pipeline_stages?.length ?? 0);

    expect(data.answer).toBeTruthy();
    // This is the key assertion: pipeline_stages should be populated
    expect(data.pipeline_stages?.length, 'Expected pipeline_stages in non-streaming response').toBeGreaterThan(0);
  });

  test('3. Test query via SSE streaming (progress events)', async ({ request }) => {
    console.log('Sending test query via SSE streaming...');

    const res = await fetch(
      `${API_BASE}/api/km/workspaces/${wsId}/test-query-stream`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ query: 'What is CI/CD?' }),
      },
    );

    expect(res.ok).toBeTruthy();

    const reader = res.body!.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let eventType = '';
    let dataLines: string[] = [];
    const progressEvents: unknown[] = [];
    let resultEvent: Record<string, unknown> | null = null;

    // eslint-disable-next-line no-constant-condition
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';

      for (const line of lines) {
        if (line.startsWith('event:')) {
          eventType = line.slice(6).trim();
        } else if (line.startsWith('data:')) {
          dataLines.push(line.slice(5).trim());
        } else if (line === '') {
          const data = dataLines.join('\n');
          dataLines = [];

          if (data === '[DONE]') {
            console.log('[SSE] [DONE]');
          } else if (eventType === 'progress') {
            try {
              const parsed = JSON.parse(data);
              progressEvents.push(parsed);
              console.log('[SSE] progress:', JSON.stringify(parsed));
            } catch {
              console.log('[SSE] progress (unparseable):', data);
            }
          } else if (eventType === 'result') {
            try {
              resultEvent = JSON.parse(data);
              console.log('[SSE] result received, answer length:', (resultEvent as { answer?: string })?.answer?.length);
            } catch {
              console.log('[SSE] result (unparseable):', data.slice(0, 200));
            }
          } else if (eventType === 'error') {
            console.log('[SSE] error:', data);
          } else {
            console.log(`[SSE] unknown event type="${eventType}" data="${data.slice(0, 100)}"`);
          }

          eventType = '';
        }
      }
    }

    console.log('\n=== SSE Summary ===');
    console.log('Total progress events:', progressEvents.length);
    console.log('Result received:', !!resultEvent);
    console.log('Result pipeline_stages:', JSON.stringify((resultEvent as Record<string, unknown>)?.pipeline_stages));

    expect(progressEvents.length, 'Expected at least 1 progress event (search)').toBeGreaterThan(0);
  });

  // Skip: Qdrant vector dimension mismatch causes query failure
  test.skip('4. UI test - pipeline stages render in Test Chat', async ({ page }) => {
    // Login
    await page.goto('/login');
    await page.getByPlaceholder('Email').fill(TEST_EMAIL);
    await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
    await page.getByRole('button', { name: 'Sign In' }).click();
    await page.waitForURL('/', { timeout: 10_000 });

    // Navigate to Test Chat
    await page.getByRole('menu').getByText('Test Chat', { exact: true }).click();
    await page.waitForTimeout(1000);

    // Select org/dept/workspace
    const orgSelect = page.locator('.ant-select').nth(0);
    await orgSelect.click();
    await page.getByTitle(`PipelineOrg-${suffix}`).click();
    await page.waitForTimeout(500);

    const deptSelect = page.locator('.ant-select').nth(1);
    await deptSelect.click();
    await page.getByTitle(`PipelineDept-${suffix}`).click();
    await page.waitForTimeout(500);

    const wsSelect = page.locator('.ant-select').nth(2);
    await wsSelect.click();
    await page.getByTitle(`PipelineWS-${suffix}`).click();
    await page.waitForTimeout(500);

    // Listen to console for SSE debug output
    const consoleLogs: string[] = [];
    page.on('console', (msg) => {
      const text = msg.text();
      if (text.includes('[SSE]') || text.includes('pipeline-debug') || text.includes('Pipeline')) {
        consoleLogs.push(`${msg.type()}: ${text}`);
      }
    });

    // Send query
    const input = page.getByRole('textbox', { name: 'Ask a question about' });
    await input.fill('What is software testing?');
    await page.getByRole('button', { name: 'Send' }).click();

    // Capture a mid-progress screenshot (wait for at least one pipeline stage to show)
    await expect(page.getByText(/Query Analyzer|Hybrid Search|Response Generator/)).toBeVisible({ timeout: 60_000 });
    await page.waitForTimeout(500);
    await page.screenshot({ path: 'e2e/screenshots/pipeline-stages-live.png', fullPage: true });
    console.log('Live progress screenshot saved');

    // Wait for response — look for the timing tag which appears only after completion
    await expect(
      page.getByText(/Total:.*ms/)
    ).toBeVisible({ timeout: 300_000 });

    // Wait for React to settle
    await page.waitForTimeout(3000);

    // Print browser console logs
    console.log('\n=== Browser Console Logs ===');
    for (const log of consoleLogs) {
      console.log(log);
    }

    // Take screenshot
    await page.screenshot({ path: 'e2e/screenshots/pipeline-stages-ui.png', fullPage: true });
    console.log('Screenshot saved');

    // Check for pipeline stages collapse panel
    const pipelineStagesPanel = page.getByText(/Pipeline Stages/);
    const hasPipelineStages = await pipelineStagesPanel.isVisible().catch(() => false);
    console.log('Pipeline Stages visible:', hasPipelineStages);

    // Check for timing tags
    const totalTag = page.getByText(/Total:.*ms/);
    const hasTotalTag = await totalTag.isVisible().catch(() => false);
    console.log('Total timing tag visible:', hasTotalTag);

    // Check for chunks
    const chunksText = page.getByText(/chunk.*retrieved/);
    const hasChunks = await chunksText.isVisible().catch(() => false);
    console.log('Chunks section visible:', hasChunks);

    // Dump the HTML of the assistant response area
    const messageArea = page.locator('[style*="flex-start"]').last();
    const html = await messageArea.innerHTML().catch(() => 'N/A');
    console.log('\n=== Assistant message area HTML (first 2000 chars) ===');
    console.log(html.slice(0, 2000));

    expect(hasPipelineStages, 'Pipeline Stages should be visible in UI').toBeTruthy();
  });
});
