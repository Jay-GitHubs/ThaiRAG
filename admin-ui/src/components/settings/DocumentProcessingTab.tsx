import { useEffect, useState, useCallback } from 'react';
import {
  Card,
  Switch,
  InputNumber,
  Slider,
  Button,
  Typography,
  Space,
  Descriptions,
  Alert,
  Spin,
  message,
  Divider,
  Select,
  Input,
  Collapse,
  Tag,
  Tooltip,
  Segmented,
  theme,
} from 'antd';
import {
  RobotOutlined,
  SettingOutlined,
  SaveOutlined,
  ThunderboltOutlined,
  SyncOutlined,
  ReloadOutlined,
  ClusterOutlined,
  QuestionCircleOutlined,
} from '@ant-design/icons';
import { getDocumentConfig, updateDocumentConfig, syncModels, getProviderConfig, updateProviderConfig, syncEmbeddingModels } from '../../api/settings';
import type {
  AiPreprocessingConfig,
  AvailableModel,
  DocumentConfigResponse,
  LlmConfigUpdate,
  LlmProviderInfo,
  ProviderConfigResponse,
  UpdateDocumentConfigRequest,
} from '../../api/types';

const { Text, Paragraph } = Typography;

type TaskWeight = 'Light' | 'Medium' | 'Heavy' | undefined;

interface StaticModel {
  label: string;
  value: string;
  tier: 'light' | 'medium' | 'heavy';
  vision?: boolean;
}

const staticModels: Record<string, StaticModel[]> = {
  Claude: [
    { label: 'Claude Opus 4', value: 'claude-opus-4-20250514', tier: 'heavy', vision: true },
    { label: 'Claude Sonnet 4', value: 'claude-sonnet-4-20250514', tier: 'heavy', vision: true },
    { label: 'Claude Haiku 4', value: 'claude-haiku-4-20250414', tier: 'light', vision: true },
    { label: 'Claude 3.5 Sonnet', value: 'claude-3-5-sonnet-20241022', tier: 'medium', vision: true },
  ],
  OpenAi: [
    { label: 'GPT-4o', value: 'gpt-4o', tier: 'heavy', vision: true },
    { label: 'GPT-4o Mini', value: 'gpt-4o-mini', tier: 'medium', vision: true },
    { label: 'GPT-4.1', value: 'gpt-4.1', tier: 'heavy', vision: true },
    { label: 'GPT-4.1 Mini', value: 'gpt-4.1-mini', tier: 'medium', vision: true },
    { label: 'GPT-4.1 Nano', value: 'gpt-4.1-nano', tier: 'light', vision: true },
    { label: 'o3', value: 'o3', tier: 'heavy', vision: true },
    { label: 'o3 Mini', value: 'o3-mini', tier: 'medium', vision: true },
    { label: 'o4 Mini', value: 'o4-mini', tier: 'medium', vision: true },
  ],
  Gemini: [
    { label: 'Gemini 2.5 Pro', value: 'gemini-2.5-pro', tier: 'heavy', vision: true },
    { label: 'Gemini 2.5 Flash', value: 'gemini-2.5-flash', tier: 'medium', vision: true },
    { label: 'Gemini 2.0 Flash', value: 'gemini-2.0-flash', tier: 'light', vision: true },
    { label: 'Gemini 1.5 Pro', value: 'gemini-1.5-pro', tier: 'heavy', vision: true },
    { label: 'Gemini 1.5 Flash', value: 'gemini-1.5-flash', tier: 'light', vision: true },
  ],
};

// Known Ollama vision model prefixes (matched against model ID before the colon/tag)
const OLLAMA_VISION_PREFIXES = [
  'llava', 'llama3.2-vision', 'minicpm-v', 'bakllava',
  'moondream', 'moondream2', 'cogvlm', 'internvl',
  'qwen2.5vl', 'qwen2-vl', 'qwenvl', 'gemma3',
];

function isOllamaVisionModel(modelId: string): boolean {
  const lower = modelId.toLowerCase();
  const base = lower.split(':')[0]; // strip tag like ":latest", ":13b-q4_0"
  return OLLAMA_VISION_PREFIXES.some((p) => base === p || base.startsWith(p + '-'));
}

// Ollama model size limits per task weight (in bytes)
// Light: ≤5GB (~1-4B params), Medium: ≤10GB (~4-8B params), Heavy: all
const OLLAMA_SIZE_LIMITS: Record<string, number> = {
  Light: 5 * 1024 * 1024 * 1024,
  Medium: 10 * 1024 * 1024 * 1024,
};

// Cloud model tier filtering: Light shows light, Medium shows light+medium, Heavy shows all
const TIER_ALLOWED: Record<string, Set<string>> = {
  Light: new Set(['light']),
  Medium: new Set(['light', 'medium']),
  Heavy: new Set(['light', 'medium', 'heavy']),
};

function filterSyncedModels(models: AvailableModel[], taskWeight: TaskWeight): AvailableModel[] {
  if (!taskWeight) return models;
  const limit = OLLAMA_SIZE_LIMITS[taskWeight];
  if (!limit) return models; // Heavy = no limit
  return models.filter((m) => !m.size || m.size <= limit);
}

function filterStaticModels(models: StaticModel[], taskWeight: TaskWeight): StaticModel[] {
  if (!taskWeight) return models;
  const allowed = TIER_ALLOWED[taskWeight];
  if (!allowed) return models;
  return models.filter((m) => allowed.has(m.tier));
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

interface LlmFormState {
  kind: string;
  model: string;
  base_url: string;
  api_key: string;
}

const defaultLlmForm: LlmFormState = {
  kind: 'Ollama',
  model: '',
  base_url: 'http://localhost:11435',
  api_key: '',
};

function llmInfoToForm(info: LlmProviderInfo): LlmFormState {
  return {
    kind: info.kind,
    model: info.model,
    base_url: info.base_url || (info.kind === 'Ollama' ? 'http://localhost:11435' : ''),
    api_key: '',
  };
}

const providerOptions = [
  { label: 'Ollama (Local)', value: 'Ollama' },
  { label: 'Claude (Anthropic)', value: 'Claude' },
  { label: 'OpenAI', value: 'OpenAi' },
  { label: 'Gemini (Google)', value: 'Gemini' },
  { label: 'OpenAI-Compatible', value: 'OpenAiCompatible' },
];

// ── Reusable LLM Config Form ─────────────────────────────────────────

interface LlmConfigFormProps {
  form: LlmFormState;
  onChange: (form: LlmFormState) => void;
  existingKey?: boolean;
  compact?: boolean;
  /** When set, filters synced/static models to fit the agent's task weight. */
  taskWeight?: TaskWeight;
  /** When true, only show vision-capable models (for converter with OCR). */
  requireVision?: boolean;
  /** Called when models are synced, reporting model id → size mapping. */
  onModelsLoaded?: (models: AvailableModel[]) => void;
}

function LlmConfigForm({ form, onChange, existingKey, compact, taskWeight, requireVision, onModelsLoaded }: LlmConfigFormProps) {
  const [syncedModels, setSyncedModels] = useState<AvailableModel[] | null>(null);
  const [syncing, setSyncing] = useState(false);

  useEffect(() => {
    setSyncedModels(null);
  }, [form.kind]);

  const handleSync = async () => {
    setSyncing(true);
    try {
      const result = await syncModels({
        kind: form.kind,
        base_url: form.base_url || '',
        api_key: form.api_key || '',
      });
      const filtered = filterSyncedModels(result.models, taskWeight);
      if (result.models.length === 0) {
        message.warning('No models found. Check your credentials and try again.');
      } else if (filtered.length === 0) {
        message.info(`Found ${result.models.length} model(s), but none match the "${taskWeight}" task weight. Showing all.`);
        setSyncedModels(result.models);
        setSyncing(false);
        return;
      } else if (filtered.length < result.models.length) {
        message.success(`Showing ${filtered.length} of ${result.models.length} model(s) suitable for "${taskWeight}" tasks`);
      } else {
        message.success(`Found ${filtered.length} model(s)`);
      }
      setSyncedModels(filtered);
      onModelsLoaded?.(result.models);
    } catch {
      message.error('Failed to sync models.');
    } finally {
      setSyncing(false);
    }
  };

  const toOptions = (models: AvailableModel[]) =>
    models.map((m) => ({
      label: m.size
        ? `${m.name} (${formatBytes(m.size)})${requireVision && form.kind === 'Ollama' && isOllamaVisionModel(m.id) ? ' [vision]' : ''}`
        : m.name,
      value: m.id,
    }));

  const allStatic = staticModels[form.kind] || [];
  let filteredStatic = filterStaticModels(allStatic, taskWeight);
  if (requireVision) {
    filteredStatic = filteredStatic.filter((m) => m.vision);
  }

  // Filter synced Ollama models for vision if required
  const displaySynced = syncedModels && syncedModels.length > 0
    ? (requireVision && form.kind === 'Ollama'
        ? syncedModels.filter((m) => isOllamaVisionModel(m.id))
        : syncedModels)
    : null;

  const modelOptions =
    displaySynced && displaySynced.length > 0
      ? toOptions(displaySynced)
      : filteredStatic.map((m) => ({
          label: `${m.label}${m.vision ? '' : ''}`,
          value: m.value,
        }));
  const useModelSelect = modelOptions.length > 0;

  const needsBaseUrl = form.kind === 'Ollama' || form.kind === 'OpenAiCompatible';
  const needsApiKey = ['Claude', 'OpenAi', 'Gemini', 'OpenAiCompatible'].includes(form.kind);
  const gap = compact ? 8 : 12;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap }}>
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <div style={{ minWidth: 180, flex: compact ? '0 0 180px' : '0 0 220px' }}>
          <Text type="secondary" style={{ fontSize: 12 }}>Provider</Text>
          <Select
            size={compact ? 'small' : 'middle'}
            style={{ width: '100%', marginTop: 2 }}
            value={form.kind}
            onChange={(v) =>
              onChange({
                kind: v,
                model: '',
                base_url: v === 'Ollama' ? 'http://localhost:11435' : '',
                api_key: '',
              })
            }
            options={providerOptions}
          />
        </div>
        <div style={{ flex: 1, minWidth: 200 }}>
          <Text type="secondary" style={{ fontSize: 12 }}>Model</Text>
          <div style={{ display: 'flex', gap: 6, marginTop: 2 }}>
            {useModelSelect ? (
              <Select
                size={compact ? 'small' : 'middle'}
                showSearch
                optionFilterProp="label"
                options={modelOptions}
                value={form.model || undefined}
                onChange={(v) => onChange({ ...form, model: v })}
                placeholder="Select a model"
                style={{ flex: 1 }}
                allowClear={false}
              />
            ) : (
              <Input
                size={compact ? 'small' : 'middle'}
                value={form.model}
                onChange={(e) => onChange({ ...form, model: e.target.value })}
                placeholder={
                  form.kind === 'Ollama'
                    ? 'Sync to discover, or type e.g. llama3.2'
                    : form.kind === 'OpenAiCompatible'
                    ? 'e.g. deepseek-chat'
                    : 'Enter model name'
                }
                style={{ flex: 1 }}
              />
            )}
            <Tooltip title="Fetch available models from provider">
              <Button
                size={compact ? 'small' : 'middle'}
                icon={<SyncOutlined spin={syncing} />}
                onClick={handleSync}
                loading={syncing}
              />
            </Tooltip>
          </div>
        </div>
      </div>

      {(needsBaseUrl || needsApiKey) && (
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          {needsBaseUrl && (
            <div style={{ flex: 1, minWidth: 200 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>Base URL</Text>
              <Input
                size={compact ? 'small' : 'middle'}
                style={{ marginTop: 2 }}
                value={form.base_url}
                onChange={(e) => onChange({ ...form, base_url: e.target.value })}
                placeholder={
                  form.kind === 'Ollama'
                    ? 'http://localhost:11435'
                    : 'e.g. https://api.groq.com/openai'
                }
              />
            </div>
          )}
          {needsApiKey && (
            <div style={{ flex: 1, minWidth: 200 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>API Key</Text>
              <Input.Password
                size={compact ? 'small' : 'middle'}
                style={{ marginTop: 2 }}
                value={form.api_key}
                onChange={(e) => onChange({ ...form, api_key: e.target.value })}
                placeholder={existingKey ? '(unchanged)' : 'Enter API key'}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ── Agent definitions ────────────────────────────────────────────────

interface AgentLlmState {
  enabled: boolean;
  form: LlmFormState;
  max_tokens: number;
}

const defaultAgentMaxTokens: Record<string, number> = {
  analyzer: 1024,
  converter: 4096,
  quality: 1024,
  chunker: 2048,
  enricher: 2048,
  orchestrator: 512,
};

interface AgentInfo {
  label: string;
  shortLabel: string;
  description: string;
  taskWeight: 'Light' | 'Medium' | 'Heavy';
  /** When true, only vision-capable models should be offered for this agent. */
  requireVision?: boolean;
  recommendations: {
    ollama: string[];
    cloud: string[];
    tip: string;
  };
}

const agentDescriptions: Record<string, AgentInfo> = {
  analyzer: {
    label: 'Analyzer Agent',
    shortLabel: 'Analyzer',
    description: 'Analyzes document images to detect language, structure, OCR quality, and content type.',
    taskWeight: 'Light',
    requireVision: true,
    recommendations: {
      ollama: ['gemma3:4b', 'qwen2.5vl:latest', 'minicpm-v'],
      cloud: ['claude-haiku-4-20250414', 'gpt-4.1-nano', 'gemini-2.0-flash'],
      tip: 'Vision model enables direct image analysis for better OCR/structure detection.',
    },
  },
  converter: {
    label: 'Converter Agent',
    shortLabel: 'Converter',
    description: 'Converts raw text to clean Markdown. For OCR/scanned documents, uses vision to read directly from the image.',
    taskWeight: 'Heavy',
    requireVision: true,
    recommendations: {
      ollama: ['llava:13b', 'llama3.2-vision:11b', 'minicpm-v'],
      cloud: ['claude-sonnet-4-20250514', 'gpt-4o', 'gemini-2.5-flash'],
      tip: 'Vision model required for OCR documents. Reads directly from images for accurate Thai text extraction.',
    },
  },
  quality: {
    label: 'Quality Checker',
    shortLabel: 'Quality',
    description: 'Compares original document image against converted Markdown for accurate quality scoring.',
    taskWeight: 'Medium',
    requireVision: true,
    recommendations: {
      ollama: ['gemma3:4b', 'qwen2.5vl:latest', 'minicpm-v'],
      cloud: ['claude-haiku-4-20250414', 'gpt-4.1-nano', 'gemini-2.0-flash'],
      tip: 'Vision model compares original image vs converted text for more accurate quality scores.',
    },
  },
  chunker: {
    label: 'Smart Chunker',
    shortLabel: 'Chunker',
    description: 'Splits document into semantic sections with topic labels.',
    taskWeight: 'Medium',
    recommendations: {
      ollama: ['llama3.1:8b', 'gemma3:4b', 'qwen3:4b', 'mistral:7b'],
      cloud: ['claude-haiku-4-20250414', 'gpt-4.1-mini', 'gemini-2.5-flash'],
      tip: 'Needs semantic understanding. Mid-size model balances speed and quality.',
    },
  },
  enricher: {
    label: 'Chunk Enricher',
    shortLabel: 'Enricher',
    description: 'Adds search-optimized metadata to each chunk: context prefix, summary, keywords (Thai + English), and hypothetical queries (HyDE) for better retrieval.',
    taskWeight: 'Medium',
    recommendations: {
      ollama: ['llama3.1:8b', 'gemma3:4b', 'qwen3:4b', 'mistral:7b'],
      cloud: ['claude-haiku-4-20250414', 'gpt-4.1-mini', 'gemini-2.5-flash'],
      tip: 'Processes chunks in batches. Needs good bilingual (Thai/English) understanding for keyword extraction.',
    },
  },
  orchestrator: {
    label: 'Orchestrator Agent',
    shortLabel: 'Orchestrator',
    description: 'Reviews each agent\'s output and decides: accept, retry, adjust params, or flag for review.',
    taskWeight: 'Light',
    recommendations: {
      ollama: ['gemma3:4b', 'llama3.2:3b', 'phi4-mini', 'qwen3:4b'],
      cloud: ['claude-haiku-4-20250414', 'gpt-4.1-nano', 'gemini-2.0-flash'],
      tip: 'Makes JSON decisions only. Small/fast model is ideal — speed matters more than capability.',
    },
  },
};

function resolveCloudKind(model: string): string {
  if (model.startsWith('claude')) return 'Claude';
  if (model.startsWith('gpt') || model.startsWith('o3') || model.startsWith('o4')) return 'OpenAi';
  if (model.startsWith('gemini')) return 'Gemini';
  return 'OpenAi';
}

// ── Memory Estimate Component ────────────────────────────────────────

// Known Ollama model sizes (approximate, in bytes) for common models
const KNOWN_OLLAMA_SIZES: Record<string, number> = {
  'gemma3:4b': 3.3 * 1024 ** 3,
  'gemma3:12b': 8.1 * 1024 ** 3,
  'llama3.2:3b': 2.0 * 1024 ** 3,
  'llama3.1:8b': 4.7 * 1024 ** 3,
  'phi4-mini': 2.5 * 1024 ** 3,
  'qwen3:4b': 2.6 * 1024 ** 3,
  'qwen3:8b': 4.9 * 1024 ** 3,
  'mistral:7b': 4.1 * 1024 ** 3,
};

interface MemoryEstimateProps {
  llmMode: 'chat' | 'shared' | 'per-agent';
  sharedForm: LlmFormState;
  agentLlms: Record<string, AgentLlmState>;
  orchestratorEnabled: boolean;
  enricherEnabled: boolean;
  modelSizeCache: Record<string, number>;
}

function MemoryEstimate({ llmMode, sharedForm, agentLlms, orchestratorEnabled, enricherEnabled, modelSizeCache }: MemoryEstimateProps) {
  // Collect all Ollama models in use
  const ollamaModels: { agent: string; model: string; size: number | null }[] = [];

  const agentList = ['analyzer', 'converter', 'quality', 'chunker', ...(enricherEnabled ? ['enricher'] : []), ...(orchestratorEnabled ? ['orchestrator'] : [])];

  if (llmMode === 'shared' && sharedForm.kind === 'Ollama' && sharedForm.model) {
    const size = modelSizeCache[sharedForm.model] ?? KNOWN_OLLAMA_SIZES[sharedForm.model] ?? null;
    ollamaModels.push({ agent: 'All agents (shared)', model: sharedForm.model, size });
  } else if (llmMode === 'per-agent') {
    for (const agent of agentList) {
      const state = agentLlms[agent];
      if (state?.enabled && state.form.kind === 'Ollama' && state.form.model) {
        const size = modelSizeCache[state.form.model] ?? KNOWN_OLLAMA_SIZES[state.form.model] ?? null;
        ollamaModels.push({ agent: agentDescriptions[agent]?.shortLabel ?? agent, model: state.form.model, size });
      }
    }
  }

  if (ollamaModels.length === 0) return null;

  // Deduplicate: Ollama loads each model once regardless of how many agents use it
  const uniqueModels = new Map<string, { agents: string[]; size: number | null }>();
  for (const entry of ollamaModels) {
    const existing = uniqueModels.get(entry.model);
    if (existing) {
      existing.agents.push(entry.agent);
    } else {
      uniqueModels.set(entry.model, { agents: [entry.agent], size: entry.size });
    }
  }

  const knownTotal = Array.from(uniqueModels.values())
    .filter((v) => v.size !== null)
    .reduce((sum, v) => sum + v.size!, 0);
  const unknownCount = Array.from(uniqueModels.values()).filter((v) => v.size === null).length;

  return (
    <div style={{ marginTop: 12, padding: '10px 14px', background: 'rgba(22,119,255,0.04)', border: '1px solid rgba(22,119,255,0.15)', borderRadius: 6 }}>
      <Text strong style={{ fontSize: 13 }}>
        Estimated Local Memory (VRAM/RAM)
      </Text>
      <div style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 2 }}>
        {Array.from(uniqueModels.entries()).map(([model, info]) => (
          <div key={model} style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12 }}>
            <Text type="secondary">
              <code style={{ fontSize: 11 }}>{model}</code>
              {info.agents.length > 1 && (
                <span style={{ marginLeft: 4 }}>({info.agents.join(', ')} — loaded once)</span>
              )}
              {info.agents.length === 1 && (
                <span style={{ marginLeft: 4 }}>({info.agents[0]})</span>
              )}
            </Text>
            <Text style={{ fontSize: 12 }}>
              {info.size ? formatBytes(info.size) : <Text type="warning" style={{ fontSize: 11 }}>sync to get size</Text>}
            </Text>
          </div>
        ))}
      </div>
      <Divider style={{ margin: '6px 0' }} />
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13 }}>
        <Text strong>Total{uniqueModels.size > 1 ? ` (${uniqueModels.size} unique models)` : ''}</Text>
        <Text strong>
          {knownTotal > 0 ? formatBytes(knownTotal) : '—'}
          {unknownCount > 0 && <Text type="warning" style={{ fontSize: 11, marginLeft: 4 }}>+{unknownCount} unknown</Text>}
        </Text>
      </div>
      {uniqueModels.size < ollamaModels.length && (
        <Text type="secondary" style={{ fontSize: 11, display: 'block', marginTop: 4 }}>
          Ollama shares models across agents — duplicate models are loaded only once in memory.
        </Text>
      )}
    </div>
  );
}

// ── Main Component ───────────────────────────────────────────────────

const AGENTS = ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const;

export function DocumentProcessingTab() {
  const { token } = theme.useToken();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [config, setConfig] = useState<DocumentConfigResponse | null>(null);
  const [aiConfig, setAiConfig] = useState<AiPreprocessingConfig>({
    enabled: false,
    auto_params: true,
    quality_threshold: 0.7,
    max_llm_input_chars: 30000,
    agent_max_tokens: 4096,
    min_ai_size_bytes: 500,
    orchestrator_enabled: false,
    auto_orchestrator_budget: true,
    max_orchestrator_calls: 10,
    enricher_enabled: true,
    retry: {
      enabled: true,
      converter_max_retries: 2,
      chunker_max_retries: 1,
      analyzer_max_retries: 1,
      analyzer_retry_below_confidence: 0.5,
    },
  });

  // LLM mode: 'chat' = use main chat LLM, 'shared' = one custom model for all agents, 'per-agent' = different model per agent
  const [llmMode, setLlmMode] = useState<'chat' | 'shared' | 'per-agent'>('chat');
  const [llmForm, setLlmForm] = useState<LlmFormState>({ ...defaultLlmForm });

  const [agentLlms, setAgentLlms] = useState<Record<string, AgentLlmState>>({
    analyzer: { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens.analyzer },
    converter: { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens.converter },
    quality: { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens.quality },
    chunker: { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens.chunker },
    enricher: { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens.enricher },
    orchestrator: { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens.orchestrator },
  });

  // Pipeline settings (editable)
  const [maxChunkSize, setMaxChunkSize] = useState(512);
  const [chunkOverlap, setChunkOverlap] = useState(64);
  const [maxUploadSizeMb, setMaxUploadSizeMb] = useState(50);
  const [savingPipeline, setSavingPipeline] = useState(false);

  // Cache of model sizes from Ollama sync (model_id → size_bytes)
  const [modelSizeCache, setModelSizeCache] = useState<Record<string, number>>({});

  const handleModelsLoaded = useCallback((models: AvailableModel[]) => {
    setModelSizeCache((prev) => {
      const next = { ...prev };
      for (const m of models) {
        if (m.size) next[m.id] = m.size;
      }
      return next;
    });
  }, []);

  const updateAgentLlm = useCallback(
    (agent: string, update: Partial<AgentLlmState>) => {
      setAgentLlms((prev) => ({
        ...prev,
        [agent]: { ...prev[agent], ...update },
      }));
    },
    [],
  );

  useEffect(() => {
    loadConfig();
  }, []);

  async function loadConfig() {
    try {
      const data = await getDocumentConfig();
      setConfig(data);
      setAiConfig(data.ai_preprocessing);
      setMaxChunkSize(data.max_chunk_size);
      setChunkOverlap(data.chunk_overlap);
      setMaxUploadSizeMb(data.max_upload_size_mb);

      // Determine LLM mode from saved config
      const hasSharedLlm = !!data.ai_preprocessing.llm;
      const hasAnyAgentLlm = (['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const).some(
        (a) => !!(data.ai_preprocessing[`${a}_llm` as keyof AiPreprocessingConfig]),
      );

      if (hasAnyAgentLlm) {
        setLlmMode('per-agent');
      } else if (hasSharedLlm) {
        setLlmMode('shared');
      } else {
        setLlmMode('chat');
      }

      if (hasSharedLlm) {
        setLlmForm(llmInfoToForm(data.ai_preprocessing.llm!));
      }

      const agents = ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const;
      const newAgentLlms: Record<string, AgentLlmState> = {};
      for (const agent of agents) {
        const key = `${agent}_llm` as keyof AiPreprocessingConfig;
        const info = data.ai_preprocessing[key] as LlmProviderInfo | undefined;
        const mt = info?.max_tokens ?? defaultAgentMaxTokens[agent];
        newAgentLlms[agent] = info
          ? { enabled: true, form: llmInfoToForm(info), max_tokens: mt }
          : { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens[agent] };
      }
      setAgentLlms(newAgentLlms);
    } catch {
      message.error('Failed to load document config');
    } finally {
      setLoading(false);
    }
  }

  function validateLlmForm(form: LlmFormState, label: string, existingKey?: boolean): string | null {
    if (!form.model.trim()) return `Please select or enter a model for ${label}`;
    const needsKey = ['Claude', 'OpenAi', 'Gemini'].includes(form.kind);
    if (needsKey && !form.api_key && !existingKey) return `API key is required for ${label}`;
    if (form.kind === 'OpenAiCompatible' && !form.base_url.trim())
      return `Base URL is required for ${label}`;
    return null;
  }

  function buildLlmUpdate(form: LlmFormState): LlmConfigUpdate {
    return {
      kind: form.kind,
      model: form.model.trim(),
      base_url: form.base_url.trim() || undefined,
      api_key: form.api_key || undefined,
    };
  }

  async function handleSavePipeline() {
    setSavingPipeline(true);
    try {
      const resp = await updateDocumentConfig({
        max_chunk_size: maxChunkSize,
        chunk_overlap: chunkOverlap,
        max_upload_size_mb: maxUploadSizeMb,
      });
      setConfig(resp);
      message.success('Pipeline settings saved');
    } catch {
      message.error('Failed to save pipeline settings');
    } finally {
      setSavingPipeline(false);
    }
  }

  async function handleSave() {
    if (aiConfig.enabled && llmMode === 'shared') {
      const err = validateLlmForm(
        llmForm,
        'Preprocessing LLM',
        aiConfig.llm?.has_api_key && llmForm.kind === aiConfig.llm?.kind,
      );
      if (err) { message.error(err); return; }
    }

    if (aiConfig.enabled && llmMode === 'per-agent') {
      for (const [agent, state] of Object.entries(agentLlms)) {
        if (!state.enabled) continue;
        const info = agentDescriptions[agent];
        const key = `${agent}_llm` as keyof AiPreprocessingConfig;
        const existing = aiConfig[key] as LlmProviderInfo | undefined;
        const err = validateLlmForm(
          state.form,
          info.label,
          existing?.has_api_key && state.form.kind === existing?.kind,
        );
        if (err) { message.error(err); return; }
      }
    }

    setSaving(true);
    try {
      const req: UpdateDocumentConfigRequest = {
        ai_preprocessing: {
          enabled: aiConfig.enabled,
          auto_params: aiConfig.auto_params,
          quality_threshold: aiConfig.quality_threshold,
          max_llm_input_chars: aiConfig.max_llm_input_chars,
          agent_max_tokens: aiConfig.agent_max_tokens,
          min_ai_size_bytes: aiConfig.min_ai_size_bytes,
          retry_enabled: aiConfig.retry.enabled,
          converter_max_retries: aiConfig.retry.converter_max_retries,
          chunker_max_retries: aiConfig.retry.chunker_max_retries,
          analyzer_max_retries: aiConfig.retry.analyzer_max_retries,
          analyzer_retry_below_confidence: aiConfig.retry.analyzer_retry_below_confidence,
          orchestrator_enabled: aiConfig.orchestrator_enabled,
          auto_orchestrator_budget: aiConfig.auto_orchestrator_budget,
          max_orchestrator_calls: aiConfig.max_orchestrator_calls,
          enricher_enabled: aiConfig.enricher_enabled,
        },
      };

      const ai = req.ai_preprocessing!;

      if (llmMode === 'shared') {
        ai.llm = buildLlmUpdate(llmForm);
        // Remove per-agent overrides
        for (const agent of ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const) {
          (ai as Record<string, unknown>)[`remove_${agent}_llm`] = true;
        }
      } else if (llmMode === 'per-agent') {
        ai.remove_llm = true;
        for (const agent of ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const) {
          const state = agentLlms[agent];
          if (state.enabled) {
            (ai as Record<string, unknown>)[`${agent}_llm`] = {
              ...buildLlmUpdate(state.form),
              max_tokens: state.max_tokens,
            };
          } else {
            (ai as Record<string, unknown>)[`remove_${agent}_llm`] = true;
          }
        }
      } else {
        // 'chat' mode — remove all custom LLMs
        ai.remove_llm = true;
        for (const agent of ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const) {
          (ai as Record<string, unknown>)[`remove_${agent}_llm`] = true;
        }
      }

      const updated = await updateDocumentConfig(req);
      setConfig(updated);
      setAiConfig(updated.ai_preprocessing);

      // Restore LLM mode from response
      const hasSharedLlm = !!updated.ai_preprocessing.llm;
      const hasAnyAgentLlm = (['analyzer', 'converter', 'quality', 'chunker', 'enricher'] as const).some(
        (a) => !!(updated.ai_preprocessing[`${a}_llm` as keyof typeof updated.ai_preprocessing]),
      );
      if (hasAnyAgentLlm) {
        setLlmMode('per-agent');
      } else if (hasSharedLlm) {
        setLlmMode('shared');
      } else {
        setLlmMode('chat');
      }

      if (hasSharedLlm) {
        setLlmForm(llmInfoToForm(updated.ai_preprocessing.llm!));
      }

      const agents = ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const;
      const newAgentLlms: Record<string, AgentLlmState> = {};
      for (const agent of agents) {
        const key = `${agent}_llm` as keyof AiPreprocessingConfig;
        const info = updated.ai_preprocessing[key] as LlmProviderInfo | undefined;
        const mt = info?.max_tokens ?? defaultAgentMaxTokens[agent];
        newAgentLlms[agent] = info
          ? { enabled: true, form: llmInfoToForm(info), max_tokens: mt }
          : { enabled: false, form: { ...defaultLlmForm }, max_tokens: defaultAgentMaxTokens[agent] };
      }
      setAgentLlms(newAgentLlms);

      message.success('Document processing settings saved');
    } catch {
      message.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return <Spin tip="Loading document config..." />;
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {/* Pipeline Settings */}
      <Collapse
        defaultActiveKey={['pipeline-settings']}
        items={[{
          key: 'pipeline-settings',
          label: <><SettingOutlined /> Pipeline Settings</>,
          children: (
      <Card
        size="small"
        title={<Text type="secondary"><SettingOutlined /> Pipeline Settings</Text>}
        extra={
          <Button
            type="primary"
            size="small"
            icon={<SaveOutlined />}
            loading={savingPipeline}
            onClick={handleSavePipeline}
          >
            Save
          </Button>
        }
      >
        <Space size="large" wrap>
          <Space direction="vertical" size={2}>
            <Text type="secondary">Max Chunk Size (chars)</Text>
            <InputNumber
              min={64}
              max={100000}
              step={64}
              value={maxChunkSize}
              onChange={(v) => v && setMaxChunkSize(v)}
              style={{ width: 140 }}
            />
          </Space>
          <Space direction="vertical" size={2}>
            <Text type="secondary">Chunk Overlap (chars)</Text>
            <InputNumber
              min={0}
              max={10000}
              step={16}
              value={chunkOverlap}
              onChange={(v) => v != null && setChunkOverlap(v)}
              style={{ width: 140 }}
            />
          </Space>
          <Space direction="vertical" size={2}>
            <Text type="secondary">Max Upload Size (MB)</Text>
            <InputNumber
              min={1}
              max={1024}
              value={maxUploadSizeMb}
              onChange={(v) => v && setMaxUploadSizeMb(v)}
              style={{ width: 140 }}
            />
          </Space>
        </Space>
        <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0, fontSize: 12 }}>
          Note: Max upload size change takes effect after server restart.
        </Paragraph>
      </Card>
          ),
        }]}
      />

      {/* AI Preprocessing */}
      <Collapse
        defaultActiveKey={['ai-preprocessing']}
        items={[{
          key: 'ai-preprocessing',
          label: <><RobotOutlined /> AI Document Preprocessing</>,
          children: (
      <Card
        title={
          <Space>
            <RobotOutlined />
            <span>AI Document Preprocessing</span>
            <Switch
              size="small"
              checked={aiConfig.enabled}
              onChange={(checked) => setAiConfig({ ...aiConfig, enabled: checked })}
            />
          </Space>
        }
        extra={
          <Button
            type="primary"
            size="small"
            icon={<SaveOutlined />}
            onClick={handleSave}
            loading={saving}
          >
            Save
          </Button>
        }
      >
        {!aiConfig.enabled ? (
          <div>
            <Text type="secondary">
              When enabled, uploaded documents go through an intelligent 4-step AI pipeline:
            </Text>
            <div style={{ margin: '8px 0', fontSize: 12, color: token.colorTextSecondary, lineHeight: 2 }}>
              <Tag color="blue">1. Analyzer</Tag> Examines the document — detects language (Thai/English), content type, and whether it needs OCR correction<br />
              <Tag color="green">2. Converter</Tag> Converts raw text (or reads directly from images for scanned docs) into clean, structured Markdown<br />
              <Tag color="orange">3. Quality Checker</Tag> Compares the original against the conversion to score accuracy and completeness<br />
              <Tag color="purple">4. Smart Chunker</Tag> Splits the document into meaningful semantic sections for search
            </div>
            <Text type="secondary">
              Without AI, documents are split mechanically by character count — faster but lower quality for search.
            </Text>
          </div>
        ) : (
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Alert
              type="info"
              showIcon
              message="AI preprocessing uses LLM API calls per document. ~$0.08-0.15 per 10-page doc with cloud APIs; free with local Ollama."
              style={{ marginBottom: 0 }}
            />

            {/* Processing Parameters */}
            <div>
              <Space style={{ marginBottom: 8 }}>
                <SettingOutlined />
                <Text strong>Processing Parameters</Text>
                <Switch
                  data-testid="auto-params-switch"
                  size="small"
                  checked={aiConfig.auto_params}
                  onChange={(checked) => setAiConfig({ ...aiConfig, auto_params: checked })}
                />
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {aiConfig.auto_params ? 'Auto — AI adjusts per document' : 'Manual — fixed values'}
                </Text>
              </Space>

              {aiConfig.auto_params ? (
                <div style={{
                  padding: '8px 12px',
                  background: token.colorFillQuaternary,
                  borderRadius: 6,
                  fontSize: 12,
                  color: token.colorTextSecondary,
                }}>
                  The Analyzer agent examines each document and automatically sets the best quality threshold,
                  chunk size, and minimum AI size based on content type and complexity.
                  No manual tuning needed.
                </div>
              ) : (
                <div style={{
                  padding: '8px 12px',
                  background: token.colorFillQuaternary,
                  borderRadius: 6,
                  fontSize: 12,
                  color: token.colorTextSecondary,
                }}>
                  Fixed values applied to all documents. The AI will not adjust these per document.
                </div>
              )}
            </div>

            <div>
              {/* Quality Threshold — only in manual mode */}
              {!aiConfig.auto_params && (
                <div style={{ marginBottom: 8 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                    <Text style={{ fontSize: 13 }}>
                      Quality Threshold: <Text strong>{aiConfig.quality_threshold.toFixed(2)}</Text>
                    </Text>
                    <Tooltip title="After converting a document, the Quality Checker scores it 0-1. If the score is below this threshold, the conversion is rejected and retried. Lower = more lenient (good for OCR/scanned docs). Higher = stricter (good for clean text).">
                      <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
                    </Tooltip>
                  </div>
                  <Slider
                    min={0.3}
                    max={1.0}
                    step={0.05}
                    value={aiConfig.quality_threshold}
                    onChange={(value) => setAiConfig({ ...aiConfig, quality_threshold: value })}
                    marks={{ 0.3: 'Lenient', 0.7: 'Default', 1.0: 'Strict' }}
                    style={{ margin: '4px 0 0' }}
                  />
                </div>
              )}

              <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap' }}>
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                    <Text type="secondary" style={{ fontSize: 12 }}>Max LLM Input (chars)</Text>
                    <Tooltip title="How much text the AI reads at once. Large documents are split into segments of this size. The AI processes each segment separately, then joins them together. Bigger = better context but slower and more expensive.">
                      <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </div>
                  <InputNumber
                    size="small"
                    min={5000}
                    max={100000}
                    step={5000}
                    value={aiConfig.max_llm_input_chars}
                    onChange={(v) => v && setAiConfig({ ...aiConfig, max_llm_input_chars: v })}
                    style={{ width: 120 }}
                  />
                </div>
                {llmMode !== 'per-agent' && (
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                    <Text type="secondary" style={{ fontSize: 12 }}>Agent Max Tokens</Text>
                    <Tooltip title="How much the AI is allowed to write back (output limit). Each agent call is capped at this many tokens. Too small = response gets cut off. Too large = wastes memory. In per-agent mode, this is configured individually per agent.">
                      <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </div>
                  <InputNumber
                    size="small"
                    min={256}
                    max={16384}
                    step={256}
                    value={aiConfig.agent_max_tokens}
                    onChange={(v) => v && setAiConfig({ ...aiConfig, agent_max_tokens: v })}
                    style={{ width: 120 }}
                  />
                </div>
                )}
                {!aiConfig.auto_params && (
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                    <Text type="secondary" style={{ fontSize: 12 }}>Min AI Size (bytes)</Text>
                    <Tooltip title="Documents smaller than this skip AI processing and use simple mechanical splitting instead. Very small documents don't benefit from AI. Set to 0 to always use AI.">
                      <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </div>
                  <InputNumber
                    size="small"
                    min={0}
                    max={10000}
                    step={100}
                    value={aiConfig.min_ai_size_bytes}
                    onChange={(v) => v != null && setAiConfig({ ...aiConfig, min_ai_size_bytes: v })}
                    style={{ width: 120 }}
                  />
                </div>
                )}
              </div>
            </div>

            <Divider style={{ margin: '8px 0' }} />

            {/* Agent LLM — unified section */}
            <div>
              <Space style={{ marginBottom: 8 }}>
                <ThunderboltOutlined />
                <Text strong>Agent LLM</Text>
              </Space>
              <div style={{ marginBottom: 12 }}>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 8 }}>
                  Which model should the AI agents use for document processing?
                </Text>
                <Segmented
                  value={llmMode}
                  onChange={(v) => setLlmMode(v as 'chat' | 'shared' | 'per-agent')}
                  options={[
                    { label: 'Use Chat LLM', value: 'chat' },
                    { label: 'Same model for all', value: 'shared' },
                    { label: 'Different per agent', value: 'per-agent' },
                  ]}
                  style={{ marginBottom: 8 }}
                />
                <div style={{
                  padding: '6px 12px',
                  background: token.colorFillQuaternary,
                  borderRadius: 6,
                  fontSize: 12,
                  color: token.colorTextSecondary,
                }}>
                  {llmMode === 'chat' && 'All agents share your main Chat LLM. Simplest setup — no extra config needed.'}
                  {llmMode === 'shared' && 'All agents use a dedicated model below, keeping document processing separate from chat.'}
                  {llmMode === 'per-agent' && 'Assign a different model to each agent to optimize cost and memory usage.'}
                </div>
              </div>

              {/* Shared LLM config */}
              {llmMode === 'shared' && (
                <div style={{ padding: '12px 16px', background: token.colorFillQuaternary, borderRadius: 6 }}>
                  <LlmConfigForm
                    form={llmForm}
                    onChange={setLlmForm}
                    existingKey={aiConfig.llm?.has_api_key && llmForm.kind === aiConfig.llm?.kind}
                    onModelsLoaded={handleModelsLoaded}
                  />
                </div>
              )}

              {/* Per-agent LLM config */}
              {llmMode === 'per-agent' && (
                <Collapse
                  size="small"
                  items={(['analyzer', 'converter', 'quality', 'chunker', ...(aiConfig.enricher_enabled ? ['enricher'] : []), ...(aiConfig.orchestrator_enabled ? ['orchestrator'] : [])] as string[]).map((agent) => {
                    const desc = agentDescriptions[agent];
                    const state = agentLlms[agent];
                    const key = `${agent}_llm` as keyof AiPreprocessingConfig;
                    const existingInfo = aiConfig[key] as LlmProviderInfo | undefined;

                    return {
                      key: agent,
                      label: (
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, width: '100%' }}>
                          <Text strong style={{ minWidth: 90 }}>{desc.shortLabel}</Text>
                          <Tag
                            bordered={false}
                            color={
                              desc.taskWeight === 'Light' ? 'green'
                              : desc.taskWeight === 'Heavy' ? 'volcano'
                              : 'default'
                            }
                            style={{ fontSize: 11, lineHeight: '18px', padding: '0 6px' }}
                          >
                            {desc.taskWeight}
                          </Tag>
                          {desc.requireVision && (
                            <Tag bordered={false} color="blue" style={{ fontSize: 11, lineHeight: '18px', padding: '0 6px' }}>
                              Vision
                            </Tag>
                          )}
                          <Text type="secondary" style={{ fontSize: 12, flex: 1 }}>
                            {state.enabled
                              ? `${state.form.kind} / ${state.form.model || '...'}`
                              : 'Uses Chat LLM'}
                          </Text>
                        </div>
                      ),
                      children: (
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                          <Text type="secondary" style={{ fontSize: 12 }}>{desc.description}</Text>

                          {/* Recommendations */}
                          <div style={{ fontSize: 12, color: token.colorTextSecondary, lineHeight: 1.8 }}>
                            <Text type="secondary" style={{ fontSize: 12 }}>{desc.recommendations.tip}</Text>
                            <br />
                            <span>Suggested: </span>
                            {desc.recommendations.ollama.map((m) => (
                              <Tag
                                key={m}
                                bordered={false}
                                style={{
                                  fontSize: 11,
                                  cursor: 'pointer',
                                  padding: '0 5px',
                                  background: token.colorFillSecondary,
                                }}
                                onClick={() => {
                                  updateAgentLlm(agent, {
                                    enabled: true,
                                    form: { kind: 'Ollama', model: m, base_url: 'http://localhost:11435', api_key: '' },
                                  });
                                }}
                              >
                                {m}
                              </Tag>
                            ))}
                            {desc.recommendations.cloud.map((m) => (
                              <Tag
                                key={m}
                                bordered={false}
                                style={{
                                  fontSize: 11,
                                  cursor: 'pointer',
                                  padding: '0 5px',
                                  background: token.colorFillTertiary,
                                }}
                                onClick={() => {
                                  updateAgentLlm(agent, {
                                    enabled: true,
                                    form: { kind: resolveCloudKind(m), model: m, base_url: '', api_key: '' },
                                  });
                                }}
                              >
                                {m}
                              </Tag>
                            ))}
                            <Text type="secondary" style={{ fontSize: 11 }}> (click to select)</Text>
                          </div>

                          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                            <Switch
                              size="small"
                              checked={state.enabled}
                              onChange={(checked) => updateAgentLlm(agent, { enabled: checked })}
                            />
                            <Text style={{ fontSize: 13 }}>
                              {state.enabled ? 'Custom model' : 'Uses Chat LLM'}
                            </Text>
                          </div>

                          {state.enabled && (
                            <div style={{ padding: '8px 12px', background: token.colorFillQuaternary, borderRadius: 6 }}>
                              <LlmConfigForm
                                form={state.form}
                                onChange={(form) => updateAgentLlm(agent, { form })}
                                existingKey={existingInfo?.has_api_key && state.form.kind === existingInfo?.kind}
                                compact
                                taskWeight={desc.taskWeight}
                                requireVision={desc.requireVision}
                                onModelsLoaded={handleModelsLoaded}
                              />
                            </div>
                          )}

                          {/* Per-agent Max Tokens */}
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                            <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                              <Text type="secondary" style={{ fontSize: 12 }}>Max Tokens:</Text>
                              <Tooltip title={
                                agent === 'converter'
                                  ? 'How much the Converter can write back. Needs to be large since it outputs the full converted Markdown.'
                                  : agent === 'chunker'
                                  ? 'How much the Chunker can write back. It outputs a JSON array of sections, so needs moderate space.'
                                  : `How much the ${desc.shortLabel} can write back. It returns short JSON, so a small value is fine.`
                              }>
                                <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                              </Tooltip>
                            </div>
                            <InputNumber
                              size="small"
                              min={256}
                              max={16384}
                              step={256}
                              value={state.max_tokens}
                              onChange={(v) => v && updateAgentLlm(agent, { max_tokens: v })}
                              style={{ width: 100 }}
                            />
                            <Text type="secondary" style={{ fontSize: 11 }}>
                              (default: {defaultAgentMaxTokens[agent]})
                            </Text>
                          </div>
                        </div>
                      ),
                    };
                  })}
                />
              )}

              {/* Memory Estimate */}
              <MemoryEstimate
                llmMode={llmMode}
                sharedForm={llmForm}
                agentLlms={agentLlms}
                orchestratorEnabled={aiConfig.orchestrator_enabled}
                enricherEnabled={aiConfig.enricher_enabled}
                modelSizeCache={modelSizeCache}
              />
            </div>

            <Divider style={{ margin: '8px 0' }} />

            {/* Retry-with-Feedback Settings */}
            <div>
              <Space style={{ marginBottom: 8 }}>
                <ReloadOutlined />
                <Text strong>Retry-with-Feedback</Text>
                <Switch
                  size="small"
                  checked={aiConfig.retry.enabled}
                  onChange={(checked) =>
                    setAiConfig({ ...aiConfig, retry: { ...aiConfig.retry, enabled: checked } })
                  }
                />
              </Space>
              <div style={{
                padding: '6px 12px',
                background: token.colorFillQuaternary,
                borderRadius: 6,
                fontSize: 12,
                color: token.colorTextSecondary,
                marginBottom: 12,
              }}>
                {aiConfig.retry.enabled
                  ? 'When an agent\'s output doesn\'t meet quality standards, it gets a second chance with specific feedback about what to fix — like a teacher marking corrections on an essay.'
                  : 'Disabled — if an agent fails, the system falls back to simple mechanical processing (no second chances).'}
              </div>

              {aiConfig.retry.enabled && !aiConfig.orchestrator_enabled && (
                <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap' }}>
                  <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <Text type="secondary" style={{ fontSize: 12 }}>Converter Retries</Text>
                      <Tooltip title="How many times the Converter can retry if the Quality Checker finds issues (e.g., missing content, broken formatting). Each retry includes feedback about what to fix.">
                        <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                      </Tooltip>
                    </div>
                    <InputNumber
                      size="small"
                      min={0}
                      max={5}
                      value={aiConfig.retry.converter_max_retries}
                      onChange={(v) =>
                        v != null && setAiConfig({ ...aiConfig, retry: { ...aiConfig.retry, converter_max_retries: v } })
                      }
                      style={{ width: 80 }}
                    />
                  </div>
                  <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <Text type="secondary" style={{ fontSize: 12 }}>Chunker Retries</Text>
                      <Tooltip title="How many times the Smart Chunker can retry if it produces invalid sections (e.g., overlapping ranges, gaps in coverage, chunks that are too large).">
                        <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                      </Tooltip>
                    </div>
                    <InputNumber
                      size="small"
                      min={0}
                      max={5}
                      value={aiConfig.retry.chunker_max_retries}
                      onChange={(v) =>
                        v != null && setAiConfig({ ...aiConfig, retry: { ...aiConfig.retry, chunker_max_retries: v } })
                      }
                      style={{ width: 80 }}
                    />
                  </div>
                  <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <Text type="secondary" style={{ fontSize: 12 }}>Analyzer Retries</Text>
                      <Tooltip title="How many times the Analyzer can retry with a larger text excerpt if it's not confident about its analysis (e.g., can't determine the language or document type).">
                        <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                      </Tooltip>
                    </div>
                    <InputNumber
                      size="small"
                      min={0}
                      max={5}
                      value={aiConfig.retry.analyzer_max_retries}
                      onChange={(v) =>
                        v != null && setAiConfig({ ...aiConfig, retry: { ...aiConfig.retry, analyzer_max_retries: v } })
                      }
                      style={{ width: 80 }}
                    />
                  </div>
                  <div style={{ minWidth: 200 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        Analyzer Retry Confidence: <Text strong>{aiConfig.retry.analyzer_retry_below_confidence.toFixed(2)}</Text>
                      </Text>
                      <Tooltip title="If the Analyzer's confidence score is below this value, it retries with a larger excerpt. For example, at 0.5: if the Analyzer is less than 50% sure, it reads more text and tries again.">
                        <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                      </Tooltip>
                    </div>
                    <Slider
                      min={0.1}
                      max={0.9}
                      step={0.05}
                      value={aiConfig.retry.analyzer_retry_below_confidence}
                      onChange={(v) =>
                        setAiConfig({ ...aiConfig, retry: { ...aiConfig.retry, analyzer_retry_below_confidence: v } })
                      }
                      marks={{ 0.1: '0.1', 0.5: '0.5', 0.9: '0.9' }}
                      style={{ margin: '4px 0 0' }}
                    />
                  </div>
                </div>
              )}
            </div>

            <Divider style={{ margin: '8px 0' }} />

            {/* Chunk Enrichment */}
            <div>
              <Space style={{ marginBottom: 8 }}>
                <ThunderboltOutlined />
                <Text strong>Chunk Enrichment</Text>
                <Switch
                  data-testid="enricher-switch"
                  size="small"
                  checked={aiConfig.enricher_enabled}
                  onChange={(checked) =>
                    setAiConfig({ ...aiConfig, enricher_enabled: checked })
                  }
                />
                <Tooltip title="After chunking, an AI agent enhances each chunk with: a context prefix (e.g., 'From: Tax Policy 2025, Section 3'), a one-sentence summary, bilingual keywords (Thai + English), and hypothetical questions the chunk answers. This dramatically improves search accuracy because the embedding captures richer context.">
                  <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
                </Tooltip>
              </Space>
              <div style={{
                padding: '6px 12px',
                background: token.colorFillQuaternary,
                borderRadius: 6,
                fontSize: 12,
                color: token.colorTextSecondary,
                marginBottom: 12,
              }}>
                {aiConfig.enricher_enabled
                  ? 'Each chunk gets enriched with search metadata before embedding. Think of it as adding a table of contents and index entries to every chunk so search can find it more easily — even when the user\'s query uses different words than the document.'
                  : 'Disabled — chunks are embedded as-is. Enable this for significantly better search retrieval, especially for Thai documents.'}
              </div>
            </div>

            <Divider style={{ margin: '8px 0' }} />

            {/* LLM-Driven Orchestration */}
            <div>
              <Space style={{ marginBottom: 8 }}>
                <ClusterOutlined />
                <Text strong>Smart Orchestration</Text>
                <Switch
                  data-testid="orchestrator-switch"
                  size="small"
                  checked={aiConfig.orchestrator_enabled}
                  onChange={(checked) =>
                    setAiConfig({ ...aiConfig, orchestrator_enabled: checked })
                  }
                />
                <Tooltip title="Instead of fixed retry counts, an AI 'supervisor' reviews each step's output and makes smart decisions: accept if good enough, retry with specific feedback, adjust quality thresholds, or fall back to simple processing.">
                  <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
                </Tooltip>
              </Space>
              <div style={{
                padding: '6px 12px',
                background: token.colorFillQuaternary,
                borderRadius: 6,
                fontSize: 12,
                color: token.colorTextSecondary,
                marginBottom: 12,
              }}>
                {aiConfig.orchestrator_enabled
                  ? 'A separate AI "supervisor" reviews each agent\'s work and decides the next step — like a manager who checks quality and gives feedback. This replaces the fixed retry counts above with intelligent, adaptive decision-making.'
                  : 'Disabled — uses the fixed retry counts from Retry-with-Feedback above. Enable this for smarter, adaptive processing.'}
              </div>

              {aiConfig.orchestrator_enabled && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <Text type="secondary" style={{ fontSize: 12 }}>Budget Mode:</Text>
                    <Tooltip title="The budget controls how many times the supervisor can review and give feedback per document. Auto mode gives complex documents (OCR, tables) more reviews and simple documents fewer. Fixed mode uses the same limit for all documents.">
                      <QuestionCircleOutlined style={{ fontSize: 11, color: token.colorTextQuaternary }} />
                    </Tooltip>
                    <Segmented
                      size="small"
                      options={[
                        { label: 'Auto (adaptive)', value: 'auto' },
                        { label: 'Fixed', value: 'fixed' },
                      ]}
                      value={aiConfig.auto_orchestrator_budget ? 'auto' : 'fixed'}
                      onChange={(v) =>
                        setAiConfig({ ...aiConfig, auto_orchestrator_budget: v === 'auto' })
                      }
                    />
                  </div>

                  {aiConfig.auto_orchestrator_budget ? (
                    <div style={{
                      padding: '8px 12px',
                      background: token.colorFillQuaternary,
                      borderRadius: 6,
                      fontSize: 12,
                      lineHeight: 1.7,
                    }}>
                      <Text type="secondary">
                        Budget is computed per document from complexity — no manual tuning needed.
                      </Text>
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '2px 16px', marginTop: 4 }}>
                        <Text style={{ fontSize: 11 }}>Base: <Text strong>3</Text></Text>
                        <Text style={{ fontSize: 11 }}>OCR: <Text strong>+2</Text></Text>
                        <Text style={{ fontSize: 11 }}>Unstructured: <Text strong>+1</Text></Text>
                        <Text style={{ fontSize: 11 }}>Mixed/Tabular/Form: <Text strong>+1</Text></Text>
                        <Text style={{ fontSize: 11 }}>Multipage: <Text strong>+1</Text></Text>
                        <Text style={{ fontSize: 11 }}>&gt;20 sections: <Text strong>+1</Text></Text>
                        <Text style={{ fontSize: 11 }}>Well-structured: <Text strong>-1</Text></Text>
                      </div>
                      <Text type="secondary" style={{ fontSize: 11, display: 'block', marginTop: 4 }}>
                        Scanned OCR PDF with tables → <Text strong>7</Text> calls.
                        Clean text file → <Text strong>2</Text> calls.
                        Simple docs use fewer calls; complex docs get more — automatically.
                      </Text>
                    </div>
                  ) : (
                    <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
                      <div>
                        <Text type="secondary" style={{ fontSize: 12 }}>Calls per Document</Text>
                        <br />
                        <InputNumber
                          size="small"
                          min={2}
                          max={15}
                          value={aiConfig.max_orchestrator_calls}
                          onChange={(v) =>
                            v != null && setAiConfig({ ...aiConfig, max_orchestrator_calls: v })
                          }
                          style={{ width: 80 }}
                        />
                      </div>
                      <Text type="secondary" style={{ fontSize: 11, flex: 1 }}>
                        Every document gets up to this many orchestrator calls regardless of complexity.
                        Useful for controlling costs with cloud LLMs. Running locally with Ollama has no API cost.
                      </Text>
                    </div>
                  )}
                </div>
              )}
            </div>
          </Space>
        )}
      </Card>
          ),
        }]}
      />

      {/* Pipeline Explanation — compact */}
      <Card size="small">
        <Paragraph style={{ marginBottom: 4 }}>
          <Text strong>Without AI:</Text> Mechanical convert → fixed-size chunk by paragraph.
        </Paragraph>
        <Paragraph style={{ marginBottom: 0 }}>
          <Text strong>With AI:</Text>{' '}
          <Text type="secondary">
            Analyzer (language/structure) → Converter (clean markdown) → Quality Check → Smart Chunker (semantic sections)
            {aiConfig.enricher_enabled ? ' → Chunk Enricher (keywords, summaries, HyDE)' : ''}.
            {aiConfig.orchestrator_enabled
              ? ' Orchestrator reviews each step and adaptively decides retry/accept/fallback.'
              : ' Auto-fallback to mechanical on failure.'}
          </Text>
        </Paragraph>
      </Card>

      {/* Embedding & Vector Store — final pipeline steps */}
      <Collapse
        defaultActiveKey={['embedding-vector']}
        items={[{
          key: 'embedding-vector',
          label: 'Embedding & Vector Store',
          children: <EmbeddingVectorSection />,
        }]}
      />
    </Space>
  );
}

// ── Embedding & Vector Store Section ────────────────────────────────

const embeddingProviderOptions = [
  { label: 'Fastembed (Local)', value: 'Fastembed' },
  { label: 'OpenAI / Compatible', value: 'OpenAi' },
  { label: 'Ollama (Local)', value: 'Ollama' },
  { label: 'Cohere', value: 'Cohere' },
];

const staticEmbModels: Record<string, { label: string; value: string }[]> = {
  Fastembed: [
    { label: 'BGE Small EN v1.5 (dim=384)', value: 'BAAI/bge-small-en-v1.5' },
    { label: 'BGE Base EN v1.5 (dim=768)', value: 'BAAI/bge-base-en-v1.5' },
    { label: 'BGE Large EN v1.5 (dim=1024)', value: 'BAAI/bge-large-en-v1.5' },
    { label: 'All-MiniLM-L6-v2 (dim=384)', value: 'sentence-transformers/all-MiniLM-L6-v2' },
    { label: 'Multilingual MiniLM L12 v2 (dim=384)', value: 'sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2' },
    { label: 'Jina Embeddings v2 Small EN (dim=512)', value: 'jinaai/jina-embeddings-v2-small-en' },
    { label: 'Jina Embeddings v2 Base EN (dim=768)', value: 'jinaai/jina-embeddings-v2-base-en' },
  ],
  OpenAi: [
    { label: 'text-embedding-3-small (dim=1536)', value: 'text-embedding-3-small' },
    { label: 'text-embedding-3-large (dim=3072)', value: 'text-embedding-3-large' },
    { label: 'text-embedding-ada-002 (dim=1536)', value: 'text-embedding-ada-002' },
  ],
  Cohere: [
    { label: 'Embed v4.0 (dim=1024)', value: 'embed-v4.0' },
    { label: 'Embed English v3.0 (dim=1024)', value: 'embed-english-v3.0' },
    { label: 'Embed Multilingual v3.0 (dim=1024)', value: 'embed-multilingual-v3.0' },
    { label: 'Embed English Light v3.0 (dim=384)', value: 'embed-english-light-v3.0' },
    { label: 'Embed Multilingual Light v3.0 (dim=384)', value: 'embed-multilingual-light-v3.0' },
  ],
};

/** Known model → default dimension mapping. Used to auto-fill the Dimension field. */
const knownEmbDimensions: Record<string, number> = {
  // Fastembed
  'BAAI/bge-small-en-v1.5': 384,
  'BAAI/bge-base-en-v1.5': 768,
  'BAAI/bge-large-en-v1.5': 1024,
  'sentence-transformers/all-MiniLM-L6-v2': 384,
  'sentence-transformers/all-MiniLM-L12-v2': 384,
  'sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2': 384,
  'jinaai/jina-embeddings-v2-small-en': 512,
  'jinaai/jina-embeddings-v2-base-en': 768,
  // OpenAI
  'text-embedding-3-small': 1536,
  'text-embedding-3-large': 3072,
  'text-embedding-ada-002': 1536,
  // Cohere
  'embed-v4.0': 1024,
  'embed-english-v3.0': 1024,
  'embed-multilingual-v3.0': 1024,
  'embed-english-light-v3.0': 384,
  'embed-multilingual-light-v3.0': 384,
  // Ollama — popular embedding models
  'nomic-embed-text:latest': 768,
  'nomic-embed-text-v2-moe:latest': 768,
  'mxbai-embed-large:latest': 1024,
  'all-minilm:latest': 384,
  'bge-m3:latest': 1024,
  'snowflake-arctic-embed:latest': 1024,
  'embeddinggemma:300m': 1536,
  'qwen3-embedding:8b': 2048,
  'qwen3-embedding:0.6b': 1024,
};

/** Try to find the dimension for a model, matching with or without ":latest" tag. */
function lookupEmbDimension(model: string): number | undefined {
  if (knownEmbDimensions[model]) return knownEmbDimensions[model];
  // Try with :latest suffix
  if (knownEmbDimensions[model + ':latest']) return knownEmbDimensions[model + ':latest'];
  // Try without tag
  const base = model.replace(/:.*$/, '');
  if (knownEmbDimensions[base + ':latest']) return knownEmbDimensions[base + ':latest'];
  return undefined;
}

const vectorStoreOptions = [
  { label: 'In-Memory (dev only)', value: 'InMemory' },
  { label: 'Qdrant', value: 'Qdrant' },
  { label: 'pgvector (PostgreSQL)', value: 'Pgvector' },
  { label: 'ChromaDB', value: 'ChromaDb' },
  { label: 'Pinecone', value: 'Pinecone' },
  { label: 'Weaviate', value: 'Weaviate' },
  { label: 'Milvus', value: 'Milvus' },
];

const vsUrlPlaceholders: Record<string, string> = {
  Qdrant: 'http://localhost:6334',
  Pgvector: 'postgresql://user:pass@localhost:5432/db',
  ChromaDb: 'http://localhost:8000',
  Milvus: 'http://localhost:19530',
  Weaviate: 'http://localhost:8080',
  Pinecone: 'https://index-xxx.svc.pinecone.io',
};

const kindColors: Record<string, string> = {
  Fastembed: 'cyan', OpenAi: 'green', Ollama: 'blue', Cohere: 'magenta',
  InMemory: 'default', Qdrant: 'orange', Pgvector: 'volcano', ChromaDb: 'lime',
  Pinecone: 'green', Weaviate: 'purple', Milvus: 'geekblue',
};

function EmbeddingVectorSection() {
  const { token } = theme.useToken();
  const [providerConfig, setProviderConfig] = useState<ProviderConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  // Embedding form state
  const [embKind, setEmbKind] = useState('Fastembed');
  const [embModel, setEmbModel] = useState('');
  const [embDimension, setEmbDimension] = useState(384);
  const [embBaseUrl, setEmbBaseUrl] = useState('');
  const [embApiKey, setEmbApiKey] = useState('');
  const [syncedEmbModels, setSyncedEmbModels] = useState<AvailableModel[] | null>(null);
  const [syncingEmb, setSyncingEmb] = useState(false);

  // Vector store form state
  const [vsKind, setVsKind] = useState('InMemory');
  const [vsUrl, setVsUrl] = useState('');
  const [vsCollection, setVsCollection] = useState('');
  const [vsApiKey, setVsApiKey] = useState('');
  const [vsIsolation, setVsIsolation] = useState('Shared');

  useEffect(() => {
    loadProviderConfig();
  }, []);

  async function loadProviderConfig() {
    try {
      const data = await getProviderConfig();
      setProviderConfig(data);
      setEmbKind(data.embedding.kind);
      setEmbModel(data.embedding.model);
      setEmbDimension(data.embedding.dimension);
      setEmbBaseUrl(data.embedding.base_url || '');
      setVsKind(data.vector_store.kind);
      setVsUrl(data.vector_store.url || '');
      setVsCollection(data.vector_store.collection || '');
      setVsIsolation(data.vector_store.isolation || 'Shared');
    } catch {
      message.error('Failed to load provider config');
    } finally {
      setLoading(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    try {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const req: Record<string, any> = {};

      // Embedding changes
      const emb: Record<string, unknown> = {};
      if (embKind !== providerConfig?.embedding.kind) emb.kind = embKind;
      if (embModel !== providerConfig?.embedding.model) emb.model = embModel;
      if (embDimension !== providerConfig?.embedding.dimension) emb.dimension = embDimension;
      if (embBaseUrl !== (providerConfig?.embedding.base_url || '')) emb.base_url = embBaseUrl;
      if (embApiKey) emb.api_key = embApiKey;
      if (Object.keys(emb).length > 0) req.embedding = emb;

      // Vector store changes
      const vs: Record<string, unknown> = {};
      if (vsKind !== providerConfig?.vector_store.kind) vs.kind = vsKind;
      if (vsUrl !== (providerConfig?.vector_store.url || '')) vs.url = vsUrl;
      if (vsCollection !== (providerConfig?.vector_store.collection || '')) vs.collection = vsCollection;
      if (vsApiKey) vs.api_key = vsApiKey;
      if (vsIsolation !== (providerConfig?.vector_store.isolation || 'Shared')) vs.isolation = vsIsolation;
      if (Object.keys(vs).length > 0) req.vector_store = vs;

      if (Object.keys(req).length === 0) {
        message.info('No changes to save');
        setSaving(false);
        return;
      }

      const updated = await updateProviderConfig(req);
      setProviderConfig(updated);
      setEmbKind(updated.embedding.kind);
      setEmbModel(updated.embedding.model);
      setEmbDimension(updated.embedding.dimension);
      setEmbBaseUrl(updated.embedding.base_url || '');
      setEmbApiKey('');
      setVsKind(updated.vector_store.kind);
      setVsUrl(updated.vector_store.url || '');
      setVsCollection(updated.vector_store.collection || '');
      setVsApiKey('');
      setVsIsolation(updated.vector_store.isolation || 'Shared');
      message.success('Embedding & Vector Store settings saved');
    } catch {
      message.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  const handleSyncEmb = async () => {
    setSyncingEmb(true);
    try {
      const result = await syncEmbeddingModels({
        kind: embKind,
        base_url: embBaseUrl || '',
        api_key: embApiKey || '',
      });
      if (result.models.length === 0) {
        message.warning('No embedding models found.');
      } else {
        message.success(`Found ${result.models.length} embedding model(s)`);
      }
      setSyncedEmbModels(result.models);
    } catch {
      message.error('Failed to sync embedding models.');
    } finally {
      setSyncingEmb(false);
    }
  };

  /** Set model and auto-fill dimension if known. */
  const handleEmbModelChange = (model: string) => {
    setEmbModel(model);
    const dim = lookupEmbDimension(model);
    if (dim) setEmbDimension(dim);
  };

  useEffect(() => {
    setSyncedEmbModels(null);
  }, [embKind]);

  const embModelOptions = (() => {
    if (syncedEmbModels && syncedEmbModels.length > 0) {
      return syncedEmbModels.map((m) => ({
        label: m.size ? `${m.name} (${formatModelSize(m.size)})` : m.name,
        value: m.id,
      }));
    }
    return staticEmbModels[embKind] || [];
  })();

  if (loading) return <Spin tip="Loading provider config..." />;

  return (
    <Card
      title={
        <Space>
          <SettingOutlined />
          <span>Embedding & Vector Store</span>
          <Tooltip title="After chunks are prepared (and optionally enriched), they are converted to numerical vectors by the Embedding Model, then stored in the Vector Database for fast similarity search. These are the final two steps of the ingestion pipeline.">
            <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
          </Tooltip>
        </Space>
      }
      extra={
        <Button
          type="primary"
          size="small"
          icon={<SaveOutlined />}
          onClick={handleSave}
          loading={saving}
        >
          Save
        </Button>
      }
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        {/* Embedding Model */}
        <div>
          <Space style={{ marginBottom: 8 }}>
            <Text strong>Embedding Model</Text>
            <Tooltip title="The embedding model converts text chunks into numerical vectors (arrays of numbers). Similar texts get similar vectors, which is how search finds relevant results. The dimension determines the vector size — higher dimensions capture more nuance but use more storage.">
              <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
            </Tooltip>
            {providerConfig && (
              <Tag color={kindColors[providerConfig.embedding.kind] || 'default'}>
                {providerConfig.embedding.kind}
              </Tag>
            )}
          </Space>

          <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
            <div style={{ minWidth: 180 }}>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Provider</Text>
              <Select
                size="small"
                value={embKind}
                onChange={(v) => { setEmbKind(v); setEmbModel(''); }}
                options={embeddingProviderOptions}
                style={{ width: 180 }}
              />
            </div>

            <div style={{ flex: 1, minWidth: 250 }}>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Model</Text>
              <Space.Compact style={{ width: '100%' }}>
                {embModelOptions.length > 0 ? (
                  <Select
                    size="small"
                    showSearch
                    optionFilterProp="label"
                    value={embModel || undefined}
                    onChange={handleEmbModelChange}
                    options={embModelOptions}
                    placeholder="Select embedding model"
                    style={{ width: '100%' }}
                  />
                ) : (
                  <Input
                    size="small"
                    value={embModel}
                    onChange={(e) => setEmbModel(e.target.value)}
                    onBlur={(e) => { const d = lookupEmbDimension(e.target.value); if (d) setEmbDimension(d); }}
                    placeholder="e.g. nomic-embed-text"
                    style={{ width: '100%' }}
                  />
                )}
                <Button
                  size="small"
                  icon={<SyncOutlined spin={syncingEmb} />}
                  onClick={handleSyncEmb}
                  loading={syncingEmb}
                >
                  Sync
                </Button>
              </Space.Compact>
            </div>

            <div>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Dimension</Text>
              <InputNumber
                size="small"
                min={1}
                max={8192}
                value={embDimension}
                onChange={(v) => v && setEmbDimension(v)}
                style={{ width: 90 }}
              />
            </div>
          </div>

          {(embKind === 'Ollama' || embKind === 'OpenAi') && (
            <div style={{ marginTop: 8 }}>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Base URL</Text>
              <Input
                size="small"
                value={embBaseUrl}
                onChange={(e) => setEmbBaseUrl(e.target.value)}
                placeholder={embKind === 'Ollama' ? 'http://localhost:11435' : 'https://api.openai.com (default)'}
                style={{ maxWidth: 400 }}
              />
            </div>
          )}

          {(embKind === 'OpenAi' || embKind === 'Cohere') && (
            <div style={{ marginTop: 8 }}>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>
                API Key {providerConfig?.embedding.has_api_key && <Tag color="success" style={{ fontSize: 10 }}>Configured</Tag>}
              </Text>
              <Input.Password
                size="small"
                value={embApiKey}
                onChange={(e) => setEmbApiKey(e.target.value)}
                placeholder={providerConfig?.embedding.has_api_key ? '(unchanged — leave blank to keep)' : 'Enter API key'}
                style={{ maxWidth: 400 }}
              />
            </div>
          )}

          <div style={{
            marginTop: 8,
            padding: '6px 12px',
            background: token.colorFillQuaternary,
            borderRadius: 6,
            fontSize: 12,
            color: token.colorTextSecondary,
          }}>
            {embKind === 'Fastembed' && 'Runs locally on your server — no API calls, no cost. Good for getting started.'}
            {embKind === 'Ollama' && 'Uses Ollama\'s embedding models locally. Good balance of quality and privacy.'}
            {embKind === 'OpenAi' && 'High-quality embeddings via OpenAI API. Best multilingual support. Costs ~$0.02 per 1M tokens.'}
            {embKind === 'Cohere' && 'Strong multilingual embeddings. Embed Multilingual v3 works well for Thai + English.'}
          </div>
        </div>

        <Divider style={{ margin: '4px 0' }} />

        {/* Vector Store */}
        <div>
          <Space style={{ marginBottom: 8 }}>
            <Text strong>Vector Database</Text>
            <Tooltip title="The vector database stores all chunk embeddings and enables fast similarity search. In-Memory is fine for development but loses data on restart. For production, use a persistent store like Qdrant, pgvector, or Pinecone.">
              <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
            </Tooltip>
            {providerConfig && (
              <Tag color={kindColors[providerConfig.vector_store.kind] || 'default'}>
                {providerConfig.vector_store.kind}
              </Tag>
            )}
          </Space>

          <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
            <div style={{ minWidth: 200 }}>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Provider</Text>
              <Select
                size="small"
                value={vsKind}
                onChange={setVsKind}
                options={vectorStoreOptions}
                style={{ width: 200 }}
              />
            </div>

            {vsKind !== 'InMemory' && (
              <div style={{ flex: 1, minWidth: 250 }}>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>URL</Text>
                <Input
                  size="small"
                  value={vsUrl}
                  onChange={(e) => setVsUrl(e.target.value)}
                  placeholder={vsUrlPlaceholders[vsKind] || 'Enter URL'}
                  style={{ width: '100%' }}
                />
              </div>
            )}

            {['Qdrant', 'ChromaDb', 'Weaviate', 'Milvus'].includes(vsKind) && (
              <div style={{ minWidth: 180 }}>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Collection</Text>
                <Input
                  size="small"
                  value={vsCollection}
                  onChange={(e) => setVsCollection(e.target.value)}
                  placeholder="thairag_chunks"
                  style={{ width: 180 }}
                />
              </div>
            )}
          </div>

          {['Pinecone', 'Weaviate'].includes(vsKind) && (
            <div style={{ marginTop: 8 }}>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>
                API Key {providerConfig?.vector_store.has_api_key && <Tag color="success" style={{ fontSize: 10 }}>Configured</Tag>}
              </Text>
              <Input.Password
                size="small"
                value={vsApiKey}
                onChange={(e) => setVsApiKey(e.target.value)}
                placeholder={providerConfig?.vector_store.has_api_key ? '(unchanged — leave blank to keep)' : 'Enter API key'}
                style={{ maxWidth: 400 }}
              />
            </div>
          )}

          {/* Data Isolation */}
          <div style={{ marginTop: 12 }}>
            <Space style={{ marginBottom: 6 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>Data Isolation</Text>
              <Tooltip title="Controls how vector data is separated across organizations and workspaces. Shared uses one collection with metadata filtering. Per-Organization or Per-Workspace creates separate collections for stronger data isolation.">
                <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
              </Tooltip>
            </Space>
            <Segmented
              size="small"
              value={vsIsolation}
              onChange={(v) => setVsIsolation(v as string)}
              options={[
                { label: 'Shared', value: 'Shared' },
                { label: 'Per Organization', value: 'PerOrganization' },
                { label: 'Per Workspace', value: 'PerWorkspace' },
              ]}
            />
            <div style={{ marginTop: 4, fontSize: 12, color: token.colorTextSecondary }}>
              {vsIsolation === 'Shared' && 'All data in one collection. Simplest setup — uses metadata filtering for access control.'}
              {vsIsolation === 'PerOrganization' && `Separate collection per organization. Collection name becomes a prefix (e.g. ${vsCollection || 'thairag_chunks'}_org_xxx).`}
              {vsIsolation === 'PerWorkspace' && `Separate collection per workspace. Maximum isolation but creates many collections (e.g. ${vsCollection || 'thairag_chunks'}_ws_xxx).`}
            </div>
            {vsIsolation !== (providerConfig?.vector_store.isolation || 'Shared') && (
              <Alert
                type="warning"
                showIcon
                style={{ marginTop: 6, fontSize: 12 }}
                message="Changing isolation strategy requires re-indexing all existing documents."
              />
            )}
          </div>

          <div style={{
            marginTop: 8,
            padding: '6px 12px',
            background: token.colorFillQuaternary,
            borderRadius: 6,
            fontSize: 12,
            color: token.colorTextSecondary,
          }}>
            {vsKind === 'InMemory' && 'Data is lost when the server restarts. Only for development and testing.'}
            {vsKind === 'Qdrant' && 'Purpose-built vector DB. Fast, reliable, supports filtering. Recommended for production.'}
            {vsKind === 'Pgvector' && 'Vector search as a PostgreSQL extension. Great if you already use Postgres — one less service to manage.'}
            {vsKind === 'ChromaDb' && 'Simple, developer-friendly vector DB. Easy to set up for small-to-medium datasets.'}
            {vsKind === 'Pinecone' && 'Fully managed cloud vector DB. Zero ops, auto-scaling. Pay per usage.'}
            {vsKind === 'Weaviate' && 'Open-source vector DB with hybrid search built in. Supports both vector and keyword search natively.'}
            {vsKind === 'Milvus' && 'High-performance vector DB built for scale. Good for large datasets (millions of vectors).'}
          </div>
        </div>
      </Space>
    </Card>
  );
}

function formatModelSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
