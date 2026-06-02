import { test, expect, type Page } from '@playwright/test';

// Deterministic UI test for the "Reset to Balanced" button on the chat-pipeline
// card. The button is pure client-side state, so instead of standing up the
// backend we seed an auth session in sessionStorage and mock the settings
// endpoints. We load a config with every optional/experimental feature ON,
// click Reset to Balanced, and assert the experimental switches flip OFF while
// the core agents stay ON and the LLM mode becomes Shared.

const json = (body: unknown) => ({
  status: 200,
  contentType: 'application/json',
  body: JSON.stringify(body),
});

const llm = {
  kind: 'Ollama',
  model: 'gemma4:e4b',
  has_api_key: false,
  supports_vision: false,
};

const providerConfig = {
  llm,
  embedding: { kind: 'FastEmbed', model: 'qwen3-embedding', dimension: 512, has_api_key: false },
  vector_store: { kind: 'Qdrant', has_api_key: false },
  text_search: { kind: 'Tantivy', index_path: '/data/idx' },
  reranker: { kind: 'None', has_api_key: false },
};

const guardrails = {
  max_query_chars: 8000,
  detect_thai_id: true,
  detect_thai_phone: true,
  detect_email: true,
  detect_credit_card: true,
  detect_secrets: true,
  detect_prompt_injection: true,
  blocklist_phrases: [],
  input_on_violation: 'block',
  output_on_violation: 'redact',
  redaction_token: '[REDACTED]',
  fail_open: true,
};

// Everything that can be on, is on.
const allOnPipeline = {
  enabled: true,
  llm_mode: 'chat',
  llm,
  query_analyzer_enabled: true,
  query_rewriter_enabled: true,
  context_curator_enabled: true,
  quality_guard_enabled: true,
  quality_guard_max_retries: 1,
  quality_guard_threshold: 0.6,
  language_adapter_enabled: true,
  orchestrator_enabled: true,
  max_orchestrator_calls: 3,
  max_context_tokens: 4096,
  agent_max_tokens: 2048,
  request_timeout_secs: 120,
  ollama_keep_alive: '5m',
  conversation_memory_enabled: true,
  memory_max_summaries: 10,
  memory_summary_max_tokens: 256,
  retrieval_refinement_enabled: true,
  refinement_min_relevance: 0.3,
  refinement_max_retries: 1,
  tool_use_enabled: true,
  tool_use_max_calls: 3,
  adaptive_threshold_enabled: true,
  feedback_decay_days: 30,
  adaptive_min_samples: 20,
  self_rag_enabled: true,
  self_rag_threshold: 0.7,
  graph_rag_enabled: true,
  graph_rag_max_entities: 10,
  graph_rag_max_depth: 2,
  crag_enabled: true,
  crag_relevance_threshold: 0.3,
  crag_web_search_url: '',
  crag_max_web_results: 5,
  speculative_rag_enabled: true,
  speculative_candidates: 3,
  map_reduce_enabled: true,
  map_reduce_max_chunks: 15,
  ragas_enabled: true,
  ragas_sample_rate: 0.1,
  compression_enabled: true,
  compression_target_ratio: 0.5,
  multimodal_enabled: true,
  multimodal_max_images: 5,
  raptor_enabled: true,
  raptor_max_depth: 2,
  raptor_group_size: 3,
  colbert_enabled: true,
  colbert_top_n: 10,
  active_learning_enabled: true,
  active_learning_min_interactions: 5,
  active_learning_max_low_confidence: 100,
  context_compaction_enabled: true,
  model_context_window: 0,
  compaction_threshold: 0.8,
  compaction_keep_recent: 6,
  personal_memory_enabled: true,
  personal_memory_top_k: 5,
  personal_memory_max_per_user: 200,
  personal_memory_decay_factor: 0.95,
  personal_memory_min_relevance: 0.1,
  live_retrieval_enabled: true,
  live_retrieval_timeout_secs: 15,
  live_retrieval_max_connectors: 3,
  live_retrieval_max_content_chars: 30000,
  input_guardrails_enabled: true,
  output_guardrails_enabled: true,
  guardrails,
};

async function seedAuthAndMocks(page: Page) {
  await page.addInitScript(() => {
    sessionStorage.setItem('thairag-token', 'test-token');
    sessionStorage.setItem(
      'thairag-user',
      JSON.stringify({
        id: 'u1',
        email: 'pw@test.com',
        name: 'PW',
        auth_provider: 'local',
        is_super_admin: true,
        role: 'super_admin',
        disabled: false,
        created_at: new Date().toISOString(),
      }),
    );
    localStorage.setItem('thairag-tour-state', '{}');
    localStorage.setItem('thairag-quickstart-dismissed', 'true');
  });

  // Fallback: anything we don't explicitly mock returns an empty object so no
  // request hits the (absent) backend. Anchored to the /api/ path prefix so we
  // don't intercept Vite's own module requests (e.g. /src/api/client.ts).
  await page.route(
    (url) => url.pathname.startsWith('/api/'),
    (route) => route.fulfill(json({})),
  );

  // Specific endpoints (registered after the catch-all so they take priority).
  await page.route(/\/api\/km\/settings\/providers$/, (route) =>
    route.fulfill(json(providerConfig)),
  );
  await page.route(/\/api\/km\/settings\/providers\/models$/, (route) =>
    route.fulfill(json({ models: [] })),
  );
  await page.route(/\/api\/km\/settings\/chat-pipeline(\?|$)/, (route) =>
    route.fulfill(json(allOnPipeline)),
  );
  await page.route(/\/api\/km\/settings\/vault\/profiles$/, (route) =>
    route.fulfill(json([])),
  );
  await page.route(/\/api\/km\/settings\/presets$/, (route) =>
    route.fulfill(json([])),
  );
  await page.route(/\/api\/km\/settings\/recommendations\/status$/, (route) =>
    route.fulfill(
      json({ has_data: false, model_count: 0, stale: false, enabled: false, configured: false }),
    ),
  );
  await page.route(/\/api\/km\/settings\/feedback\/stats$/, (route) =>
    route.fulfill(
      json({
        total: 0,
        positive: 0,
        negative: 0,
        positive_rate: 0,
        current_threshold: 0.6,
        adaptive_enabled: false,
        min_samples: 20,
      }),
    ),
  );
}

// Switch lives in a Collapse panel header alongside the feature name.
function featureSwitch(page: Page, label: string) {
  return page
    .locator('.ant-collapse-item', { hasText: label })
    .locator('.ant-switch')
    .first();
}

test.describe('Reset to Balanced button', () => {
  test('flips experimental features off but keeps core agents on', async ({ page }) => {
    await seedAuthAndMocks(page);
    await page.goto('/settings');

    // Open the "Chat & Response Pipeline" tab.
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();

    // The card renders with the all-on config.
    await expect(page.getByText('Response Pipeline', { exact: true })).toBeVisible({
      timeout: 10_000,
    });

    // Sanity: experimental features start ON.
    await expect(featureSwitch(page, 'Self-RAG')).toHaveClass(/ant-switch-checked/);
    await expect(featureSwitch(page, 'Map-Reduce RAG')).toHaveClass(/ant-switch-checked/);
    await expect(featureSwitch(page, 'Contextual Compression')).toHaveClass(/ant-switch-checked/);
    // Core agent (Language Adapter) is also on.
    await expect(featureSwitch(page, 'Language Adapter')).toHaveClass(/ant-switch-checked/);

    // Click Reset to Balanced → confirm in the popconfirm.
    await page.getByRole('button', { name: 'Reset to Balanced' }).click();
    await page.getByRole('button', { name: 'Reset', exact: true }).click();

    // Experimental features are now OFF.
    await expect(featureSwitch(page, 'Self-RAG')).not.toHaveClass(/ant-switch-checked/);
    await expect(featureSwitch(page, 'Map-Reduce RAG')).not.toHaveClass(/ant-switch-checked/);
    await expect(featureSwitch(page, 'Contextual Compression')).not.toHaveClass(
      /ant-switch-checked/,
    );
    await expect(featureSwitch(page, 'Graph RAG')).not.toHaveClass(/ant-switch-checked/);
    await expect(featureSwitch(page, 'Personal Memory')).not.toHaveClass(/ant-switch-checked/);

    // Core agent stays ON; LLM mode switched to Shared.
    await expect(featureSwitch(page, 'Language Adapter')).toHaveClass(/ant-switch-checked/);
    await expect(page.locator('.ant-segmented-item-selected')).toHaveText('Shared');
  });
});
