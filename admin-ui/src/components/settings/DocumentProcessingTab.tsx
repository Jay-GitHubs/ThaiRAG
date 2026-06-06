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
  AutoComplete,
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
import { getDocumentConfig, updateDocumentConfig, syncModels, getProviderConfig, updateProviderConfig } from '../../api/settings';
import { useLlmProfiles } from '../../hooks/useSettings';
import type {
  AiPreprocessingConfig,
  AvailableModel,
  DocumentConfigResponse,
  LlmConfigUpdate,
  LlmProviderInfo,
  ProviderConfigResponse,
  SettingsScopeParam,
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

// Recommendation predicates. Models are never hidden — these only decide which
// ones get the ★ marker and float to the top. A model is "recommended" when it
// fits the agent's task weight (and vision requirement, if any); everything
// else stays freely selectable.
function syncedModelRecommended(
  m: AvailableModel,
  kind: string,
  taskWeight: TaskWeight,
  requireVision?: boolean,
): boolean {
  const limit = taskWeight ? OLLAMA_SIZE_LIMITS[taskWeight] : undefined;
  const sizeOk = !limit || !m.size || m.size <= limit;
  const visionOk = !requireVision || kind !== 'Ollama' || isOllamaVisionModel(m.id);
  return sizeOk && visionOk;
}

function staticModelRecommended(
  m: StaticModel,
  taskWeight: TaskWeight,
  requireVision?: boolean,
): boolean {
  const allowed = taskWeight ? TIER_ALLOWED[taskWeight] : undefined;
  const tierOk = !allowed || allowed.has(m.tier);
  const visionOk = !requireVision || !!m.vision;
  return tierOk && visionOk;
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
  profile_id?: string;
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
    profile_id: info.profile_id,
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
  const { data: profiles } = useLlmProfiles();
  const profileList = profiles ?? [];
  const isProfileMode = !!form.profile_id;
  const selectedProfile = profileList.find((p) => p.id === form.profile_id);

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
      if (result.models.length === 0) {
        message.warning('No models found. Check your credentials and try again.');
      } else {
        const recCount = result.models.filter((m) =>
          syncedModelRecommended(m, form.kind, taskWeight, requireVision),
        ).length;
        message.success(
          taskWeight && recCount
            ? `Found ${result.models.length} model(s) — ${recCount} recommended for "${taskWeight}" tasks (★)`
            : `Found ${result.models.length} model(s)`,
        );
      }
      setSyncedModels(result.models);
      onModelsLoaded?.(result.models);
    } catch {
      message.error('Failed to sync models.');
    } finally {
      setSyncing(false);
    }
  };

  // Build the full model list — nothing is hidden. Recommended models (those
  // matching the agent's task weight + vision requirement) are marked with ★
  // and sorted first; every other model stays selectable, and free-text entry
  // is always allowed via AutoComplete.
  type ModelChoice = { value: string; display: string; recommended: boolean };

  let choices: ModelChoice[];
  if (syncedModels && syncedModels.length > 0) {
    choices = syncedModels.map((m) => {
      const visionTag =
        form.kind === 'Ollama' && isOllamaVisionModel(m.id) ? ' [vision]' : '';
      const sizeStr = m.size ? ` (${formatBytes(m.size)})` : '';
      return {
        value: m.id,
        display: `${m.name}${sizeStr}${visionTag}`,
        recommended: syncedModelRecommended(m, form.kind, taskWeight, requireVision),
      };
    });
  } else {
    choices = (staticModels[form.kind] || []).map((m) => ({
      value: m.value,
      display: m.label,
      recommended: staticModelRecommended(m, taskWeight, requireVision),
    }));
  }

  choices.sort((a, b) =>
    a.recommended === b.recommended
      ? a.display.localeCompare(b.display)
      : a.recommended
        ? -1
        : 1,
  );

  const modelOptions = choices.map((c) => ({
    value: c.value,
    label: (
      <span>
        {c.recommended && <span style={{ color: '#faad14' }}>★ </span>}
        {c.display}
      </span>
    ),
  }));

  const needsBaseUrl = form.kind === 'Ollama' || form.kind === 'OpenAiCompatible';
  const needsApiKey = ['Claude', 'OpenAi', 'Gemini', 'OpenAiCompatible'].includes(form.kind);
  const gap = compact ? 8 : 12;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap }}>
      {profileList.length > 0 && (
        <Segmented
          size="small"
          options={[
            { label: 'Custom', value: 'custom' },
            { label: 'Profile', value: 'profile' },
          ]}
          value={isProfileMode ? 'profile' : 'custom'}
          onChange={(v) => {
            if (v === 'profile') {
              const first = profileList[0];
              onChange({ ...form, profile_id: first.id, kind: first.kind, model: first.model });
            } else {
              onChange({ ...form, profile_id: undefined });
            }
          }}
        />
      )}

      {isProfileMode ? (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <Select
            size={compact ? 'small' : 'middle'}
            value={form.profile_id}
            onChange={(id) => {
              const p = profileList.find((pp) => pp.id === id);
              if (p) onChange({ ...form, profile_id: p.id, kind: p.kind, model: p.model });
            }}
            style={{ maxWidth: 400 }}
            options={profileList.map((p) => ({
              label: `${p.name} (${p.kind} / ${p.model})`,
              value: p.id,
            }))}
          />
          {selectedProfile && (
            <div style={{ fontSize: 12, display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
              <Tag>{selectedProfile.kind}</Tag>
              <Tag color="blue">{selectedProfile.model}</Tag>
              {selectedProfile.vault_key_name && (
                <Tag color="green">Key: {selectedProfile.vault_key_name}</Tag>
              )}
            </div>
          )}
        </div>
      ) : (
        <>
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
                <AutoComplete
                  size={compact ? 'small' : 'middle'}
                  options={modelOptions}
                  value={form.model || undefined}
                  onChange={(v) => onChange({ ...form, model: v })}
                  filterOption={(input, option) =>
                    String(option?.value ?? '').toLowerCase().includes(input.toLowerCase())
                  }
                  placeholder={
                    form.kind === 'Ollama'
                      ? 'Sync to discover, or type e.g. llama3.2'
                      : form.kind === 'OpenAiCompatible'
                      ? 'e.g. deepseek-chat'
                      : 'Select or type any model'
                  }
                  style={{ flex: 1 }}
                />
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
        </>
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

export function DocumentProcessingTab({ scope }: { scope?: SettingsScopeParam }) {
  const { token } = theme.useToken();
  // Tier-1 per-scope override: when a non-global scope is selected, only the
  // chunk-size knobs are editable; AI preprocessing + the other knobs stay
  // global-only and are hidden to avoid implying they're scope-aware.
  const isScoped = !!scope && scope.scope_type !== 'global';
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
  const [pdfImageDpi, setPdfImageDpi] = useState(150);
  const [maxImageEdge, setMaxImageEdge] = useState(2048);
  // Smart-PDF vision OCR knobs (global-only, opt-in escape hatch for scanned PDFs)
  const [imageDescriptionEnabled, setImageDescriptionEnabled] = useState(false);
  const [pdfVisionFallbackEnabled, setPdfVisionFallbackEnabled] = useState(true);
  const [pdfMinCharsPerPage, setPdfMinCharsPerPage] = useState(50);
  const [pdfMaxVisionPages, setPdfMaxVisionPages] = useState(100);
  const [pdfHighQuality, setPdfHighQuality] = useState(false);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope?.scope_type, scope?.scope_id]);

  async function loadConfig() {
    try {
      const data = await getDocumentConfig(scope);
      setConfig(data);
      setAiConfig(data.ai_preprocessing);
      setMaxChunkSize(data.max_chunk_size);
      setChunkOverlap(data.chunk_overlap);
      setMaxUploadSizeMb(data.max_upload_size_mb);
      setPdfImageDpi(data.pdf_image_dpi);
      setMaxImageEdge(data.max_image_edge);
      setImageDescriptionEnabled(data.image_description_enabled);
      setPdfVisionFallbackEnabled(data.pdf_vision_fallback_enabled);
      setPdfMinCharsPerPage(data.pdf_min_chars_per_page);
      setPdfMaxVisionPages(data.pdf_max_vision_pages);
      setPdfHighQuality(data.pdf_high_quality);

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
    if (form.profile_id) return null; // Profile mode — no validation needed
    if (!form.model.trim()) return `Please select or enter a model for ${label}`;
    const needsKey = ['Claude', 'OpenAi', 'Gemini'].includes(form.kind);
    if (needsKey && !form.api_key && !existingKey) return `API key is required for ${label}`;
    if (form.kind === 'OpenAiCompatible' && !form.base_url.trim())
      return `Base URL is required for ${label}`;
    return null;
  }

  function buildLlmUpdate(form: LlmFormState, hadProfileBefore?: boolean): LlmConfigUpdate {
    if (form.profile_id) {
      return { profile_id: form.profile_id };
    }
    const update: LlmConfigUpdate = {
      kind: form.kind,
      model: form.model.trim(),
      base_url: form.base_url.trim() || undefined,
      api_key: form.api_key || undefined,
    };
    if (hadProfileBefore) {
      update.clear_profile = true;
    }
    return update;
  }

  async function handleSavePipeline() {
    setSavingPipeline(true);
    try {
      // In a non-global scope only the two chunk knobs are scope-aware; don't
      // send the global-only fields so a workspace save can't clobber them.
      const req: UpdateDocumentConfigRequest = isScoped
        ? { max_chunk_size: maxChunkSize, chunk_overlap: chunkOverlap }
        : {
            max_chunk_size: maxChunkSize,
            chunk_overlap: chunkOverlap,
            max_upload_size_mb: maxUploadSizeMb,
            pdf_image_dpi: pdfImageDpi,
            max_image_edge: maxImageEdge,
            image_description_enabled: imageDescriptionEnabled,
            pdf_vision_fallback_enabled: pdfVisionFallbackEnabled,
            pdf_min_chars_per_page: pdfMinCharsPerPage,
            pdf_max_vision_pages: pdfMaxVisionPages,
            pdf_high_quality: pdfHighQuality,
          };
      const resp = await updateDocumentConfig(req, scope);
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
        ai.llm = buildLlmUpdate(llmForm, !!aiConfig.llm?.profile_id);
        // Remove per-agent overrides
        for (const agent of ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const) {
          (ai as Record<string, unknown>)[`remove_${agent}_llm`] = true;
        }
      } else if (llmMode === 'per-agent') {
        ai.remove_llm = true;
        for (const agent of ['analyzer', 'converter', 'quality', 'chunker', 'enricher', 'orchestrator'] as const) {
          const state = agentLlms[agent];
          if (state.enabled) {
            const existingKey = `${agent}_llm` as keyof AiPreprocessingConfig;
            const existingInfo = aiConfig[existingKey] as LlmProviderInfo | undefined;
            (ai as Record<string, unknown>)[`${agent}_llm`] = {
              ...buildLlmUpdate(state.form, !!existingInfo?.profile_id),
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
          {!isScoped && (
            <>
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
              <Space direction="vertical" size={2}>
                <Text type="secondary">PDF Render DPI (vision)</Text>
                <InputNumber
                  min={72}
                  max={600}
                  step={10}
                  value={pdfImageDpi}
                  onChange={(v) => v && setPdfImageDpi(v)}
                  style={{ width: 140 }}
                />
              </Space>
              <Space direction="vertical" size={2}>
                <Text type="secondary">Max Image Edge (px)</Text>
                <InputNumber
                  min={0}
                  max={8192}
                  step={128}
                  value={maxImageEdge}
                  onChange={(v) => v != null && setMaxImageEdge(v)}
                  style={{ width: 140 }}
                />
              </Space>
            </>
          )}
        </Space>
        {isScoped ? (
          <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0, fontSize: 12 }}>
            Editing <strong>{scope?.scope_type}</strong> scope. Only chunk size and overlap can be
            overridden per scope — they fall back to the global default when unset and apply at the
            next document upload/reprocess. Upload size, DPI, image edge, and AI preprocessing are
            global-only (switch the scope selector to Global to edit them).
          </Paragraph>
        ) : (
          <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0, fontSize: 12 }}>
            Note: Max upload size change takes effect after server restart. PDF Render DPI controls
            the resolution of PDF pages sent to the vision model — lower it (e.g. 110) to cut vision
            tokens and memory; raise it for sharper OCR on dense pages. Max Image Edge caps the
            longest side (px) of <em>every</em> image sent to vision — embedded DOCX/XLSX/HTML images
            and direct uploads too, not just PDFs — downscaling larger ones to bound token cost and
            RAM (0 disables).
          </Paragraph>
        )}

        {!isScoped && (
          <>
            <Divider style={{ margin: '16px 0 12px' }} />
            <Space style={{ marginBottom: 8 }}>
              <RobotOutlined />
              <Text strong>Smart-PDF Vision OCR</Text>
              <Switch
                size="small"
                checked={imageDescriptionEnabled}
                onChange={setImageDescriptionEnabled}
              />
              <Text type="secondary" style={{ fontSize: 12 }}>
                {imageDescriptionEnabled ? 'On — vision path enabled' : 'Off (default)'}
              </Text>
            </Space>
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 12 }}
              message="Use only for scanned / image-only PDFs"
              description={
                <span>
                  Vision OCR reads pages as images. On table- or figure-heavy pages it can
                  <strong> invent</strong> text — fabricated numbers then get embedded and cited as if
                  they came from the source. It needs a vision-capable model and is slow + RAM-heavy.
                  For faithful extraction prefer the DOCX/native source; keep this for PDFs that have
                  <em> no</em> text layer to extract.
                </span>
              }
            />
            {imageDescriptionEnabled && (
              <Space size="large" wrap>
                <Space direction="vertical" size={2}>
                  <Space size={4}>
                    <Text type="secondary">Fallback OCR for low-text pages</Text>
                    <Tooltip title="Rasterize + OCR only pages whose extracted text is below the char threshold (e.g. scanned or PowerPoint-exported pages). Digital-text pages skip vision entirely.">
                      <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </Space>
                  <Switch
                    checked={pdfVisionFallbackEnabled}
                    onChange={setPdfVisionFallbackEnabled}
                  />
                </Space>
                <Space direction="vertical" size={2}>
                  <Space size={4}>
                    <Text type="secondary">Min chars/page (below → OCR)</Text>
                    <Tooltip title="A PDF page with fewer than this many extracted characters is treated as 'no text' and routed to vision OCR.">
                      <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </Space>
                  <InputNumber
                    min={0}
                    max={100000}
                    step={10}
                    value={pdfMinCharsPerPage}
                    onChange={(v) => v != null && setPdfMinCharsPerPage(v)}
                    style={{ width: 140 }}
                  />
                </Space>
                <Space direction="vertical" size={2}>
                  <Space size={4}>
                    <Text type="secondary">Max OCR pages/doc</Text>
                    <Tooltip title="Hard cap on vision-LLM page calls per document — guards against a huge PDF translating to thousands of calls.">
                      <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </Space>
                  <InputNumber
                    min={1}
                    max={10000}
                    step={10}
                    value={pdfMaxVisionPages}
                    onChange={(v) => v && setPdfMaxVisionPages(v)}
                    style={{ width: 140 }}
                  />
                </Space>
                <Space direction="vertical" size={2}>
                  <Space size={4}>
                    <Text type="secondary">High quality (OCR every page)</Text>
                    <Tooltip title="Force vision OCR on EVERY page, not just low-text ones — highest fidelity for fully scanned docs, but slowest and most expensive, and maximizes hallucination exposure on tables/figures.">
                      <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextQuaternary }} />
                    </Tooltip>
                  </Space>
                  <Switch checked={pdfHighQuality} onChange={setPdfHighQuality} />
                </Space>
              </Space>
            )}
          </>
        )}
      </Card>
          ),
        }]}
      />

      {!isScoped && (<>
      {/* Document Vision LLM — the model the Smart-PDF Vision OCR above (and embedded
          image description) calls during ingestion. Global; lives beside its feature. */}
      <Collapse
        items={[{
          key: 'doc-vision',
          label: <><RobotOutlined /> Document Vision LLM — model for Smart-PDF OCR &amp; image description</>,
          children: <DocVisionSection />,
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
                  {llmMode === 'chat' && 'No dedicated document model is configured — agents currently reuse the main Chat LLM. Pick a mode above and save to give Document Processing its own model.'}
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
                              ? (state.form.profile_id
                                ? `Profile: ${state.form.kind} / ${state.form.model || '...'}`
                                : `${state.form.kind} / ${state.form.model || '...'}`)

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

      {/* Vector Database — final storage step (embedding model lives in Shared / Common) */}
      <Collapse
        defaultActiveKey={['vector-store']}
        items={[{
          key: 'vector-store',
          label: 'Vector Database',
          children: <VectorStoreSection />,
        }]}
      />
      </>)}
    </Space>
  );
}

// ── Embedding & Vector Store Section ────────────────────────────────

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

function VectorStoreSection() {
  const { token } = theme.useToken();
  const [providerConfig, setProviderConfig] = useState<ProviderConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

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
      const vs: Record<string, unknown> = {};
      if (vsKind !== providerConfig?.vector_store.kind) vs.kind = vsKind;
      if (vsUrl !== (providerConfig?.vector_store.url || '')) vs.url = vsUrl;
      if (vsCollection !== (providerConfig?.vector_store.collection || '')) vs.collection = vsCollection;
      if (vsApiKey) vs.api_key = vsApiKey;
      if (vsIsolation !== (providerConfig?.vector_store.isolation || 'Shared')) vs.isolation = vsIsolation;

      if (Object.keys(vs).length === 0) {
        message.info('No changes to save');
        setSaving(false);
        return;
      }

      const updated = await updateProviderConfig({ vector_store: vs });
      setProviderConfig(updated);
      setVsKind(updated.vector_store.kind);
      setVsUrl(updated.vector_store.url || '');
      setVsCollection(updated.vector_store.collection || '');
      setVsApiKey('');
      setVsIsolation(updated.vector_store.isolation || 'Shared');
      message.success('Vector Database settings saved');
    } catch {
      message.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  if (loading) return <Spin tip="Loading provider config..." />;

  return (
    <Card
      title={
        <Space>
          <SettingOutlined />
          <span>Vector Database</span>
          <Tooltip title="After chunks are embedded by the shared Embedding Model (configured in Shared / Common), the vectors are stored in the Vector Database for fast similarity search. This is the final step of the ingestion pipeline.">
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

// ── Document Vision LLM Section ─────────────────────────────────────
// Dedicated vision model for ingestion (OCR / image description). Global —
// stored in the provider config as `doc_vision_llm`. When disabled, ingestion
// vision falls back to the primary preprocessing LLM.
function DocVisionSection() {
  const { token } = theme.useToken();
  const [providerConfig, setProviderConfig] = useState<ProviderConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [form, setForm] = useState<LlmFormState>({ ...defaultLlmForm });
  const [numCtx, setNumCtx] = useState(0);

  useEffect(() => {
    (async () => {
      try {
        const data = await getProviderConfig();
        setProviderConfig(data);
        if (data.doc_vision_llm) {
          setEnabled(true);
          setForm(llmInfoToForm(data.doc_vision_llm));
          setNumCtx(data.doc_vision_llm.ollama_num_ctx_max ?? 0);
        }
      } catch {
        message.error('Failed to load provider config');
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const existing = providerConfig?.doc_vision_llm;
  const existingKey = !!existing?.has_api_key && form.kind === existing?.kind;

  async function handleSave() {
    if (enabled && !form.profile_id && !form.model.trim()) {
      message.error('Please select or enter a model for the Document Vision LLM');
      return;
    }
    setSaving(true);
    try {
      const req: { doc_vision_llm?: LlmConfigUpdate; clear_doc_vision_llm?: boolean } = {};
      if (!enabled) {
        if (existing) req.clear_doc_vision_llm = true;
        else {
          message.info('No changes to save');
          setSaving(false);
          return;
        }
      } else if (form.profile_id) {
        req.doc_vision_llm = { profile_id: form.profile_id };
      } else {
        req.doc_vision_llm = {
          kind: form.kind,
          model: form.model.trim(),
          base_url: form.base_url.trim() || undefined,
          api_key: form.api_key || undefined,
          ollama_num_ctx_max: numCtx,
          ...(existing?.profile_id ? { clear_profile: true } : {}),
        };
      }
      const updated = await updateProviderConfig(req);
      setProviderConfig(updated);
      if (updated.doc_vision_llm) {
        setEnabled(true);
        setForm(llmInfoToForm(updated.doc_vision_llm));
        setNumCtx(updated.doc_vision_llm.ollama_num_ctx_max ?? 0);
      } else {
        setEnabled(false);
        setForm({ ...defaultLlmForm });
        setNumCtx(0);
      }
      message.success('Document Vision LLM saved');
    } catch {
      message.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  if (loading) return <Spin tip="Loading provider config..." />;

  return (
    <Card
      title={
        <Space>
          <RobotOutlined />
          <span>Document Vision LLM</span>
          <Tooltip title="Dedicated vision model used during ingestion for OCR and image description (scanned PDFs, image-only documents). When off, vision tasks reuse the primary preprocessing LLM. Shared across the whole platform.">
            <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
          </Tooltip>
        </Space>
      }
      extra={
        <Space>
          <Switch
            checked={enabled}
            onChange={setEnabled}
            checkedChildren="Dedicated"
            unCheckedChildren="Use primary LLM"
            data-testid="doc-vision-switch"
          />
          <Button type="primary" size="small" icon={<SaveOutlined />} onClick={handleSave} loading={saving}>
            Save
          </Button>
        </Space>
      }
    >
      {!enabled ? (
        <Alert
          type="info"
          showIcon
          message="Vision tasks (image description, PDF OCR) will use the primary preprocessing LLM."
          description="Enable a dedicated vision model if your primary LLM is not vision-capable, or to isolate OCR memory usage."
        />
      ) : (
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Alert
            type="info"
            showIcon
            message="Vision-capable models"
            description="Cloud: Claude 3+ (Opus/Sonnet/Haiku), GPT-4o/4.1, Gemini 1.5+. Ollama: llava, llava-llama3, qwen2.5vl, llama3.2-vision, minicpm-v, bakllava, moondream."
          />
          <LlmConfigForm form={form} onChange={setForm} existingKey={existingKey} requireVision />
          {form.kind === 'Ollama' && !form.profile_id && (
            <div>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>
                Max Context (num_ctx)
                <Tooltip title="Vision calls request this full value (image token counts aren't known up front). Lower it, or lower PDF Render DPI, to cut memory. 0 = inherit model default.">
                  <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary, marginLeft: 6 }} />
                </Tooltip>
              </Text>
              <InputNumber min={0} max={131072} step={1024} value={numCtx} onChange={(v) => setNumCtx(v ?? 0)} style={{ width: 200 }} />
            </div>
          )}
        </Space>
      )}
    </Card>
  );
}
