import { useEffect, useState, useCallback } from 'react';
import {
  Card,
  Switch,
  InputNumber,
  Slider,
  Button,
  Typography,
  Space,
  Spin,
  message,
  Divider,
  Select,
  Input,
  Collapse,
  Tag,
  Tooltip,
  Segmented,
  Alert,
} from 'antd';
import {
  RobotOutlined,
  SaveOutlined,
  SyncOutlined,
  QuestionCircleOutlined,
  ThunderboltOutlined,
  InfoCircleOutlined,
} from '@ant-design/icons';
import { getChatPipelineConfig, updateChatPipelineConfig, syncModels, getFeedbackStats } from '../../api/settings';
import type {
  ChatPipelineConfigResponse,
  AvailableModel,
  FeedbackStats,
  LlmConfigUpdate,
  LlmProviderInfo,
  UpdateChatPipelineRequest,
} from '../../api/types';

const { Text, Paragraph } = Typography;

// ── Agent descriptions with hints ────────────────────────────────────

interface RecommendedModel {
  provider: string;
  model: string;
  note?: string;
}

interface AgentInfo {
  label: string;
  description: string;
  whenRuns: string;
  disableImpact: string;
  llmTip: string;
  taskWeight: 'Light' | 'Medium' | 'Heavy';
  alwaysOn?: boolean;
  recommended: RecommendedModel[];
}

const chatAgents: Record<string, AgentInfo> = {
  query_analyzer: {
    label: 'Query Analyzer',
    description: 'Detects language (Thai/English/Mixed), intent (greeting, retrieval, comparison, analysis), complexity level, and topic keywords from user queries.',
    whenRuns: 'Runs on every query as the first pipeline step.',
    disableImpact: 'Falls back to heuristic rules — fast but less accurate for nuanced Thai/English mixed queries.',
    llmTip: 'Small/fast model works well — outputs only a short JSON classification.',
    taskWeight: 'Light',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:4b', note: 'Free, fast' },
      { provider: 'OpenAI', model: 'gpt-4.1-nano', note: 'Cheapest' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.0-flash', note: 'Fast' },
    ],
  },
  pipeline_orchestrator: {
    label: 'Pipeline Orchestrator',
    description: 'Dynamically decides which downstream agents to run based on query complexity. Simple queries skip rewriting; greetings skip retrieval entirely. Saves latency and cost.',
    whenRuns: 'Runs after Query Analyzer on every query.',
    disableImpact: 'Uses zero-latency heuristic routing (still smart — just not LLM-driven). Simple greetings still short-circuit.',
    llmTip: 'Only uses LLM when enabled. Without LLM, the heuristic decision tree handles routing at zero cost.',
    taskWeight: 'Light',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:4b', note: 'Free, fast' },
      { provider: 'OpenAI', model: 'gpt-4.1-nano', note: 'Cheapest' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.0-flash', note: 'Fast' },
    ],
  },
  query_rewriter: {
    label: 'Query Rewriter',
    description: 'Decomposes complex queries into sub-queries, expands Thai↔English terms, and generates HyDE (hypothetical answer) queries for better retrieval.',
    whenRuns: 'Runs for moderate/complex queries. Skipped by orchestrator for simple lookups.',
    disableImpact: 'Search uses the raw user query only — no sub-queries or term expansion. May miss relevant documents.',
    llmTip: 'Medium model recommended — needs to understand query semantics and generate multiple search variants.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:12b', note: 'Free, good balance' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Strong reasoning' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
  context_curator: {
    label: 'Context Curator',
    description: 'Selects and ranks the most relevant retrieved chunks within a token budget. Removes noise so the generator gets clean, focused context.',
    whenRuns: 'Runs after search results are retrieved and reranked.',
    disableImpact: 'Takes the top-K search results directly without LLM curation. Works well with a good reranker.',
    llmTip: 'Small/fast model works — it only needs to judge chunk relevance, not generate text.',
    taskWeight: 'Light',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:4b', note: 'Free, fast' },
      { provider: 'OpenAI', model: 'gpt-4.1-nano', note: 'Cheapest' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.0-flash', note: 'Fast' },
    ],
  },
  response_generator: {
    label: 'Response Generator',
    description: 'Generates the final answer using curated context. Produces citation markers [1][2] linking to source documents. This is the core agent that creates the response.',
    whenRuns: 'Always runs — this is the core generation step.',
    disableImpact: 'Cannot be disabled — always on.',
    llmTip: 'Use your best/largest model here — response quality directly depends on it.',
    taskWeight: 'Heavy',
    alwaysOn: true,
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, best local quality' },
      { provider: 'OpenAI', model: 'gpt-4.1', note: 'Top tier' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Best balance' },
      { provider: 'Gemini', model: 'gemini-2.5-pro', note: 'Most capable' },
    ],
  },
  quality_guard: {
    label: 'Quality Guard',
    description: 'Evaluates the generated response for relevance, hallucination, and completeness (scores 0.0-1.0). If below threshold, sends feedback to the generator for retry.',
    whenRuns: 'Runs after generation in non-streaming mode. Forced on for complex queries by the orchestrator.',
    disableImpact: 'Responses are returned without quality verification. Faster but may include hallucinations.',
    llmTip: 'Small/fast model works — it evaluates quality, not generates content. Keep threshold 0.5-0.7 for balance.',
    taskWeight: 'Light',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:4b', note: 'Free, fast' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Good judgment' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.0-flash', note: 'Fast' },
    ],
  },
  language_adapter: {
    label: 'Language Adapter',
    description: 'Ensures the response language matches the user\'s query language. If a user asks in Thai but the response is in English, it translates while preserving [1][2] citations.',
    whenRuns: 'Runs as the final step in non-streaming mode. Skipped in streaming mode.',
    disableImpact: 'Response may come in a different language than the query (e.g., English response to Thai query).',
    llmTip: 'Needs a bilingual model that handles Thai well. Only invoked when language mismatch is detected.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:8b', note: 'Free, good multilingual' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Good multilingual' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Excellent Thai' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Strong multilingual' },
    ],
  },
};

// ── LLM form state ──────────────────────────────────────────────────

interface LlmFormState {
  kind: string;
  model: string;
  base_url: string;
  api_key: string;
}

const defaultLlmForm: LlmFormState = { kind: 'Ollama', model: '', base_url: '', api_key: '' };

function llmInfoToForm(info: LlmProviderInfo): LlmFormState {
  return {
    kind: info.kind,
    model: info.model,
    base_url: info.base_url || '',
    api_key: '',
  };
}

function formToUpdate(form: LlmFormState, hasExistingKey: boolean): LlmConfigUpdate {
  const update: LlmConfigUpdate = { kind: form.kind, model: form.model };
  // Always send base_url for providers that need it (Ollama defaults to localhost)
  const needsBaseUrl = form.kind === 'Ollama' || form.kind === 'OpenAiCompatible';
  if (needsBaseUrl) {
    update.base_url = form.base_url || 'http://localhost:11434';
  } else if (form.base_url) {
    update.base_url = form.base_url;
  }
  if (form.api_key || !hasExistingKey) update.api_key = form.api_key;
  return update;
}

// ── Agent hint panel ─────────────────────────────────────────────────

function AgentHints({ info }: { info: AgentInfo }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginTop: 4 }}>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '4px 16px' }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <ThunderboltOutlined /> <strong>Runs:</strong> {info.whenRuns}
        </Text>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <InfoCircleOutlined /> <strong>If disabled:</strong> {info.disableImpact}
        </Text>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <RobotOutlined /> <strong>LLM tip:</strong> {info.llmTip}
        </Text>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <Tag color={info.taskWeight === 'Heavy' ? 'red' : info.taskWeight === 'Medium' ? 'orange' : 'green'} style={{ fontSize: 11 }}>
            {info.taskWeight} workload
          </Tag>
        </Text>
      </div>
      {info.recommended.length > 0 && (
        <div style={{ marginTop: 4 }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            <RobotOutlined /> <strong>Recommended models:</strong>
          </Text>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 2 }}>
            {info.recommended.map((r) => (
              <Tooltip key={`${r.provider}-${r.model}`} title={`${r.provider} — ${r.note || r.model}`}>
                <Tag
                  color={
                    r.provider === 'Claude' ? 'purple' :
                    r.provider === 'OpenAI' ? 'green' :
                    r.provider === 'Gemini' ? 'gold' :
                    'blue'
                  }
                  style={{ fontSize: 11, cursor: 'default' }}
                >
                  {r.model} {r.note ? `(${r.note})` : ''}
                </Tag>
              </Tooltip>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// ── Feature LLM recommendations ──────────────────────────────────────

interface FeatureLlmInfo {
  llmTip: string;
  taskWeight: 'Light' | 'Medium' | 'Heavy';
  recommended: RecommendedModel[];
}

const featureLlmRecommendations: Record<string, FeatureLlmInfo> = {
  memory: {
    llmTip: 'Summarizes conversations — needs good comprehension. Medium model recommended.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good quality' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
  tool_use: {
    llmTip: 'Decides search strategy and workspace selection. Needs reasoning ability.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good reasoning' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Strong reasoning' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
  self_rag: {
    llmTip: 'Decides if retrieval is needed — short classification output. Small model works.',
    taskWeight: 'Light',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:4b', note: 'Free, fast' },
      { provider: 'OpenAI', model: 'gpt-4.1-nano', note: 'Cheapest' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.0-flash', note: 'Fast' },
    ],
  },
  graph_rag: {
    llmTip: 'Extracts entities and relationships from text. Needs strong comprehension.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good NER' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Strong extraction' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
  map_reduce: {
    llmTip: 'Processes many chunks independently then synthesizes. Heavy workload on large result sets.',
    taskWeight: 'Heavy',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good synthesis' },
      { provider: 'OpenAI', model: 'gpt-4.1', note: 'Top tier' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Best balance' },
      { provider: 'Gemini', model: 'gemini-2.5-pro', note: 'Most capable' },
    ],
  },
  ragas: {
    llmTip: 'Evaluates response quality (faithfulness, relevancy). Runs on a sample — not latency-critical.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good judgment' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
  compression: {
    llmTip: 'Compresses context by removing low-importance content. Needs good text understanding.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good compression' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
  multimodal: {
    llmTip: 'Generates text descriptions of images. Requires a vision-capable model.',
    taskWeight: 'Heavy',
    recommended: [
      { provider: 'Ollama', model: 'llama4:scout', note: 'Free, vision-capable' },
      { provider: 'OpenAI', model: 'gpt-4.1', note: 'Strong vision' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Excellent vision' },
      { provider: 'Gemini', model: 'gemini-2.5-pro', note: 'Best vision' },
    ],
  },
  raptor: {
    llmTip: 'Builds hierarchical summaries over chunks. Multiple LLM calls per request — quality matters.',
    taskWeight: 'Heavy',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good summaries' },
      { provider: 'OpenAI', model: 'gpt-4.1', note: 'Top tier' },
      { provider: 'Claude', model: 'claude-sonnet-4-20250514', note: 'Best balance' },
      { provider: 'Gemini', model: 'gemini-2.5-pro', note: 'Most capable' },
    ],
  },
  colbert: {
    llmTip: 'LLM-based reranking across multiple aspects. Runs per search result — keep model fast.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'gemma3:4b', note: 'Free, fast' },
      { provider: 'OpenAI', model: 'gpt-4.1-nano', note: 'Cheapest' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.0-flash', note: 'Fast' },
    ],
  },
  personal_memory: {
    llmTip: 'Extracts memories from conversations. Needs comprehension for typed memory classification.',
    taskWeight: 'Medium',
    recommended: [
      { provider: 'Ollama', model: 'qwen3:14b', note: 'Free, good extraction' },
      { provider: 'OpenAI', model: 'gpt-4.1-mini', note: 'Best value' },
      { provider: 'Claude', model: 'claude-haiku-4-20250414', note: 'Fast & cheap' },
      { provider: 'Gemini', model: 'gemini-2.5-flash', note: 'Fast & capable' },
    ],
  },
};

function FeatureLlmHints({ featureKey }: { featureKey: string }) {
  const info = featureLlmRecommendations[featureKey];
  if (!info) return null;
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginTop: 4 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <RobotOutlined /> <strong>LLM tip:</strong> {info.llmTip}
        </Text>
        <Tag color={info.taskWeight === 'Heavy' ? 'red' : info.taskWeight === 'Medium' ? 'orange' : 'green'} style={{ fontSize: 11 }}>
          {info.taskWeight} workload
        </Tag>
      </div>
      {info.recommended.length > 0 && (
        <div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            <RobotOutlined /> <strong>Recommended models:</strong>
          </Text>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 2 }}>
            {info.recommended.map((r) => (
              <Tooltip key={`${r.provider}-${r.model}`} title={`${r.provider} — ${r.note || r.model}`}>
                <Tag
                  color={
                    r.provider === 'Claude' ? 'purple' :
                    r.provider === 'OpenAI' ? 'green' :
                    r.provider === 'Gemini' ? 'gold' :
                    'blue'
                  }
                  style={{ fontSize: 11, cursor: 'default' }}
                >
                  {r.model} {r.note ? `(${r.note})` : ''}
                </Tag>
              </Tooltip>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// ── LlmConfigForm sub-component ─────────────────────────────────────

function LlmConfigForm({
  form, onChange, syncedModels, onSync, syncing,
}: {
  form: LlmFormState;
  onChange: (f: LlmFormState) => void;
  syncedModels: AvailableModel[] | null;
  onSync: (kind: string, baseUrl: string, apiKey: string) => void;
  syncing: boolean;
}) {
  const needsBaseUrl = form.kind === 'Ollama' || form.kind === 'OpenAiCompatible';
  const needsApiKey = form.kind !== 'Ollama';

  return (
    <Space direction="vertical" size="small" style={{ width: '100%' }}>
      <Space wrap>
        <Select
          value={form.kind}
          onChange={(v) => onChange({ ...form, kind: v, model: '' })}
          style={{ width: 160 }}
          options={[
            { label: 'Ollama', value: 'Ollama' },
            { label: 'Claude', value: 'Claude' },
            { label: 'OpenAI', value: 'OpenAi' },
            { label: 'Gemini', value: 'Gemini' },
            { label: 'OpenAI-Compatible', value: 'OpenAiCompatible' },
          ]}
        />
        {syncedModels ? (
          <Select
            value={form.model || undefined}
            onChange={(v) => onChange({ ...form, model: v })}
            placeholder="Select model"
            style={{ width: 240 }}
            showSearch
            options={syncedModels.map((m) => ({ label: m.name || m.id, value: m.id }))}
          />
        ) : (
          <Input
            value={form.model}
            onChange={(e) => onChange({ ...form, model: e.target.value })}
            placeholder="Model name"
            style={{ width: 240 }}
          />
        )}
        <Button
          icon={<SyncOutlined spin={syncing} />}
          size="small"
          onClick={() => onSync(form.kind, form.base_url, form.api_key)}
          loading={syncing}
        >
          Sync
        </Button>
      </Space>
      {needsBaseUrl && (
        <Input
          value={form.base_url}
          onChange={(e) => onChange({ ...form, base_url: e.target.value })}
          placeholder="Base URL (e.g., http://localhost:11434)"
          style={{ width: 400 }}
        />
      )}
      {needsApiKey && (
        <Input.Password
          value={form.api_key}
          onChange={(e) => onChange({ ...form, api_key: e.target.value })}
          placeholder="API Key (leave empty to keep existing)"
          style={{ width: 400 }}
        />
      )}
    </Space>
  );
}

// ── Main Component ──────────────────────────────────────────────────

export function ChatPipelineCard() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [config, setConfig] = useState<ChatPipelineConfigResponse | null>(null);

  // Helper: render model tag for a feature from featureLlms state (live updates)
  const featureModelTag = (featureKey: string, isEnabled: boolean) => {
    if (!isEnabled) return null;
    const form = featureLlms[featureKey];
    const model = form?.model;
    return model ? (
      <Tag color="purple" style={{ fontSize: 11 }}>{form?.kind}: {model}</Tag>
    ) : (
      <Tag color="warning" style={{ fontSize: 11 }}>No model (uses fallback)</Tag>
    );
  };

  // Helper: render LLM config form for a feature with recommendations
  const featureLlmForm = (featureKey: string, isEnabled: boolean) => {
    if (!isEnabled) return null;
    return (
      <>
        <Divider style={{ margin: '8px 0 4px' }} />
        <FeatureLlmHints featureKey={featureKey} />
        <Text type="secondary" style={{ marginTop: 4 }}>Feature LLM Override:</Text>
        <LlmConfigForm
          form={featureLlms[featureKey] || defaultLlmForm}
          onChange={(f) => setFeatureLlms((prev) => ({ ...prev, [featureKey]: f }))}
          syncedModels={syncedModels}
          onSync={handleSync}
          syncing={syncing}
        />
      </>
    );
  };

  // Pipeline-level state
  const [enabled, setEnabled] = useState(false);
  const [llmMode, setLlmMode] = useState<'chat' | 'shared' | 'per-agent'>('chat');
  const [sharedLlm, setSharedLlm] = useState<LlmFormState>({ ...defaultLlmForm });
  const [agentToggles, setAgentToggles] = useState<Record<string, boolean>>({
    query_analyzer: true,
    pipeline_orchestrator: false,
    query_rewriter: true,
    context_curator: true,
    quality_guard: false,
    language_adapter: true,
  });
  const [agentLlms, setAgentLlms] = useState<Record<string, LlmFormState>>({});
  const [featureLlms, setFeatureLlms] = useState<Record<string, LlmFormState>>({});
  const [qualityMaxRetries, setQualityMaxRetries] = useState(1);
  const [qualityThreshold, setQualityThreshold] = useState(0.6);
  const [maxContextTokens, setMaxContextTokens] = useState(4096);
  const [agentMaxTokens, setAgentMaxTokens] = useState(2048);
  const [requestTimeoutSecs, setRequestTimeoutSecs] = useState(120);
  const [ollamaKeepAlive, setOllamaKeepAlive] = useState('5m');
  const [maxOrchestratorCalls, setMaxOrchestratorCalls] = useState(3);

  // Feature: Conversation Memory
  const [conversationMemoryEnabled, setConversationMemoryEnabled] = useState(false);
  const [memoryMaxSummaries, setMemoryMaxSummaries] = useState(10);
  const [memorySummaryMaxTokens, setMemorySummaryMaxTokens] = useState(256);
  // Feature: Multi-turn Retrieval Refinement
  const [retrievalRefinementEnabled, setRetrievalRefinementEnabled] = useState(false);
  const [refinementMinRelevance, setRefinementMinRelevance] = useState(0.3);
  const [refinementMaxRetries, setRefinementMaxRetries] = useState(1);
  // Feature: Agentic Tool Use
  const [toolUseEnabled, setToolUseEnabled] = useState(false);
  const [toolUseMaxCalls, setToolUseMaxCalls] = useState(3);
  // Feature: Adaptive Quality Thresholds
  const [adaptiveThresholdEnabled, setAdaptiveThresholdEnabled] = useState(false);
  const [feedbackDecayDays, setFeedbackDecayDays] = useState(30);
  const [adaptiveMinSamples, setAdaptiveMinSamples] = useState(20);
  const [feedbackStats, setFeedbackStats] = useState<FeedbackStats | null>(null);
  // Self-RAG
  const [selfRagEnabled, setSelfRagEnabled] = useState(false);
  const [selfRagThreshold, setSelfRagThreshold] = useState(0.7);
  // Graph RAG
  const [graphRagEnabled, setGraphRagEnabled] = useState(false);
  const [graphRagMaxEntities, setGraphRagMaxEntities] = useState(10);
  const [graphRagMaxDepth, setGraphRagMaxDepth] = useState(2);
  // CRAG
  const [cragEnabled, setCragEnabled] = useState(false);
  const [cragRelevanceThreshold, setCragRelevanceThreshold] = useState(0.3);
  const [cragWebSearchUrl, setCragWebSearchUrl] = useState('');
  const [cragMaxWebResults, setCragMaxWebResults] = useState(5);
  // Speculative RAG
  const [speculativeRagEnabled, setSpeculativeRagEnabled] = useState(false);
  const [speculativeCandidates, setSpeculativeCandidates] = useState(3);
  // Map-Reduce RAG
  const [mapReduceEnabled, setMapReduceEnabled] = useState(false);
  const [mapReduceMaxChunks, setMapReduceMaxChunks] = useState(15);
  // RAGAS
  const [ragasEnabled, setRagasEnabled] = useState(false);
  const [ragasSampleRate, setRagasSampleRate] = useState(0.1);
  // Contextual Compression
  const [compressionEnabled, setCompressionEnabled] = useState(false);
  const [compressionTargetRatio, setCompressionTargetRatio] = useState(0.5);
  // Multi-modal RAG
  const [multimodalEnabled, setMultimodalEnabled] = useState(false);
  const [multimodalMaxImages, setMultimodalMaxImages] = useState(5);
  // RAPTOR
  const [raptorEnabled, setRaptorEnabled] = useState(false);
  const [raptorMaxDepth, setRaptorMaxDepth] = useState(2);
  const [raptorGroupSize, setRaptorGroupSize] = useState(3);
  // ColBERT
  const [colbertEnabled, setColbertEnabled] = useState(false);
  const [colbertTopN, setColbertTopN] = useState(10);
  // Active Learning
  const [activeLearningEnabled, setActiveLearningEnabled] = useState(false);
  const [activeLearningMinInteractions, setActiveLearningMinInteractions] = useState(5);
  const [activeLearningMaxLowConfidence, setActiveLearningMaxLowConfidence] = useState(100);

  // Context Compaction
  const [contextCompactionEnabled, setContextCompactionEnabled] = useState(false);
  const [modelContextWindow, setModelContextWindow] = useState(0);
  const [compactionThreshold, setCompactionThreshold] = useState(0.8);
  const [compactionKeepRecent, setCompactionKeepRecent] = useState(6);

  // Personal Memory (Per-User RAG)
  const [personalMemoryEnabled, setPersonalMemoryEnabled] = useState(false);
  const [personalMemoryTopK, setPersonalMemoryTopK] = useState(5);
  const [personalMemoryMaxPerUser, setPersonalMemoryMaxPerUser] = useState(200);
  const [personalMemoryDecayFactor, setPersonalMemoryDecayFactor] = useState(0.95);
  const [personalMemoryMinRelevance, setPersonalMemoryMinRelevance] = useState(0.1);

  // Sync state
  const [syncedModels, setSyncedModels] = useState<AvailableModel[] | null>(null);
  const [syncing, setSyncing] = useState(false);

  useEffect(() => { loadConfig(); }, []);

  async function loadConfig() {
    try {
      const data = await getChatPipelineConfig();
      setConfig(data);
      setEnabled(data.enabled);
      setQualityMaxRetries(data.quality_guard_max_retries);
      setQualityThreshold(data.quality_guard_threshold);
      setMaxContextTokens(data.max_context_tokens);
      setAgentMaxTokens(data.agent_max_tokens);
      setRequestTimeoutSecs(data.request_timeout_secs);
      setOllamaKeepAlive(data.ollama_keep_alive);
      setMaxOrchestratorCalls(data.max_orchestrator_calls);

      setAgentToggles({
        query_analyzer: data.query_analyzer_enabled,
        pipeline_orchestrator: data.orchestrator_enabled,
        query_rewriter: data.query_rewriter_enabled,
        context_curator: data.context_curator_enabled,
        quality_guard: data.quality_guard_enabled,
        language_adapter: data.language_adapter_enabled,
      });

      // Use persisted LLM mode from backend
      const mode = data.llm_mode as 'chat' | 'shared' | 'per-agent';
      setLlmMode(mode === 'shared' || mode === 'per-agent' ? mode : 'chat');

      if (data.llm) {
        setSharedLlm(llmInfoToForm(data.llm));
      }

      // Load per-agent LLMs (map pipeline_orchestrator -> orchestrator for API key)
      const llms: Record<string, LlmFormState> = {};
      const llmKeys = ['query_analyzer', 'query_rewriter', 'context_curator', 'response_generator', 'quality_guard', 'language_adapter', 'orchestrator'] as const;
      for (const k of llmKeys) {
        const llmInfo = data[`${k}_llm` as keyof ChatPipelineConfigResponse] as LlmProviderInfo | undefined;
        const stateKey = k === 'orchestrator' ? 'pipeline_orchestrator' : k;
        llms[stateKey] = llmInfo ? llmInfoToForm(llmInfo) : { ...defaultLlmForm };
      }
      setAgentLlms(llms);

      // Load feature LLMs
      const fLlms: Record<string, LlmFormState> = {};
      const featureLlmKeys = [
        'memory', 'tool_use', 'self_rag', 'graph_rag', 'map_reduce',
        'ragas', 'compression', 'multimodal', 'raptor', 'colbert', 'personal_memory',
      ] as const;
      for (const k of featureLlmKeys) {
        const llmInfo = data[`${k}_llm` as keyof ChatPipelineConfigResponse] as LlmProviderInfo | undefined;
        fLlms[k] = llmInfo ? llmInfoToForm(llmInfo) : { ...defaultLlmForm };
      }
      setFeatureLlms(fLlms);

      // Feature states
      setConversationMemoryEnabled(data.conversation_memory_enabled);
      setMemoryMaxSummaries(data.memory_max_summaries);
      setMemorySummaryMaxTokens(data.memory_summary_max_tokens);
      setRetrievalRefinementEnabled(data.retrieval_refinement_enabled);
      setRefinementMinRelevance(data.refinement_min_relevance);
      setRefinementMaxRetries(data.refinement_max_retries);
      setToolUseEnabled(data.tool_use_enabled);
      setToolUseMaxCalls(data.tool_use_max_calls);
      setAdaptiveThresholdEnabled(data.adaptive_threshold_enabled);
      setFeedbackDecayDays(data.feedback_decay_days);
      setAdaptiveMinSamples(data.adaptive_min_samples);

      // Next-gen features
      setSelfRagEnabled(data.self_rag_enabled);
      setSelfRagThreshold(data.self_rag_threshold);
      setGraphRagEnabled(data.graph_rag_enabled);
      setGraphRagMaxEntities(data.graph_rag_max_entities);
      setGraphRagMaxDepth(data.graph_rag_max_depth);
      setCragEnabled(data.crag_enabled);
      setCragRelevanceThreshold(data.crag_relevance_threshold);
      setCragWebSearchUrl(data.crag_web_search_url);
      setCragMaxWebResults(data.crag_max_web_results);
      setSpeculativeRagEnabled(data.speculative_rag_enabled);
      setSpeculativeCandidates(data.speculative_candidates);
      setMapReduceEnabled(data.map_reduce_enabled);
      setMapReduceMaxChunks(data.map_reduce_max_chunks);
      setRagasEnabled(data.ragas_enabled);
      setRagasSampleRate(data.ragas_sample_rate);
      // Final 5 features
      setCompressionEnabled(data.compression_enabled);
      setCompressionTargetRatio(data.compression_target_ratio);
      setMultimodalEnabled(data.multimodal_enabled);
      setMultimodalMaxImages(data.multimodal_max_images);
      setRaptorEnabled(data.raptor_enabled);
      setRaptorMaxDepth(data.raptor_max_depth);
      setRaptorGroupSize(data.raptor_group_size);
      setColbertEnabled(data.colbert_enabled);
      setColbertTopN(data.colbert_top_n);
      setActiveLearningEnabled(data.active_learning_enabled);
      setActiveLearningMinInteractions(data.active_learning_min_interactions);
      setActiveLearningMaxLowConfidence(data.active_learning_max_low_confidence);
      setContextCompactionEnabled(data.context_compaction_enabled);
      setModelContextWindow(data.model_context_window);
      setCompactionThreshold(data.compaction_threshold);
      setCompactionKeepRecent(data.compaction_keep_recent);
      setPersonalMemoryEnabled(data.personal_memory_enabled);
      setPersonalMemoryTopK(data.personal_memory_top_k);
      setPersonalMemoryMaxPerUser(data.personal_memory_max_per_user);
      setPersonalMemoryDecayFactor(data.personal_memory_decay_factor);
      setPersonalMemoryMinRelevance(data.personal_memory_min_relevance);

      // Load feedback stats if adaptive threshold is relevant
      try {
        const stats = await getFeedbackStats();
        setFeedbackStats(stats);
      } catch { /* ignore if endpoint not available */ }
    } catch {
      message.error('Failed to load chat pipeline config');
    } finally {
      setLoading(false);
    }
  }

  const handleSync = useCallback(async (kind: string, baseUrl: string, apiKey: string) => {
    setSyncing(true);
    try {
      const data = await syncModels({ kind, base_url: baseUrl || undefined, api_key: apiKey || undefined });
      setSyncedModels(data.models);
      message.success(`Found ${data.models.length} models`);
    } catch {
      message.error('Failed to sync models');
    } finally {
      setSyncing(false);
    }
  }, []);

  async function handleSave() {
    setSaving(true);
    try {
      const req: UpdateChatPipelineRequest = {
        enabled,
        llm_mode: llmMode,
        query_analyzer_enabled: agentToggles.query_analyzer,
        orchestrator_enabled: agentToggles.pipeline_orchestrator,
        max_orchestrator_calls: maxOrchestratorCalls,
        query_rewriter_enabled: agentToggles.query_rewriter,
        context_curator_enabled: agentToggles.context_curator,
        quality_guard_enabled: agentToggles.quality_guard,
        language_adapter_enabled: agentToggles.language_adapter,
        quality_guard_max_retries: qualityMaxRetries,
        quality_guard_threshold: qualityThreshold,
        max_context_tokens: maxContextTokens,
        agent_max_tokens: agentMaxTokens,
        request_timeout_secs: requestTimeoutSecs,
        ollama_keep_alive: ollamaKeepAlive,
        // Feature flags
        conversation_memory_enabled: conversationMemoryEnabled,
        memory_max_summaries: memoryMaxSummaries,
        memory_summary_max_tokens: memorySummaryMaxTokens,
        retrieval_refinement_enabled: retrievalRefinementEnabled,
        refinement_min_relevance: refinementMinRelevance,
        refinement_max_retries: refinementMaxRetries,
        tool_use_enabled: toolUseEnabled,
        tool_use_max_calls: toolUseMaxCalls,
        adaptive_threshold_enabled: adaptiveThresholdEnabled,
        feedback_decay_days: feedbackDecayDays,
        adaptive_min_samples: adaptiveMinSamples,
        // Next-gen features
        self_rag_enabled: selfRagEnabled,
        self_rag_threshold: selfRagThreshold,
        graph_rag_enabled: graphRagEnabled,
        graph_rag_max_entities: graphRagMaxEntities,
        graph_rag_max_depth: graphRagMaxDepth,
        crag_enabled: cragEnabled,
        crag_relevance_threshold: cragRelevanceThreshold,
        crag_web_search_url: cragWebSearchUrl,
        crag_max_web_results: cragMaxWebResults,
        speculative_rag_enabled: speculativeRagEnabled,
        speculative_candidates: speculativeCandidates,
        map_reduce_enabled: mapReduceEnabled,
        map_reduce_max_chunks: mapReduceMaxChunks,
        ragas_enabled: ragasEnabled,
        ragas_sample_rate: ragasSampleRate,
        // Final 5 features
        compression_enabled: compressionEnabled,
        compression_target_ratio: compressionTargetRatio,
        multimodal_enabled: multimodalEnabled,
        multimodal_max_images: multimodalMaxImages,
        raptor_enabled: raptorEnabled,
        raptor_max_depth: raptorMaxDepth,
        raptor_group_size: raptorGroupSize,
        colbert_enabled: colbertEnabled,
        colbert_top_n: colbertTopN,
        active_learning_enabled: activeLearningEnabled,
        active_learning_min_interactions: activeLearningMinInteractions,
        active_learning_max_low_confidence: activeLearningMaxLowConfidence,
        context_compaction_enabled: contextCompactionEnabled,
        model_context_window: modelContextWindow,
        compaction_threshold: compactionThreshold,
        compaction_keep_recent: compactionKeepRecent,
        personal_memory_enabled: personalMemoryEnabled,
        personal_memory_top_k: personalMemoryTopK,
        personal_memory_max_per_user: personalMemoryMaxPerUser,
        personal_memory_decay_factor: personalMemoryDecayFactor,
        personal_memory_min_relevance: personalMemoryMinRelevance,
      };

      // LLM configs based on mode
      if (llmMode === 'shared') {
        req.llm = formToUpdate(sharedLlm, !!config?.llm?.has_api_key);
        req.remove_query_analyzer_llm = true;
        req.remove_query_rewriter_llm = true;
        req.remove_context_curator_llm = true;
        req.remove_response_generator_llm = true;
        req.remove_quality_guard_llm = true;
        req.remove_language_adapter_llm = true;
        req.remove_orchestrator_llm = true;
      } else if (llmMode === 'per-agent') {
        req.remove_llm = true;
        const agents = ['query_analyzer', 'query_rewriter', 'context_curator', 'response_generator', 'quality_guard', 'language_adapter'] as const;
        for (const a of agents) {
          const form = agentLlms[a];
          if (form && form.model) {
            const key = `${a}_llm` as keyof UpdateChatPipelineRequest;
            const configKey = `${a}_llm` as keyof ChatPipelineConfigResponse;
            const existing = config?.[configKey] as LlmProviderInfo | undefined;
            (req as Record<string, unknown>)[key] = formToUpdate(form, !!existing?.has_api_key);
          }
        }
        // Orchestrator LLM
        const orchForm = agentLlms.pipeline_orchestrator;
        if (orchForm && orchForm.model) {
          const existing = config?.orchestrator_llm;
          req.orchestrator_llm = formToUpdate(orchForm, !!existing?.has_api_key);
        }
      } else {
        // Chat mode: remove all overrides
        req.remove_llm = true;
        req.remove_query_analyzer_llm = true;
        req.remove_query_rewriter_llm = true;
        req.remove_context_curator_llm = true;
        req.remove_response_generator_llm = true;
        req.remove_quality_guard_llm = true;
        req.remove_language_adapter_llm = true;
        req.remove_orchestrator_llm = true;
      }

      // Save feature LLMs (independent of LLM mode — features always have their own LLM config)
      const featureLlmKeys = [
        'memory', 'tool_use', 'self_rag', 'graph_rag', 'map_reduce',
        'ragas', 'compression', 'multimodal', 'raptor', 'colbert', 'personal_memory',
      ] as const;
      for (const stateKey of featureLlmKeys) {
        const form = featureLlms[stateKey];
        const reqKey = `${stateKey}_llm` as keyof UpdateChatPipelineRequest;
        const configKey = `${stateKey}_llm` as keyof ChatPipelineConfigResponse;
        if (form && form.model) {
          const existing = config?.[configKey] as LlmProviderInfo | undefined;
          (req as Record<string, unknown>)[reqKey] = formToUpdate(form, !!existing?.has_api_key);
        } else {
          const removeKey = `remove_${stateKey}_llm` as keyof UpdateChatPipelineRequest;
          (req as Record<string, unknown>)[removeKey] = true;
        }
      }

      const resp = await updateChatPipelineConfig(req);
      setConfig(resp);
      message.success('Chat pipeline settings saved');
    } catch {
      message.error('Failed to save chat pipeline settings');
    } finally {
      setSaving(false);
    }
  }

  if (loading) return <Spin tip="Loading chat pipeline config..." />;

  const agentKeys = Object.keys(chatAgents);

  return (
    <Card
      title={
        <Space>
          <RobotOutlined />
          <span>Response Pipeline</span>
          <Switch
            size="small"
            checked={enabled}
            onChange={setEnabled}
            data-testid="chat-pipeline-switch"
          />
          <Tag color={enabled ? 'green' : 'default'}>{enabled ? 'ON' : 'OFF'}</Tag>
        </Space>
      }
      extra={
        <Button
          type="primary"
          icon={<SaveOutlined />}
          loading={saving}
          onClick={handleSave}
        >
          Save
        </Button>
      }
    >
      {!enabled ? (
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <Paragraph type="secondary">
            When disabled, the legacy 2-agent pipeline (intent classifier + RAG engine) handles all queries.
          </Paragraph>
          <Alert
            type="info"
            showIcon
            message="Enable to activate the intelligent multi-agent pipeline"
            description={
              <ul style={{ margin: '4px 0', paddingLeft: 20 }}>
                <li><strong>Smart routing</strong> — orchestrator skips agents for simple queries (saves cost/latency)</li>
                <li><strong>Query expansion</strong> — Thai↔English term expansion + sub-queries for better retrieval</li>
                <li><strong>Context curation</strong> — LLM selects the most relevant chunks from search results</li>
                <li><strong>Citation-aware generation</strong> — responses include [1][2] source references</li>
                <li><strong>Quality guard</strong> — checks for hallucination before delivering the response</li>
                <li><strong>Language matching</strong> — auto-translates response to match query language</li>
              </ul>
            }
          />
        </Space>
      ) : (
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          {/* Flow diagram */}
          <Alert
            type="info"
            showIcon
            icon={<ThunderboltOutlined />}
            message="Pipeline flow"
            description="Query → Analyzer → Orchestrator → [Rewriter → Search → Reranker → Curator] → Generator → [Quality Guard] → [Language Adapter] → Response"
            style={{ fontSize: 12 }}
          />

          {/* Pipeline Parameters */}
          <Space size="large" wrap>
            <Tooltip title="Maximum estimated tokens for the context window passed to the response generator. Larger values give more context but cost more.">
              <Space direction="vertical" size={2}>
                <Text type="secondary">Max Context Tokens <QuestionCircleOutlined /></Text>
                <InputNumber
                  min={512}
                  max={32768}
                  step={512}
                  value={maxContextTokens}
                  onChange={(v) => v && setMaxContextTokens(v)}
                  style={{ width: 140 }}
                />
              </Space>
            </Tooltip>
            <Tooltip title="Maximum output tokens per agent LLM call. Higher values allow longer agent responses but increase cost.">
              <Space direction="vertical" size={2}>
                <Text type="secondary">Agent Max Tokens <QuestionCircleOutlined /></Text>
                <InputNumber
                  min={256}
                  max={8192}
                  step={256}
                  value={agentMaxTokens}
                  onChange={(v) => v && setAgentMaxTokens(v)}
                  style={{ width: 140 }}
                />
              </Space>
            </Tooltip>
            <Tooltip title="Per-LLM-call timeout in seconds. Increase if you have large document sets or slow models. This affects the backend — the Test Chat page also has a separate client-side timeout.">
              <Space direction="vertical" size={2}>
                <Text type="secondary">LLM Timeout <QuestionCircleOutlined /></Text>
                <Select
                  value={requestTimeoutSecs}
                  onChange={setRequestTimeoutSecs}
                  style={{ width: 140 }}
                  options={[
                    { label: '30 seconds', value: 30 },
                    { label: '1 minute', value: 60 },
                    { label: '2 minutes', value: 120 },
                    { label: '5 minutes', value: 300 },
                    { label: '10 minutes', value: 600 },
                  ]}
                />
              </Space>
            </Tooltip>
            <Tooltip title="Ollama keep_alive controls how long models stay loaded in GPU memory after a request. 'Keep forever' (-1) avoids cold-start delays but uses more VRAM. Only applies to Ollama providers.">
              <Space direction="vertical" size={2}>
                <Text type="secondary">Ollama Keep Alive <QuestionCircleOutlined /></Text>
                <Select
                  value={ollamaKeepAlive}
                  onChange={setOllamaKeepAlive}
                  style={{ width: 140 }}
                  options={[
                    { label: 'Unload immediately', value: '0' },
                    { label: '5 minutes', value: '5m' },
                    { label: '15 minutes', value: '15m' },
                    { label: '30 minutes', value: '30m' },
                    { label: '1 hour', value: '1h' },
                    { label: 'Keep forever', value: '-1' },
                  ]}
                />
              </Space>
            </Tooltip>
          </Space>

          <Divider style={{ margin: '8px 0' }} />

          {/* LLM Mode */}
          <Space direction="vertical" size="small">
            <Text strong>
              Agent LLM Configuration{' '}
              <Tooltip title="Choose how agents get their LLM. 'Use Chat LLM' = all agents share the main LLM above (cheapest). 'Shared' = all agents use a dedicated LLM. 'Per-Agent' = each agent can have its own LLM (most flexible, use small models for light agents).">
                <QuestionCircleOutlined />
              </Tooltip>
            </Text>
            <Segmented
              options={[
                { label: 'Use Chat LLM', value: 'chat' },
                { label: 'Shared', value: 'shared' },
                { label: 'Per-Agent', value: 'per-agent' },
              ]}
              value={llmMode}
              onChange={(v) => setLlmMode(v as 'chat' | 'shared' | 'per-agent')}
            />
          </Space>

          {llmMode === 'chat' && (
            <Paragraph type="secondary" style={{ margin: 0 }}>
              All agents use the main Chat LLM configured above. Simplest and cheapest option.
            </Paragraph>
          )}

          {llmMode === 'shared' && (
            <>
              <Text type="secondary">Dedicated LLM shared by all pipeline agents:</Text>
              <LlmConfigForm
                form={sharedLlm}
                onChange={setSharedLlm}
                syncedModels={syncedModels}
                onSync={handleSync}
                syncing={syncing}
              />
            </>
          )}

          {llmMode === 'per-agent' && (
            <>
              <Paragraph type="secondary" style={{ margin: 0 }}>
                Each agent can use a different LLM. Use small/fast models for light agents (Analyzer, Curator, Guard) and your best model for the Response Generator.
              </Paragraph>
              {(() => {
                const missingAgents = Object.keys(chatAgents).filter((key) => {
                  const info = chatAgents[key];
                  const isOn = info.alwaysOn || agentToggles[key];
                  if (!isOn) return false;
                  const form = agentLlms[key];
                  return !form || !form.model;
                });
                if (missingAgents.length > 0) {
                  const names = missingAgents.map((k) => chatAgents[k].label).join(', ');
                  return (
                    <Alert
                      type="warning"
                      showIcon
                      message="Agents without model override"
                      description={`The following enabled agents have no model configured and will fall back to the main Chat LLM Provider: ${names}. Expand each agent to set a model.`}
                      style={{ marginTop: 8 }}
                    />
                  );
                }
                return null;
              })()}
            </>
          )}

          <Divider style={{ margin: '8px 0' }} />

          {/* Agent Panels */}
          <Text strong>Agents</Text>
          <Collapse
            size="small"
            items={agentKeys.map((key) => {
              const info = chatAgents[key];
              const isOn = info.alwaysOn || agentToggles[key];
              return {
                key,
                label: (
                  <Space>
                    <span>{info.label}</span>
                    {info.alwaysOn ? (
                      <Tag color="blue">Always On</Tag>
                    ) : (
                      <Switch
                        size="small"
                        checked={agentToggles[key]}
                        onChange={(v) => setAgentToggles((prev) => ({ ...prev, [key]: v }))}
                        onClick={(_, e) => e.stopPropagation()}
                      />
                    )}
                    <Tag color={isOn ? 'green' : 'default'}>{isOn ? 'ON' : 'OFF'}</Tag>
                    <Tag color={info.taskWeight === 'Heavy' ? 'red' : info.taskWeight === 'Medium' ? 'orange' : 'green'} style={{ fontSize: 11 }}>
                      {info.taskWeight}
                    </Tag>
                    {llmMode === 'per-agent' && isOn && (() => {
                      const form = agentLlms[key];
                      const model = form?.model;
                      return model ? (
                        <Tag color="purple" style={{ fontSize: 11 }}>{form?.kind}: {model}</Tag>
                      ) : (
                        <Tag color="warning" style={{ fontSize: 11 }}>No model (uses fallback)</Tag>
                      );
                    })()}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>{info.description}</Paragraph>
                    <AgentHints info={info} />

                    {/* Pipeline Orchestrator specific settings */}
                    {key === 'pipeline_orchestrator' && isOn && (
                      <Space size="large" wrap style={{ marginTop: 4 }}>
                        <Tooltip title="Maximum LLM calls the orchestrator can make per request. Set to 0 for heuristic-only (free, zero-latency).">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max LLM Calls <QuestionCircleOutlined /></Text>
                            <InputNumber
                              min={0}
                              max={10}
                              value={maxOrchestratorCalls}
                              onChange={(v) => v != null && setMaxOrchestratorCalls(v)}
                              style={{ width: 100 }}
                            />
                          </Space>
                        </Tooltip>
                        {maxOrchestratorCalls === 0 && (
                          <Tag color="cyan">Heuristic-only (free)</Tag>
                        )}
                      </Space>
                    )}

                    {/* Quality Guard specific settings */}
                    {key === 'quality_guard' && isOn && (
                      <Space size="large" wrap style={{ marginTop: 4 }}>
                        <Tooltip title="How many times to retry generation if quality check fails.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Retries <QuestionCircleOutlined /></Text>
                            <InputNumber
                              min={0}
                              max={5}
                              value={qualityMaxRetries}
                              onChange={(v) => v != null && setQualityMaxRetries(v)}
                              style={{ width: 100 }}
                            />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Minimum combined quality score (relevance + completeness - hallucination) to pass. Lower = more permissive, higher = stricter.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Threshold <QuestionCircleOutlined /></Text>
                            <Slider
                              min={0}
                              max={1}
                              step={0.05}
                              value={qualityThreshold}
                              onChange={setQualityThreshold}
                              style={{ width: 200 }}
                            />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}

                    {/* Per-agent LLM config */}
                    {llmMode === 'per-agent' && isOn && (
                      <>
                        <Divider style={{ margin: '4px 0' }} />
                        <Text type="secondary">Agent LLM Override:</Text>
                        <LlmConfigForm
                          form={agentLlms[key] || defaultLlmForm}
                          onChange={(f) => setAgentLlms((prev) => ({ ...prev, [key]: f }))}
                          syncedModels={syncedModels}
                          onSync={handleSync}
                          syncing={syncing}
                        />
                      </>
                    )}
                  </Space>
                ),
              };
            })}
          />

          <Divider style={{ margin: '8px 0' }} />

          {/* ── Advanced Features ─────────────────────────── */}
          <Text strong>Advanced Features</Text>
          <Collapse
            size="small"
            items={[
              {
                key: 'conversation_memory',
                label: (
                  <Space>
                    <span>Conversation Memory</span>
                    <Switch
                      size="small"
                      checked={conversationMemoryEnabled}
                      onChange={setConversationMemoryEnabled}
                      onClick={(_, e) => e.stopPropagation()}
                    />
                    <Tag color={conversationMemoryEnabled ? 'green' : 'default'}>
                      {conversationMemoryEnabled ? 'ON' : 'OFF'}
                    </Tag>
                    {featureModelTag('memory', conversationMemoryEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Maintains lightweight per-user conversation summaries across sessions. The system remembers
                      past topics and context, providing more personalized and coherent responses over time.
                    </Paragraph>
                    {conversationMemoryEnabled && (
                      <Space size="large" wrap>
                        <Tooltip title="Maximum number of conversation summaries to retain per user.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Summaries <QuestionCircleOutlined /></Text>
                            <InputNumber
                              min={1}
                              max={50}
                              value={memoryMaxSummaries}
                              onChange={(v) => v && setMemoryMaxSummaries(v)}
                              style={{ width: 100 }}
                            />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Max tokens for each conversation summary. Smaller = more compact memories.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Summary Max Tokens <QuestionCircleOutlined /></Text>
                            <InputNumber
                              min={64}
                              max={1024}
                              step={64}
                              value={memorySummaryMaxTokens}
                              onChange={(v) => v && setMemorySummaryMaxTokens(v)}
                              style={{ width: 120 }}
                            />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}
                    {featureLlmForm('memory', conversationMemoryEnabled)}
                  </Space>
                ),
              },
              {
                key: 'retrieval_refinement',
                label: (
                  <Space>
                    <span>Retrieval Refinement</span>
                    <Switch
                      size="small"
                      checked={retrievalRefinementEnabled}
                      onChange={setRetrievalRefinementEnabled}
                      onClick={(_, e) => e.stopPropagation()}
                    />
                    <Tag color={retrievalRefinementEnabled ? 'green' : 'default'}>
                      {retrievalRefinementEnabled ? 'ON' : 'OFF'}
                    </Tag>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Automatically retries search with reformulated queries when the initial retrieval quality
                      is below the minimum relevance threshold. Improves answer quality for ambiguous queries.
                    </Paragraph>
                    {retrievalRefinementEnabled && (
                      <Space size="large" wrap>
                        <Tooltip title="Minimum average relevance score (0-1) for retrieved results. Below this triggers a retry with reformulated query.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Min Relevance <QuestionCircleOutlined /></Text>
                            <Slider
                              min={0}
                              max={1}
                              step={0.05}
                              value={refinementMinRelevance}
                              onChange={setRefinementMinRelevance}
                              style={{ width: 200 }}
                            />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Maximum number of retrieval retry attempts.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Retries <QuestionCircleOutlined /></Text>
                            <InputNumber
                              min={1}
                              max={5}
                              value={refinementMaxRetries}
                              onChange={(v) => v && setRefinementMaxRetries(v)}
                              style={{ width: 100 }}
                            />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}
                  </Space>
                ),
              },
              {
                key: 'tool_use',
                label: (
                  <Space>
                    <span>Agentic Tool Use</span>
                    <Switch
                      size="small"
                      checked={toolUseEnabled}
                      onChange={setToolUseEnabled}
                      onClick={(_, e) => e.stopPropagation()}
                    />
                    <Tag color={toolUseEnabled ? 'green' : 'default'}>
                      {toolUseEnabled ? 'ON' : 'OFF'}
                    </Tag>
                    {featureModelTag('tool_use', toolUseEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      The LLM decides which knowledge bases (workspaces) to search and what strategy to use
                      (broad, keyword, semantic, targeted). Enables intelligent multi-workspace queries and
                      cross-domain reasoning.
                    </Paragraph>
                    {toolUseEnabled && (
                      <Tooltip title="Maximum number of tool calls (searches) the LLM can make per request.">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Max Tool Calls <QuestionCircleOutlined /></Text>
                          <InputNumber
                            min={1}
                            max={10}
                            value={toolUseMaxCalls}
                            onChange={(v) => v && setToolUseMaxCalls(v)}
                            style={{ width: 100 }}
                          />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('tool_use', toolUseEnabled)}
                  </Space>
                ),
              },
              {
                key: 'adaptive_threshold',
                label: (
                  <Space>
                    <span>Adaptive Quality Thresholds</span>
                    <Switch
                      size="small"
                      checked={adaptiveThresholdEnabled}
                      onChange={setAdaptiveThresholdEnabled}
                      onClick={(_, e) => e.stopPropagation()}
                    />
                    <Tag color={adaptiveThresholdEnabled ? 'green' : 'default'}>
                      {adaptiveThresholdEnabled ? 'ON' : 'OFF'}
                    </Tag>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Tracks user feedback (thumbs up/down) and automatically adjusts the quality guard threshold.
                      High positive feedback → more lenient threshold. Low positive feedback → stricter threshold.
                    </Paragraph>
                    {adaptiveThresholdEnabled && (
                      <>
                        <Space size="large" wrap>
                          <Tooltip title="Number of days before old feedback data decays in weight.">
                            <Space direction="vertical" size={2}>
                              <Text type="secondary">Feedback Decay Days <QuestionCircleOutlined /></Text>
                              <InputNumber
                                min={7}
                                max={365}
                                value={feedbackDecayDays}
                                onChange={(v) => v && setFeedbackDecayDays(v)}
                                style={{ width: 100 }}
                              />
                            </Space>
                          </Tooltip>
                          <Tooltip title="Minimum feedback samples before adaptive threshold kicks in.">
                            <Space direction="vertical" size={2}>
                              <Text type="secondary">Min Samples <QuestionCircleOutlined /></Text>
                              <InputNumber
                                min={5}
                                max={100}
                                value={adaptiveMinSamples}
                                onChange={(v) => v && setAdaptiveMinSamples(v)}
                                style={{ width: 100 }}
                              />
                            </Space>
                          </Tooltip>
                        </Space>
                        {feedbackStats && (
                          <Alert
                            type="info"
                            showIcon
                            message="Feedback Statistics"
                            description={
                              <Space size="large" wrap>
                                <Text>Total: <strong>{feedbackStats.total}</strong></Text>
                                <Text>Positive: <strong>{feedbackStats.positive}</strong></Text>
                                <Text>Negative: <strong>{feedbackStats.negative}</strong></Text>
                                <Text>Rate: <strong>{(feedbackStats.positive_rate * 100).toFixed(1)}%</strong></Text>
                                <Text>Current Threshold: <strong>{feedbackStats.current_threshold.toFixed(2)}</strong></Text>
                                {feedbackStats.adaptive_threshold != null && (
                                  <Text>Adaptive: <strong>{feedbackStats.adaptive_threshold.toFixed(2)}</strong></Text>
                                )}
                              </Space>
                            }
                          />
                        )}
                      </>
                    )}
                  </Space>
                ),
              },
            ]}
          />

          <Divider style={{ margin: '8px 0' }} />

          {/* ── Next-Gen RAG Features ─────────────────── */}
          <Text strong>Next-Gen RAG</Text>
          <Collapse
            size="small"
            items={[
              {
                key: 'self_rag',
                label: (
                  <Space>
                    <span>Self-RAG</span>
                    <Switch size="small" checked={selfRagEnabled} onChange={setSelfRagEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={selfRagEnabled ? 'green' : 'default'}>{selfRagEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('self_rag', selfRagEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Decides whether document retrieval is needed before searching. Skips the entire search pipeline
                      for greetings, general knowledge, and follow-ups — saving latency and cost.
                    </Paragraph>
                    {selfRagEnabled && (
                      <Tooltip title="Minimum confidence (0-1) to skip retrieval. Higher = more conservative (retrieves more often).">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Skip Threshold <QuestionCircleOutlined /></Text>
                          <Slider min={0.3} max={1} step={0.05} value={selfRagThreshold} onChange={setSelfRagThreshold} style={{ width: 200 }} />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('self_rag', selfRagEnabled)}
                  </Space>
                ),
              },
              {
                key: 'graph_rag',
                label: (
                  <Space>
                    <span>Graph RAG</span>
                    <Switch size="small" checked={graphRagEnabled} onChange={setGraphRagEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={graphRagEnabled ? 'green' : 'default'}>{graphRagEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('graph_rag', graphRagEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Extracts named entities (people, organizations, locations, concepts) from documents and builds
                      a knowledge graph. During retrieval, traverses entity relationships to find additional relevant
                      context that keyword/vector search might miss.
                    </Paragraph>
                    {graphRagEnabled && (
                      <Space size="large" wrap>
                        <Tooltip title="Maximum entities to extract per chunk.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Entities <QuestionCircleOutlined /></Text>
                            <InputNumber min={1} max={50} value={graphRagMaxEntities} onChange={(v) => v && setGraphRagMaxEntities(v)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Graph traversal depth (hops from seed entities). Higher = broader but slower.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Depth <QuestionCircleOutlined /></Text>
                            <InputNumber min={1} max={5} value={graphRagMaxDepth} onChange={(v) => v && setGraphRagMaxDepth(v)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}
                    {featureLlmForm('graph_rag', graphRagEnabled)}
                  </Space>
                ),
              },
              {
                key: 'crag',
                label: (
                  <Space>
                    <span>Corrective RAG (CRAG)</span>
                    <Switch size="small" checked={cragEnabled} onChange={setCragEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={cragEnabled ? 'green' : 'default'}>{cragEnabled ? 'ON' : 'OFF'}</Tag>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Evaluates retrieved context quality. If the local knowledge base doesn't have good answers,
                      falls back to web search. Combines local + web results for comprehensive responses.
                    </Paragraph>
                    {cragEnabled && (
                      <Space direction="vertical" size="small" style={{ width: '100%' }}>
                        <Space size="large" wrap>
                          <Tooltip title="Minimum relevance score to accept local context without web fallback.">
                            <Space direction="vertical" size={2}>
                              <Text type="secondary">Relevance Threshold <QuestionCircleOutlined /></Text>
                              <Slider min={0} max={1} step={0.05} value={cragRelevanceThreshold} onChange={setCragRelevanceThreshold} style={{ width: 200 }} />
                            </Space>
                          </Tooltip>
                          <Tooltip title="Maximum web search results to fetch.">
                            <Space direction="vertical" size={2}>
                              <Text type="secondary">Max Web Results <QuestionCircleOutlined /></Text>
                              <InputNumber min={1} max={20} value={cragMaxWebResults} onChange={(v) => v && setCragMaxWebResults(v)} style={{ width: 100 }} />
                            </Space>
                          </Tooltip>
                        </Space>
                        <Tooltip title="URL of your web search API endpoint (e.g., SearXNG, Google Custom Search proxy). Leave empty to disable web fallback.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Web Search URL <QuestionCircleOutlined /></Text>
                            <Input
                              value={cragWebSearchUrl}
                              onChange={(e) => setCragWebSearchUrl(e.target.value)}
                              placeholder="https://search-api.example.com/search"
                              style={{ width: 400 }}
                            />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}
                  </Space>
                ),
              },
              {
                key: 'speculative_rag',
                label: (
                  <Space>
                    <span>Speculative RAG</span>
                    <Switch size="small" checked={speculativeRagEnabled} onChange={setSpeculativeRagEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={speculativeRagEnabled ? 'green' : 'default'}>{speculativeRagEnabled ? 'ON' : 'OFF'}</Tag>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Generates multiple candidate responses in parallel using different strategies (detailed, concise,
                      analytical, comparative), then uses LLM-based ranking to select the best one.
                      Higher quality but uses more LLM calls.
                    </Paragraph>
                    {speculativeRagEnabled && (
                      <Tooltip title="Number of parallel response candidates. More = better quality but higher cost.">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Candidates <QuestionCircleOutlined /></Text>
                          <InputNumber min={2} max={5} value={speculativeCandidates} onChange={(v) => v && setSpeculativeCandidates(v)} style={{ width: 100 }} />
                        </Space>
                      </Tooltip>
                    )}
                  </Space>
                ),
              },
              {
                key: 'map_reduce',
                label: (
                  <Space>
                    <span>Map-Reduce RAG</span>
                    <Switch size="small" checked={mapReduceEnabled} onChange={setMapReduceEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={mapReduceEnabled ? 'green' : 'default'}>{mapReduceEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('map_reduce', mapReduceEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      For complex synthesis queries that span many documents: independently extracts relevant info
                      from each chunk (MAP), then synthesizes all extractions into a coherent answer (REDUCE).
                      Activates automatically for complex queries with 8+ search results.
                    </Paragraph>
                    {mapReduceEnabled && (
                      <Tooltip title="Maximum chunks to process in the MAP phase. Higher = more comprehensive but slower.">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Max Chunks <QuestionCircleOutlined /></Text>
                          <InputNumber min={5} max={50} value={mapReduceMaxChunks} onChange={(v) => v && setMapReduceMaxChunks(v)} style={{ width: 100 }} />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('map_reduce', mapReduceEnabled)}
                  </Space>
                ),
              },
              {
                key: 'ragas',
                label: (
                  <Space>
                    <span>RAGAS Evaluation</span>
                    <Switch size="small" checked={ragasEnabled} onChange={setRagasEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={ragasEnabled ? 'green' : 'default'}>{ragasEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('ragas', ragasEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Automated quality benchmarking using RAGAS-style metrics. Evaluates a sample of responses for:
                      faithfulness (supported by context?), answer relevancy (answers the question?),
                      and context precision (retrieved context useful?). Scores are logged for monitoring.
                    </Paragraph>
                    {ragasEnabled && (
                      <Tooltip title="Fraction of responses to evaluate (0.0-1.0). E.g., 0.1 = evaluate 10% of requests.">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Sample Rate <QuestionCircleOutlined /></Text>
                          <Slider min={0} max={1} step={0.05} value={ragasSampleRate} onChange={setRagasSampleRate} style={{ width: 200 }} />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('ragas', ragasEnabled)}
                  </Space>
                ),
              },
              {
                key: 'compression',
                label: (
                  <Space>
                    <span>Contextual Compression</span>
                    <Switch size="small" checked={compressionEnabled} onChange={setCompressionEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={compressionEnabled ? 'green' : 'default'}>{compressionEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('compression', compressionEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      LLMLingua-style context compression: uses an LLM to identify and remove low-importance
                      content from retrieved chunks, reducing context size while preserving information density.
                      Helps fit more relevant content within the token budget.
                    </Paragraph>
                    {compressionEnabled && (
                      <Tooltip title="Target compression ratio (0.1-1.0). E.g., 0.5 means compress to ~50% of original size.">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Target Ratio <QuestionCircleOutlined /></Text>
                          <Slider min={0.1} max={1} step={0.05} value={compressionTargetRatio} onChange={setCompressionTargetRatio} style={{ width: 200 }} />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('compression', compressionEnabled)}
                  </Space>
                ),
              },
              {
                key: 'multimodal',
                label: (
                  <Space>
                    <span>Multi-modal RAG</span>
                    <Switch size="small" checked={multimodalEnabled} onChange={setMultimodalEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={multimodalEnabled ? 'green' : 'default'}>{multimodalEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('multimodal', multimodalEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Enriches context with image descriptions when documents contain embedded images.
                      Detects image references (markdown/HTML) in chunks and uses an LLM to generate
                      text descriptions, making visual content searchable and usable in responses.
                    </Paragraph>
                    {multimodalEnabled && (
                      <Tooltip title="Maximum number of images to describe per request.">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Max Images per Request <QuestionCircleOutlined /></Text>
                          <InputNumber min={1} max={20} value={multimodalMaxImages} onChange={v => setMultimodalMaxImages(v ?? 5)} />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('multimodal', multimodalEnabled)}
                  </Space>
                ),
              },
              {
                key: 'raptor',
                label: (
                  <Space>
                    <span>RAPTOR Hierarchical Summaries</span>
                    <Switch size="small" checked={raptorEnabled} onChange={setRaptorEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={raptorEnabled ? 'green' : 'default'}>{raptorEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('raptor', raptorEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Recursive Abstractive Processing for Tree-Organized Retrieval. Builds a hierarchy
                      of summaries over retrieved chunks — section summaries, topic summaries, and overviews.
                      Enables both detail-oriented and high-level synthesis answers from the same results.
                    </Paragraph>
                    {raptorEnabled && (
                      <>
                        <Tooltip title="Maximum levels of summarization above the leaf chunks.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Tree Depth <QuestionCircleOutlined /></Text>
                            <InputNumber min={1} max={5} value={raptorMaxDepth} onChange={v => setRaptorMaxDepth(v ?? 2)} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="How many chunks to group together for each summary.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Group Size <QuestionCircleOutlined /></Text>
                            <InputNumber min={2} max={10} value={raptorGroupSize} onChange={v => setRaptorGroupSize(v ?? 3)} />
                          </Space>
                        </Tooltip>
                      </>
                    )}
                    {featureLlmForm('raptor', raptorEnabled)}
                  </Space>
                ),
              },
              {
                key: 'colbert',
                label: (
                  <Space>
                    <span>ColBERT Late Interaction Reranking</span>
                    <Switch size="small" checked={colbertEnabled} onChange={setColbertEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={colbertEnabled ? 'green' : 'default'}>{colbertEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('colbert', colbertEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Fine-grained LLM-based reranking inspired by ColBERT&apos;s late interaction paradigm.
                      Evaluates each search result across multiple aspects: exact match, semantic match,
                      completeness, and specificity. Blends LLM scores with original search scores for
                      more precise retrieval ranking.
                    </Paragraph>
                    {colbertEnabled && (
                      <Tooltip title="Number of top search results to rerank (remaining results keep original scores).">
                        <Space direction="vertical" size={2}>
                          <Text type="secondary">Top-N to Rerank <QuestionCircleOutlined /></Text>
                          <InputNumber min={1} max={50} value={colbertTopN} onChange={v => setColbertTopN(v ?? 10)} />
                        </Space>
                      </Tooltip>
                    )}
                    {featureLlmForm('colbert', colbertEnabled)}
                  </Space>
                ),
              },
              {
                key: 'active_learning',
                label: (
                  <Space>
                    <span>Active Learning</span>
                    <Switch size="small" checked={activeLearningEnabled} onChange={setActiveLearningEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={activeLearningEnabled ? 'green' : 'default'}>{activeLearningEnabled ? 'ON' : 'OFF'}</Tag>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Tracks user feedback at the chunk level to improve retrieval over time. Chunks that
                      consistently appear in positively-rated responses get score boosts; chunks in negatively-rated
                      responses get penalized. Also identifies low-confidence queries for review.
                    </Paragraph>
                    {activeLearningEnabled && (
                      <>
                        <Tooltip title="Minimum feedback interactions before adjusting a chunk's quality score.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Min Interactions <QuestionCircleOutlined /></Text>
                            <InputNumber min={1} max={50} value={activeLearningMinInteractions} onChange={v => setActiveLearningMinInteractions(v ?? 5)} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Maximum number of low-confidence queries to track for review.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Low-Confidence Queries <QuestionCircleOutlined /></Text>
                            <InputNumber min={10} max={1000} value={activeLearningMaxLowConfidence} onChange={v => setActiveLearningMaxLowConfidence(v ?? 100)} />
                          </Space>
                        </Tooltip>
                      </>
                    )}
                  </Space>
                ),
              },
              {
                key: 'context_compaction',
                label: (
                  <Space>
                    <span>Context Compaction</span>
                    <Switch size="small" checked={contextCompactionEnabled} onChange={setContextCompactionEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={contextCompactionEnabled ? 'green' : 'default'}>{contextCompactionEnabled ? 'ON' : 'OFF'}</Tag>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Automatically compacts conversation history when approaching the model's context window limit.
                      Older messages are summarized while keeping recent messages intact — like Claude Code's context compaction.
                    </Paragraph>
                    {contextCompactionEnabled && (
                      <Space size="large" wrap>
                        <Tooltip title="Model's context window size in tokens. 0 = use max_context_tokens as estimate.">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Model Context Window <QuestionCircleOutlined /></Text>
                            <InputNumber min={0} step={1024} value={modelContextWindow} onChange={v => setModelContextWindow(v ?? 0)} style={{ width: 140 }} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Trigger compaction when context exceeds this fraction of the window (0.0\u20131.0)">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Compaction Threshold <QuestionCircleOutlined /></Text>
                            <InputNumber min={0.5} max={1.0} step={0.05} value={compactionThreshold} onChange={v => setCompactionThreshold(v ?? 0.8)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Number of recent messages to keep intact during compaction">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Keep Recent Messages <QuestionCircleOutlined /></Text>
                            <InputNumber min={2} max={20} value={compactionKeepRecent} onChange={v => setCompactionKeepRecent(v ?? 6)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}
                  </Space>
                ),
              },
              {
                key: 'personal_memory',
                label: (
                  <Space>
                    <span>Personal Memory</span>
                    <Switch size="small" checked={personalMemoryEnabled} onChange={setPersonalMemoryEnabled} onClick={(_, e) => e.stopPropagation()} />
                    <Tag color={personalMemoryEnabled ? 'green' : 'default'}>{personalMemoryEnabled ? 'ON' : 'OFF'}</Tag>
                    {featureModelTag('personal_memory', personalMemoryEnabled)}
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size="small" style={{ width: '100%' }}>
                    <Paragraph style={{ margin: 0 }}>
                      Stores per-user memories in the vector database. During conversation, relevant past memories are
                      retrieved and injected into context — giving each user a personalized AI assistant that remembers
                      their preferences, facts, and decisions.
                    </Paragraph>
                    {personalMemoryEnabled && (
                      <Space size="large" wrap>
                        <Tooltip title="Max personal memories to retrieve per query">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Memories Per Query <QuestionCircleOutlined /></Text>
                            <InputNumber min={1} max={20} value={personalMemoryTopK} onChange={v => setPersonalMemoryTopK(v ?? 5)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Maximum memories stored per user">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Max Per User <QuestionCircleOutlined /></Text>
                            <InputNumber min={10} max={1000} step={10} value={personalMemoryMaxPerUser} onChange={v => setPersonalMemoryMaxPerUser(v ?? 200)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Daily relevance decay (0.95 = loses 5% relevance per day)">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Decay Factor <QuestionCircleOutlined /></Text>
                            <InputNumber min={0.5} max={1.0} step={0.01} value={personalMemoryDecayFactor} onChange={v => setPersonalMemoryDecayFactor(v ?? 0.95)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                        <Tooltip title="Memories below this score are pruned">
                          <Space direction="vertical" size={2}>
                            <Text type="secondary">Min Relevance <QuestionCircleOutlined /></Text>
                            <InputNumber min={0.01} max={0.5} step={0.01} value={personalMemoryMinRelevance} onChange={v => setPersonalMemoryMinRelevance(v ?? 0.1)} style={{ width: 100 }} />
                          </Space>
                        </Tooltip>
                      </Space>
                    )}
                    {featureLlmForm('personal_memory', personalMemoryEnabled)}
                  </Space>
                ),
              },
            ]}
          />

          {/* Streaming note */}
          <Alert
            type="info"
            showIcon
            message="Streaming mode — 3-layer hallucination defense"
            description={
              <ul style={{ margin: '4px 0', paddingLeft: 20, fontSize: 12 }}>
                <li><strong>Pre-stream:</strong> If retrieved context is insufficient or irrelevant, the system responds honestly instead of guessing</li>
                <li><strong>Guided generation:</strong> Low-confidence context triggers stronger anti-hallucination instructions in the system prompt</li>
                <li><strong>Post-stream:</strong> Quality Guard runs after streaming completes — if it detects hallucination, a warning is appended to the response</li>
                <li><em>Language Adapter is skipped in streaming mode (response language depends on the generator model)</em></li>
              </ul>
            }
          />
        </Space>
      )}
    </Card>
  );
}
