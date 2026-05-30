import { useState, useEffect, useCallback } from 'react';
import {
  Card,
  Collapse,
  Descriptions,
  Tag,
  Table,
  Tabs,
  Spin,
  Alert,
  Button,
  Space,
  Typography,
  Form,
  Select,
  Input,
  AutoComplete,
  Tooltip,
  message,
  Divider,
  Switch,
} from 'antd';
import {
  EditOutlined,
  ReloadOutlined,
  SaveOutlined,
  CloseOutlined,
  SyncOutlined,
  StarFilled,
} from '@ant-design/icons';
import {
  useProviderConfig,
  useAvailableModels,
  useUpdateProviderConfig,
} from '../../hooks/useSettings';
import {
  syncModels,
  syncEmbeddingModels,
  syncRerankerModels,
  resolveRecommendations,
  refreshRecommendations,
  getRecommendationsStatus,
  getModelDiscoveryConfig,
  updateModelDiscoveryConfig,
} from '../../api/settings';
import type {
  AvailableModel,
  ModelCapabilities,
  ModelDiscoveryConfig,
  ProviderConfigResponse,
  RecommendationsStatus,
  SettingsScopeParam,
} from '../../api/types';
import { ChatPipelineCard } from './ChatPipelineCard';

const kindColors: Record<string, string> = {
  Ollama: 'blue',
  Claude: 'purple',
  OpenAi: 'green',
  OpenAiCompatible: 'lime',
  Gemini: 'gold',
  Fastembed: 'cyan',
  Cohere: 'magenta',
  Qdrant: 'orange',
  InMemory: 'default',
  Pgvector: 'volcano',
  ChromaDb: 'lime',
  Pinecone: 'green',
  Weaviate: 'purple',
  Milvus: 'geekblue',
  Tantivy: 'geekblue',
  Passthrough: 'default',
  Jina: 'blue',
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function formatAge(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
  return `${Math.floor(secs / 86400)}d`;
}

// Known vision-capable model name fragments — a frontend hint mirroring the
// backend's built-in list (is_ollama_vision_model / is_vision_model). This is
// ADVISORY only: it drives the ⭐ "vision" badge, never gates model selection
// (capability detection informs, never enforces — see PR-A). PR-D will replace
// this heuristic with the resolver-backed catalog.
const OLLAMA_VISION_PREFIXES = [
  'llava', 'llama3.2-vision', 'minicpm-v', 'bakllava',
  'moondream', 'moondream2', 'cogvlm', 'internvl',
  'qwen2.5vl', 'qwen2-vl', 'qwen3-vl', 'qwenvl', 'gemma3',
];

function looksVisionCapable(kind: string, modelId: string): boolean {
  const lower = modelId.toLowerCase();
  switch (kind) {
    case 'Ollama':
    case 'OpenAiCompatible': {
      const base = lower.split(':')[0]; // strip tag like ":latest", ":8b-instruct"
      return OLLAMA_VISION_PREFIXES.some((p) => base === p || base.startsWith(p + '-'));
    }
    case 'Claude':
      // All Claude 3.x and 4.x models are vision-capable.
      return (
        lower.startsWith('claude-3') ||
        lower.startsWith('claude-4') ||
        lower.includes('opus-4') ||
        lower.includes('sonnet-4') ||
        lower.includes('haiku-4')
      );
    case 'OpenAi':
      return ['gpt-4o', 'gpt-4.1', 'gpt-4-vision', 'o3', 'o4'].some((f) => lower.includes(f));
    case 'Gemini':
      return lower.includes('gemini-1.5') || lower.includes('gemini-2');
    default:
      return false;
  }
}

// Normalized shape consumed by the model AutoComplete options builder.
type PickerModel = { id: string; name?: string; size?: number; vision?: boolean };

// Advisory ⭐ "recommended" badge — explicitly framed as a recommendation, not a
// requirement (unknown models always stay selectable; capability informs, never
// enforces — see PR-A).
function RecommendedBadge() {
  return (
    <Tooltip title="On the recommended shortlist — a recommendation, not a requirement.">
      <Tag color="gold" icon={<StarFilled />} style={{ marginInlineEnd: 0 }}>
        recommended
      </Tag>
    </Tooltip>
  );
}

// Advisory "vision" capability tag.
function VisionBadge() {
  return (
    <Tooltip title="Recognized as vision-capable — a recommendation, not a requirement.">
      <Tag color="geekblue" style={{ marginInlineEnd: 0 }}>
        vision
      </Tag>
    </Tooltip>
  );
}

// Build AutoComplete options. `value` is the model id (what gets saved); the
// rich `label` shows name + id + capability badges + size; `searchName` lets the
// filter match a friendly name as well as the id. `caps` (when present) carries
// server-resolved vision/recommended flags that take precedence over the local
// heuristic; falling back to the local hint keeps badges working offline.
function toPickerOptions(
  models: PickerModel[],
  caps?: Record<string, ModelCapabilities>,
) {
  return models.map((m) => {
    const c = caps?.[m.id];
    const vision = c?.vision ?? m.vision ?? false;
    const recommended = c?.recommended ?? false;
    return {
      value: m.id,
      searchName: m.name ?? '',
      label: (
        <Space style={{ width: '100%', justifyContent: 'space-between' }} size={4}>
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>
            <Typography.Text>{m.name || m.id}</Typography.Text>
            {m.name && m.name !== m.id && (
              <Typography.Text type="secondary" code style={{ marginLeft: 6 }}>
                {m.id}
              </Typography.Text>
            )}
          </span>
          <Space size={4}>
            {recommended && <RecommendedBadge />}
            {vision && <VisionBadge />}
            {m.size != null && (
              <Typography.Text type="secondary">{formatBytes(m.size)}</Typography.Text>
            )}
          </Space>
        </Space>
      ),
    };
  });
}

// Filter against both the model id (option.value) and its friendly name so the
// search box works whether the admin types "gpt-4o" or "GPT-4o".
function modelFilterOption(
  input: string,
  option?: { value?: unknown; searchName?: string },
): boolean {
  const q = input.toLowerCase();
  return (
    String(option?.value ?? '').toLowerCase().includes(q) ||
    String(option?.searchName ?? '').toLowerCase().includes(q)
  );
}

function ReadOnlyView({ config }: { config: ProviderConfigResponse }) {
  const p = config;
  return (
    <Collapse
      defaultActiveKey={['llm-provider', 'vision-llm', 'reranker']}
      items={[
        {
          key: 'llm-provider',
          label: 'LLM Provider',
          children: (
            <Descriptions column={2} size="small" bordered>
              <Descriptions.Item label="Provider">
                <Tag color={kindColors[p.llm.kind] || 'default'}>{p.llm.kind}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Model">
                <Typography.Text code>{p.llm.model}</Typography.Text>
              </Descriptions.Item>
              {p.llm.base_url && (
                <Descriptions.Item label="Base URL">{p.llm.base_url}</Descriptions.Item>
              )}
              <Descriptions.Item label="API Key">
                {p.llm.has_api_key ? (
                  <Tag color="success">Configured</Tag>
                ) : (
                  <Tag color="default">Not set</Tag>
                )}
              </Descriptions.Item>
            </Descriptions>
          ),
        },
        {
          key: 'vision-llm',
          label: (
            <Space>
              <span>Vision LLM</span>
              {p.vision_llm ? (
                <Tag color="purple">Dedicated</Tag>
              ) : (
                <Tag color="default">Uses primary LLM</Tag>
              )}
            </Space>
          ),
          children: p.vision_llm ? (
            <Descriptions column={2} size="small" bordered>
              <Descriptions.Item label="Provider">
                <Tag color={kindColors[p.vision_llm.kind] || 'default'}>{p.vision_llm.kind}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Model">
                <Space>
                  <Typography.Text code>{p.vision_llm.model}</Typography.Text>
                  {p.vision_llm.supports_vision ? (
                    <Tag color="success">Vision-capable</Tag>
                  ) : (
                    <Tag color="warning">Not recognized as vision-capable</Tag>
                  )}
                </Space>
              </Descriptions.Item>
              {p.vision_llm.base_url && (
                <Descriptions.Item label="Base URL">{p.vision_llm.base_url}</Descriptions.Item>
              )}
              <Descriptions.Item label="API Key">
                {p.vision_llm.has_api_key ? (
                  <Tag color="success">Configured</Tag>
                ) : (
                  <Tag color="default">Not set</Tag>
                )}
              </Descriptions.Item>
            </Descriptions>
          ) : (
            <Alert
              type="info"
              showIcon
              message="Falls back to the primary LLM for image description and PDF OCR."
              description={
                p.llm.supports_vision
                  ? 'The primary LLM is vision-capable — this works.'
                  : 'The primary LLM is NOT vision-capable. Image-only documents will fail with "Vision OCR Required". Configure a dedicated vision LLM here, or switch primary to a vision model (Claude 3+, GPT-4o, Ollama llava/qwen2.5vl).'
              }
            />
          ),
        },
        {
          key: 'reranker',
          label: 'Reranker',
          children: (
            <Descriptions column={2} size="small" bordered>
              <Descriptions.Item label="Provider">
                <Tag color={kindColors[p.reranker.kind] || 'default'}>{p.reranker.kind}</Tag>
              </Descriptions.Item>
              {p.reranker.model && (
                <Descriptions.Item label="Model">
                  <Typography.Text code>{p.reranker.model}</Typography.Text>
                </Descriptions.Item>
              )}
              <Descriptions.Item label="API Key">
                {p.reranker.has_api_key ? (
                  <Tag color="success">Configured</Tag>
                ) : (
                  <Tag color="default">Not set</Tag>
                )}
              </Descriptions.Item>
            </Descriptions>
          ),
        },
      ]}
    />
  );
}

function EditForm({
  config,
  availableModels,
  onSave,
  onCancel,
  saving,
}: {
  config: ProviderConfigResponse;
  availableModels: AvailableModel[];
  onSave: (values: Record<string, unknown>) => void;
  onCancel: () => void;
  saving: boolean;
}) {
  const [form] = Form.useForm();
  const [llmKind, setLlmKind] = useState(config.llm.kind);
  const [rrKind, setRrKind] = useState(config.reranker.kind);
  const [syncedModels, setSyncedModels] = useState<AvailableModel[] | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncedRrModels, setSyncedRrModels] = useState<AvailableModel[] | null>(null);
  const [syncingRr, setSyncingRr] = useState(false);
  const [syncedVisionModels, setSyncedVisionModels] = useState<AvailableModel[] | null>(null);
  const [syncingVision, setSyncingVision] = useState(false);
  // Server-resolved capability flags (vision/recommended) for the displayed
  // option sets. Falls back to the local heuristic when the resolve call fails.
  const [llmCaps, setLlmCaps] = useState<Record<string, ModelCapabilities>>({});
  const [visionCaps, setVisionCaps] = useState<Record<string, ModelCapabilities>>({});
  // Vision LLM is a dedicated provider for image/PDF OCR — falls back
  // to the primary `llm` when disabled. See pipeline.rs::process_image
  // and process_pdf_with_vision.
  const [visionEnabled, setVisionEnabled] = useState(!!config.vision_llm);
  const [visionKind, setVisionKind] = useState(
    config.vision_llm?.kind || config.llm.kind,
  );

  useEffect(() => {
    form.setFieldsValue({
      llm_kind: config.llm.kind,
      llm_model: config.llm.model,
      llm_base_url: config.llm.base_url || '',
      llm_api_key: '',
      rr_kind: config.reranker.kind,
      rr_model: config.reranker.model || '',
      rr_api_key: '',
      vision_kind: config.vision_llm?.kind || config.llm.kind,
      vision_model: config.vision_llm?.model || '',
      vision_base_url: config.vision_llm?.base_url || '',
      vision_api_key: '',
    });
  }, [config, form]);

  const handleFinish = (values: Record<string, unknown>) => {
    // Build partial update — only send fields that changed
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const req: Record<string, any> = {};

    const llm: Record<string, unknown> = {};
    if (values.llm_kind !== config.llm.kind) llm.kind = values.llm_kind;
    if (values.llm_model !== config.llm.model) llm.model = values.llm_model;
    if (values.llm_base_url !== (config.llm.base_url || '')) llm.base_url = values.llm_base_url;
    if (values.llm_api_key) llm.api_key = values.llm_api_key;
    if (Object.keys(llm).length > 0) req.llm = llm;

    const rr: Record<string, unknown> = {};
    if (values.rr_kind !== config.reranker.kind) rr.kind = values.rr_kind;
    if (values.rr_model !== (config.reranker.model || '')) rr.model = values.rr_model;
    if (values.rr_api_key) rr.api_key = values.rr_api_key;
    if (Object.keys(rr).length > 0) req.reranker = rr;

    // Vision LLM — three cases:
    //   1. Toggle off but config has one  → send `clear_vision_llm: true`
    //   2. Toggle on  → send `vision_llm` (kind+model required, others optional)
    //   3. Toggle off and no existing config → no-op
    if (!visionEnabled && config.vision_llm) {
      req.clear_vision_llm = true;
    } else if (visionEnabled) {
      const vision: Record<string, unknown> = {
        kind: values.vision_kind,
        model: values.vision_model,
      };
      if (values.vision_base_url) vision.base_url = values.vision_base_url;
      if (values.vision_api_key) vision.api_key = values.vision_api_key;
      req.vision_llm = vision;
    }

    if (Object.keys(req).length === 0) {
      message.info('No changes to save');
      return;
    }

    onSave(req);
  };

  // Static model lists for providers that don't have a live discovery API
  const staticModels: Record<string, { label: string; value: string }[]> = {
    Claude: [
      { label: 'Claude Opus 4', value: 'claude-opus-4-20250514' },
      { label: 'Claude Sonnet 4', value: 'claude-sonnet-4-20250514' },
      { label: 'Claude Haiku 4', value: 'claude-haiku-4-20250414' },
      { label: 'Claude 3.5 Sonnet', value: 'claude-3-5-sonnet-20241022' },
    ],
    OpenAi: [
      { label: 'GPT-4o', value: 'gpt-4o' },
      { label: 'GPT-4o Mini', value: 'gpt-4o-mini' },
      { label: 'GPT-4.1', value: 'gpt-4.1' },
      { label: 'GPT-4.1 Mini', value: 'gpt-4.1-mini' },
      { label: 'GPT-4.1 Nano', value: 'gpt-4.1-nano' },
      { label: 'o3', value: 'o3' },
      { label: 'o3 Mini', value: 'o3-mini' },
      { label: 'o4 Mini', value: 'o4-mini' },
    ],
    Gemini: [
      { label: 'Gemini 2.5 Pro', value: 'gemini-2.5-pro' },
      { label: 'Gemini 2.5 Flash', value: 'gemini-2.5-flash' },
      { label: 'Gemini 2.0 Flash', value: 'gemini-2.0-flash' },
      { label: 'Gemini 1.5 Pro', value: 'gemini-1.5-pro' },
      { label: 'Gemini 1.5 Flash', value: 'gemini-1.5-flash' },
    ],
  };

  // Build LLM picker models — prefer synced, then API-fetched, then static.
  // Each is tagged with an advisory vision flag for the ⭐ badge.
  const llmPickerModels: PickerModel[] = (() => {
    const fromAvailable =
      syncedModels && syncedModels.length > 0
        ? syncedModels
        : llmKind === config.llm.kind && availableModels.length > 0
          ? availableModels
          : null;
    if (fromAvailable) {
      return fromAvailable.map((m) => ({
        id: m.id,
        name: m.name,
        size: m.size,
        vision: looksVisionCapable(llmKind, m.id),
      }));
    }
    return (staticModels[llmKind] || []).map((m) => ({
      id: m.value,
      name: m.label,
      vision: looksVisionCapable(llmKind, m.value),
    }));
  })();
  const llmModelIds = llmPickerModels.map((m) => m.id).filter(Boolean).join(',');
  useEffect(() => {
    const ids = llmModelIds ? llmModelIds.split(',') : [];
    if (ids.length === 0) {
      setLlmCaps({});
      return;
    }
    let cancelled = false;
    resolveRecommendations({ kind: llmKind, models: ids })
      .then((r) => {
        if (!cancelled) setLlmCaps(r.resolved);
      })
      .catch(() => {
        /* keep local-heuristic fallback */
      });
    return () => {
      cancelled = true;
    };
  }, [llmKind, llmModelIds]);
  const llmOptions = toPickerOptions(llmPickerModels, llmCaps);

  // Sync models from provider API using current form values
  const handleSyncModels = async () => {
    const values = form.getFieldsValue();
    setSyncing(true);
    try {
      const result = await syncModels({
        kind: llmKind,
        base_url: values.llm_base_url || '',
        api_key: values.llm_api_key || '',
      });
      if (result.models.length === 0) {
        message.warning('No models found. Check your credentials and try again.');
      } else {
        message.success(`Found ${result.models.length} model(s) from ${result.provider}`);
      }
      setSyncedModels(result.models);
    } catch {
      message.error('Failed to sync models. Check your credentials.');
    } finally {
      setSyncing(false);
    }
  };

  // Sync vision models from the (possibly dedicated) vision provider API.
  const handleSyncVisionModels = async () => {
    const values = form.getFieldsValue();
    setSyncingVision(true);
    try {
      const result = await syncModels({
        kind: visionKind,
        base_url: values.vision_base_url || '',
        api_key: values.vision_api_key || '',
      });
      if (result.models.length === 0) {
        message.warning('No models found. Check your credentials and try again.');
      } else {
        message.success(`Found ${result.models.length} model(s) from ${result.provider}`);
      }
      setSyncedVisionModels(result.models);
    } catch {
      message.error('Failed to sync models. Check your credentials.');
    } finally {
      setSyncingVision(false);
    }
  };

  // Vision picker models — synced list (vision provider) or the static fallback,
  // each tagged with the advisory vision flag.
  const visionPickerModels: PickerModel[] = (
    syncedVisionModels && syncedVisionModels.length > 0
      ? syncedVisionModels.map((m) => ({
          id: m.id,
          name: m.name,
          size: m.size,
          vision: looksVisionCapable(visionKind, m.id),
        }))
      : (staticModels[visionKind] || []).map((m) => ({
          id: m.value,
          name: m.label,
          vision: looksVisionCapable(visionKind, m.value),
        }))
  );
  const visionModelIds = visionPickerModels.map((m) => m.id).filter(Boolean).join(',');
  useEffect(() => {
    const ids = visionModelIds ? visionModelIds.split(',') : [];
    if (ids.length === 0) {
      setVisionCaps({});
      return;
    }
    let cancelled = false;
    resolveRecommendations({ kind: visionKind, models: ids })
      .then((r) => {
        if (!cancelled) setVisionCaps(r.resolved);
      })
      .catch(() => {
        /* keep local-heuristic fallback */
      });
    return () => {
      cancelled = true;
    };
  }, [visionKind, visionModelIds]);
  const visionOptions = toPickerOptions(visionPickerModels, visionCaps);

  // Reset synced vision models when switching the vision provider kind.
  useEffect(() => {
    setSyncedVisionModels(null);
  }, [visionKind]);

  // Reset synced models and model selection when switching provider kind
  useEffect(() => {
    setSyncedModels(null);
    if (llmKind !== config.llm.kind) {
      form.setFieldValue('llm_model', undefined);
      // Clear base_url when switching to providers that use their own default URL
      if (llmKind !== 'Ollama' && llmKind !== 'OpenAiCompatible') {
        form.setFieldValue('llm_base_url', '');
      }
    } else {
      form.setFieldValue('llm_model', config.llm.model);
    }
  }, [llmKind, config.llm.kind, config.llm.model, form]);

  // Reranker model options + sync
  const staticRrModels: Record<string, { label: string; value: string }[]> = {
    Cohere: [
      { label: 'Rerank v3.5', value: 'rerank-v3.5' },
      { label: 'Rerank English v3.0', value: 'rerank-english-v3.0' },
      { label: 'Rerank Multilingual v3.0', value: 'rerank-multilingual-v3.0' },
      { label: 'Rerank English v2.0', value: 'rerank-english-v2.0' },
    ],
    Jina: [
      { label: 'Jina Reranker v2 Base Multilingual', value: 'jina-reranker-v2-base-multilingual' },
      { label: 'Jina Reranker v1 Base EN', value: 'jina-reranker-v1-base-en' },
      { label: 'Jina Reranker v1 Turbo EN', value: 'jina-reranker-v1-turbo-en' },
      { label: 'Jina Reranker v1 Tiny EN', value: 'jina-reranker-v1-tiny-en' },
    ],
  };

  const rrPickerModels: PickerModel[] =
    syncedRrModels && syncedRrModels.length > 0
      ? syncedRrModels.map((m) => ({ id: m.id, name: m.name, size: m.size }))
      : (staticRrModels[rrKind] || []).map((m) => ({ id: m.value, name: m.label }));
  const rrOptions = toPickerOptions(rrPickerModels);

  const handleSyncRrModels = async () => {
    setSyncingRr(true);
    try {
      const result = await syncRerankerModels({ kind: rrKind });
      if (result.models.length === 0) {
        message.warning('No reranker models found.');
      } else {
        message.success(`Found ${result.models.length} reranker model(s) from ${result.provider}`);
      }
      setSyncedRrModels(result.models);
    } catch {
      message.error('Failed to sync reranker models.');
    } finally {
      setSyncingRr(false);
    }
  };

  useEffect(() => {
    setSyncedRrModels(null);
    if (rrKind !== config.reranker.kind) {
      form.setFieldValue('rr_model', undefined);
    } else {
      form.setFieldValue('rr_model', config.reranker.model || undefined);
    }
  }, [rrKind, config.reranker.kind, config.reranker.model, form]);

  return (
    <Form form={form} layout="vertical" onFinish={handleFinish}>
      <Card
        title="LLM Provider"
        size="small"
        extra={
          <Space>
            <Button icon={<SaveOutlined />} type="primary" htmlType="submit" loading={saving}>
              Save All
            </Button>
            <Button icon={<CloseOutlined />} onClick={onCancel}>
              Cancel
            </Button>
          </Space>
        }
      >
        <Space style={{ width: '100%' }} direction="vertical">
          <Form.Item name="llm_kind" label="Provider" rules={[{ required: true }]}>
            <Select
              onChange={(v) => setLlmKind(v)}
              options={[
                { label: 'Ollama (Local)', value: 'Ollama' },
                { label: 'Claude (Anthropic)', value: 'Claude' },
                { label: 'OpenAI', value: 'OpenAi' },
                { label: 'Gemini (Google)', value: 'Gemini' },
                { label: 'OpenAI-Compatible (Groq, Mistral, Together AI, vLLM, etc.)', value: 'OpenAiCompatible' },
              ]}
            />
          </Form.Item>
          <Form.Item
            label="Model"
            required
            style={{ marginBottom: 0 }}
            extra="Search the synced list or type any model id — unrecognized models stay selectable."
          >
            <Space.Compact style={{ width: '100%' }}>
              <Form.Item name="llm_model" noStyle rules={[{ required: true, message: 'Model is required' }]}>
                <AutoComplete
                  options={llmOptions}
                  filterOption={modelFilterOption}
                  placeholder={
                    llmKind === 'OpenAiCompatible'
                      ? 'e.g. deepseek-chat, mistral-large-latest'
                      : 'Select or type a model'
                  }
                  style={{ width: '100%' }}
                />
              </Form.Item>
              <Button
                icon={<SyncOutlined spin={syncing} />}
                onClick={handleSyncModels}
                loading={syncing}
                title="Sync models from provider"
              >
                Sync
              </Button>
            </Space.Compact>
          </Form.Item>
          {llmKind === 'Ollama' && (
            <Form.Item name="llm_base_url" label="Base URL">
              <Input placeholder="http://localhost:11435" />
            </Form.Item>
          )}
          {llmKind === 'OpenAiCompatible' && (
            <Form.Item name="llm_base_url" label="Base URL" rules={[{ required: true }]} extra="The base URL of your OpenAI-compatible API provider">
              <Input placeholder="e.g. https://api.groq.com/openai, https://api.together.xyz" />
            </Form.Item>
          )}
          {(llmKind === 'Claude' || llmKind === 'OpenAi' || llmKind === 'Gemini' || llmKind === 'OpenAiCompatible') && (
            <Form.Item name="llm_api_key" label="API Key" rules={llmKind !== 'OpenAiCompatible' ? [{ required: !config.llm.has_api_key }] : undefined}>
              <Input.Password
                placeholder={config.llm.has_api_key ? '(unchanged — leave blank to keep)' : 'Enter API key'}
              />
            </Form.Item>
          )}
        </Space>
      </Card>

      <Divider />

      <Card
        title="Vision LLM"
        size="small"
        extra={
          <Switch
            checked={visionEnabled}
            onChange={setVisionEnabled}
            checkedChildren="Dedicated"
            unCheckedChildren="Use primary LLM"
          />
        }
      >
        {!visionEnabled ? (
          <Alert
            type="info"
            showIcon
            message="Vision tasks (image description, PDF OCR) will use the primary LLM."
            description={
              config.llm.supports_vision
                ? 'Your primary LLM is vision-capable — this works.'
                : 'Warning: your primary LLM is NOT recognized as vision-capable. Image-only documents will fail with "Vision OCR Required" until you either enable a dedicated vision LLM here or switch your primary to a vision model.'
            }
          />
        ) : (
          <Space style={{ width: '100%' }} direction="vertical">
            <Alert
              type="info"
              showIcon
              message="Vision-capable models"
              description="Cloud: Claude 3+ (Opus/Sonnet/Haiku), GPT-4o/4V, Gemini 1.5+. Ollama: llava, llava-llama3, qwen2.5vl, llama3.2-vision, minicpm-v, bakllava, moondream."
            />
            <Form.Item name="vision_kind" label="Provider" rules={[{ required: true }]}>
              <Select
                onChange={(v) => setVisionKind(v)}
                options={[
                  { label: 'Ollama (Local)', value: 'Ollama' },
                  { label: 'Claude (Anthropic)', value: 'Claude' },
                  { label: 'OpenAI', value: 'OpenAi' },
                  { label: 'Gemini (Google)', value: 'Gemini' },
                  { label: 'OpenAI-Compatible', value: 'OpenAiCompatible' },
                ]}
              />
            </Form.Item>
            <Form.Item
              label="Model"
              required
              style={{ marginBottom: 0 }}
              extra={
                visionKind === 'Ollama'
                  ? 'e.g. llava:13b, qwen2.5vl:7b, llama3.2-vision:11b — or type any model id.'
                  : visionKind === 'Claude'
                  ? 'e.g. claude-sonnet-4-20250514 (all Claude 3+ models support vision)'
                  : visionKind === 'OpenAi'
                  ? 'e.g. gpt-4o, gpt-4o-mini'
                  : visionKind === 'Gemini'
                  ? 'e.g. gemini-2.5-pro, gemini-1.5-pro'
                  : 'Any OpenAI-compatible vision model'
              }
            >
              <Space.Compact style={{ width: '100%' }}>
                <Form.Item
                  name="vision_model"
                  noStyle
                  rules={[{ required: true, message: 'Model is required' }]}
                >
                  <AutoComplete
                    options={visionOptions}
                    filterOption={modelFilterOption}
                    placeholder="Select or type a vision model"
                    style={{ width: '100%' }}
                  />
                </Form.Item>
                <Button
                  icon={<SyncOutlined spin={syncingVision} />}
                  onClick={handleSyncVisionModels}
                  loading={syncingVision}
                  title="Sync models from provider"
                >
                  Sync
                </Button>
              </Space.Compact>
            </Form.Item>
            {(visionKind === 'Ollama' || visionKind === 'OpenAiCompatible') && (
              <Form.Item
                name="vision_base_url"
                label="Base URL"
                rules={visionKind === 'OpenAiCompatible' ? [{ required: true }] : undefined}
                extra={
                  visionKind === 'Ollama'
                    ? 'Leave blank to use the same base URL as the primary LLM.'
                    : undefined
                }
              >
                <Input
                  placeholder={
                    visionKind === 'Ollama'
                      ? 'http://localhost:11435'
                      : 'e.g. https://api.together.xyz'
                  }
                />
              </Form.Item>
            )}
            {(visionKind === 'Claude' ||
              visionKind === 'OpenAi' ||
              visionKind === 'Gemini' ||
              visionKind === 'OpenAiCompatible') && (
              <Form.Item name="vision_api_key" label="API Key">
                <Input.Password
                  placeholder={
                    config.vision_llm?.has_api_key
                      ? '(unchanged — leave blank to keep)'
                      : 'Enter API key (or reuse primary LLM\'s key by leaving blank)'
                  }
                />
              </Form.Item>
            )}
          </Space>
        )}
      </Card>

      <Divider />

      <Card title="Reranker" size="small">
        <Form.Item name="rr_kind" label="Provider" rules={[{ required: true }]}>
          <Select
            onChange={(v) => setRrKind(v)}
            options={[
              { label: 'Passthrough (None)', value: 'Passthrough' },
              { label: 'Cohere', value: 'Cohere' },
              { label: 'Jina', value: 'Jina' },
            ]}
          />
        </Form.Item>
        {(rrKind === 'Cohere' || rrKind === 'Jina') && (
          <>
            <Form.Item label="Model" required style={{ marginBottom: 0 }}>
              <Space.Compact style={{ width: '100%' }}>
                <Form.Item name="rr_model" noStyle rules={[{ required: true, message: 'Model is required' }]}>
                  <AutoComplete
                    options={rrOptions}
                    filterOption={modelFilterOption}
                    placeholder="Select or type a reranker model"
                    style={{ width: '100%' }}
                  />
                </Form.Item>
                <Button
                  icon={<SyncOutlined spin={syncingRr} />}
                  onClick={handleSyncRrModels}
                  loading={syncingRr}
                  title="Sync reranker models from provider"
                >
                  Sync
                </Button>
              </Space.Compact>
            </Form.Item>
            <Form.Item name="rr_api_key" label="API Key">
              <Input.Password
                placeholder={
                  config.reranker.has_api_key
                    ? '(unchanged — leave blank to keep)'
                    : 'Enter API key'
                }
              />
            </Form.Item>
          </>
        )}
      </Card>
    </Form>
  );
}

function ModelTable({
  models,
  loading,
  provider,
}: {
  models?: AvailableModel[];
  loading: boolean;
  provider?: string;
}) {
  const columns = [
    { title: 'Model ID', dataIndex: 'id', key: 'id' },
    { title: 'Name', dataIndex: 'name', key: 'name' },
    {
      title: 'Size',
      dataIndex: 'size',
      key: 'size',
      render: (v?: number) => (v ? formatBytes(v) : '-'),
    },
  ];
  return (
    <>
      {provider && (
        <Typography.Text type="secondary" style={{ marginBottom: 8, display: 'block' }}>
          Source: <Tag>{provider}</Tag>
          {models?.length ?? 0} model(s) available
        </Typography.Text>
      )}
      <Table<AvailableModel>
        rowKey="id"
        columns={columns}
        dataSource={models}
        loading={loading}
        pagination={false}
        size="small"
      />
    </>
  );
}

function AvailableModelsPanel({
  config,
  llmModels,
  onRefreshLlm,
}: {
  config: ProviderConfigResponse;
  llmModels: ReturnType<typeof useAvailableModels>;
  onRefreshLlm: () => void;
}) {
  const [embModels, setEmbModels] = useState<AvailableModel[]>([]);
  const [embProvider, setEmbProvider] = useState<string>();
  const [embLoading, setEmbLoading] = useState(false);
  const [rrModels, setRrModels] = useState<AvailableModel[]>([]);
  const [rrProvider, setRrProvider] = useState<string>();
  const [rrLoading, setRrLoading] = useState(false);

  const loadEmbedding = useCallback(async () => {
    setEmbLoading(true);
    try {
      const res = await syncEmbeddingModels({ kind: config.embedding.kind });
      setEmbModels(res.models);
      setEmbProvider(res.provider);
    } catch {
      setEmbModels([]);
    } finally {
      setEmbLoading(false);
    }
  }, [config.embedding.kind]);

  const loadReranker = useCallback(async () => {
    setRrLoading(true);
    try {
      const res = await syncRerankerModels({ kind: config.reranker.kind });
      setRrModels(res.models);
      setRrProvider(res.provider);
    } catch {
      setRrModels([]);
    } finally {
      setRrLoading(false);
    }
  }, [config.reranker.kind]);

  const llmCount = llmModels.data?.models.length ?? 0;
  const embCount = embModels.length;
  const rrCount = rrModels.length;

  return (
    <Collapse
      size="small"
      items={[
        {
          key: 'available-models',
          label: (
            <span>
              Available Models
              {llmCount > 0 && <Tag style={{ marginLeft: 8 }}>{llmCount} LLM</Tag>}
              {embCount > 0 && <Tag style={{ marginLeft: 4 }}>{embCount} Embedding</Tag>}
              {rrCount > 0 && <Tag style={{ marginLeft: 4 }}>{rrCount} Reranker</Tag>}
            </span>
          ),
          children: (
            <Tabs
              size="small"
              items={[
                {
                  key: 'llm',
                  label: `LLM (${llmCount})`,
                  children: (
                    <>
                      <div style={{ marginBottom: 8, textAlign: 'right' }}>
                        <Button
                          icon={<ReloadOutlined />}
                          size="small"
                          onClick={onRefreshLlm}
                          loading={llmModels.isFetching}
                        >
                          Refresh
                        </Button>
                      </div>
                      <ModelTable
                        models={llmModels.data?.models}
                        loading={llmModels.isLoading}
                        provider={llmModels.data?.provider}
                      />
                    </>
                  ),
                },
                {
                  key: 'embedding',
                  label: `Embedding (${embCount})`,
                  children: (
                    <>
                      <div style={{ marginBottom: 8, textAlign: 'right' }}>
                        <Button
                          icon={<SyncOutlined />}
                          size="small"
                          onClick={loadEmbedding}
                          loading={embLoading}
                        >
                          {embCount > 0 ? 'Refresh' : 'Load Models'}
                        </Button>
                      </div>
                      <ModelTable models={embModels} loading={embLoading} provider={embProvider} />
                    </>
                  ),
                },
                {
                  key: 'reranker',
                  label: `Reranker (${rrCount})`,
                  children: (
                    <>
                      <div style={{ marginBottom: 8, textAlign: 'right' }}>
                        <Button
                          icon={<SyncOutlined />}
                          size="small"
                          onClick={loadReranker}
                          loading={rrLoading}
                        >
                          {rrCount > 0 ? 'Refresh' : 'Load Models'}
                        </Button>
                      </div>
                      <ModelTable models={rrModels} loading={rrLoading} provider={rrProvider} />
                    </>
                  ),
                },
              ]}
            />
          ),
        },
      ]}
    />
  );
}

// Model-discovery status + config UX. Surfaces where the advisory ⭐/vision
// badges come from (external catalog vs built-in floor) and lets an admin
// enable/point the catalog or trigger a refresh. On mount it warms the catalog
// in the background (fire-and-forget) so the next Settings open is fresh.
function RecommendationsCard() {
  const [status, setStatus] = useState<RecommendationsStatus | null>(null);
  const [cfg, setCfg] = useState<ModelDiscoveryConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(async () => {
    try {
      const [s, c] = await Promise.all([getRecommendationsStatus(), getModelDiscoveryConfig()]);
      setStatus(s);
      setCfg(c);
    } catch {
      /* non-fatal — the pickers still work with the local heuristic */
    }
  }, []);

  useEffect(() => {
    refreshRecommendations().catch(() => {});
    load();
  }, [load]);

  const handleSave = async () => {
    if (!cfg) return;
    setSaving(true);
    try {
      const saved = await updateModelDiscoveryConfig(cfg);
      setCfg(saved);
      message.success('Model discovery settings saved');
      await load();
    } catch {
      message.error('Failed to save model discovery settings');
    } finally {
      setSaving(false);
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await refreshRecommendations();
      message.info('Refreshing recommendations from the catalog…');
      // The server refresh runs in the background; poll status shortly after.
      window.setTimeout(() => {
        load();
        setRefreshing(false);
      }, 2000);
    } catch {
      message.error('Failed to trigger refresh');
      setRefreshing(false);
    }
  };

  const usingBuiltin = !status?.has_data;
  const sourceText = !status
    ? '—'
    : status.has_data
      ? `External catalog — ${status.model_count} models${
          status.age_secs != null ? `, updated ${formatAge(status.age_secs)} ago` : ''
        }`
      : 'Built-in recommendations (offline floor)';

  return (
    <Collapse
      size="small"
      items={[
        {
          key: 'recommendations',
          label: (
            <span>
              Model Recommendations
              {usingBuiltin ? (
                <Tag style={{ marginLeft: 8 }}>built-in</Tag>
              ) : (
                <Tag color="gold" style={{ marginLeft: 8 }}>
                  catalog
                </Tag>
              )}
            </span>
          ),
          children: (
            <Space direction="vertical" style={{ width: '100%' }} size="middle">
              {usingBuiltin && (
                <Alert
                  type="info"
                  showIcon
                  message="Using built-in recommendations"
                  description="Enable an external catalog (e.g. LiteLLM / models.dev) below to keep the ⭐ recommended and vision badges accurate as new models ship. Fetches are GETs of a public catalog — no data leaves your deployment, and it can be disabled for air-gapped installs."
                />
              )}
              {status?.last_error && (
                <Alert
                  type="warning"
                  showIcon
                  message="Last catalog fetch failed"
                  description={status.last_error}
                />
              )}
              <Descriptions size="small" column={1} bordered>
                <Descriptions.Item label="Source">{sourceText}</Descriptions.Item>
              </Descriptions>
              {cfg && (
                <>
                  <Space>
                    <span>Enable external discovery</span>
                    <Switch
                      checked={cfg.enabled}
                      onChange={(v) => setCfg({ ...cfg, enabled: v })}
                    />
                  </Space>
                  <div>
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      Discovery source
                    </Typography.Text>
                    <Select
                      value={cfg.mode || 'catalog'}
                      onChange={(v) => setCfg({ ...cfg, mode: v })}
                      disabled={!cfg.enabled}
                      style={{ width: '100%', marginTop: 2 }}
                      options={[
                        { label: 'Built-in catalog (LiteLLM)', value: 'catalog' },
                        { label: 'Custom HTTP catalog', value: 'http_catalog' },
                        { label: 'MCP discovery tool', value: 'mcp' },
                      ]}
                    />
                  </div>

                  {cfg.mode === 'catalog' && (
                    <div>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        Catalog URL (leave blank for the default LiteLLM catalog)
                      </Typography.Text>
                      <Input
                        value={cfg.catalog_url}
                        onChange={(e) => setCfg({ ...cfg, catalog_url: e.target.value })}
                        placeholder="https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
                        disabled={!cfg.enabled}
                        style={{ marginTop: 2 }}
                      />
                    </div>
                  )}

                  {cfg.mode === 'http_catalog' && (
                    <>
                      <div>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          Catalog endpoint URL — returns either LiteLLM-style JSON or a
                          {' '}
                          <Typography.Text code>{'{ "<id>": { vision, recommended } }'}</Typography.Text>
                          {' '}map
                        </Typography.Text>
                        <Input
                          value={cfg.endpoint}
                          onChange={(e) => setCfg({ ...cfg, endpoint: e.target.value })}
                          placeholder="https://your-host/model-capabilities.json"
                          disabled={!cfg.enabled}
                          style={{ marginTop: 2 }}
                        />
                      </div>
                      <div>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          Bearer token (optional)
                        </Typography.Text>
                        <Input.Password
                          value={cfg.auth}
                          onChange={(e) => setCfg({ ...cfg, auth: e.target.value })}
                          placeholder="(none)"
                          disabled={!cfg.enabled}
                          style={{ marginTop: 2 }}
                        />
                      </div>
                    </>
                  )}

                  {cfg.mode === 'mcp' && (
                    <>
                      <Alert
                        type="info"
                        showIcon
                        message="Best-effort MCP discovery"
                        description="Requires MCP enabled ([mcp].enabled). The configured tool is called with no arguments and its result is parsed for model capabilities; on any error the built-in floor is used."
                      />
                      <div>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          MCP server URL (SSE / streamable HTTP)
                        </Typography.Text>
                        <Input
                          value={cfg.endpoint}
                          onChange={(e) => setCfg({ ...cfg, endpoint: e.target.value })}
                          placeholder="https://your-mcp-server/sse"
                          disabled={!cfg.enabled}
                          style={{ marginTop: 2 }}
                        />
                      </div>
                      <div>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          Tool name (default: list_models)
                        </Typography.Text>
                        <Input
                          value={cfg.tool}
                          onChange={(e) => setCfg({ ...cfg, tool: e.target.value })}
                          placeholder="list_models"
                          disabled={!cfg.enabled}
                          style={{ marginTop: 2 }}
                        />
                      </div>
                      <div>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          Bearer token (optional)
                        </Typography.Text>
                        <Input.Password
                          value={cfg.auth}
                          onChange={(e) => setCfg({ ...cfg, auth: e.target.value })}
                          placeholder="(none)"
                          disabled={!cfg.enabled}
                          style={{ marginTop: 2 }}
                        />
                      </div>
                    </>
                  )}

                  <Space>
                    <Button type="primary" icon={<SaveOutlined />} loading={saving} onClick={handleSave}>
                      Save
                    </Button>
                    <Button
                      icon={<ReloadOutlined />}
                      loading={refreshing}
                      onClick={handleRefresh}
                      disabled={!cfg.enabled}
                    >
                      Refresh now
                    </Button>
                  </Space>
                </>
              )}
            </Space>
          ),
        },
      ]}
    />
  );
}

export function ProvidersTab({ scope }: { scope?: SettingsScopeParam }) {
  const config = useProviderConfig();
  const models = useAvailableModels();
  const updateMut = useUpdateProviderConfig();
  const [editing, setEditing] = useState(false);

  if (config.isLoading) return <Spin />;
  if (config.isError) return <Alert type="error" message="Failed to load provider config" />;

  const p = config.data!;

  const handleSave = async (values: Record<string, unknown>) => {
    try {
      await updateMut.mutateAsync(values);
      message.success('Provider config updated and applied');
      setEditing(false);
    } catch (err: unknown) {
      const msg =
        err && typeof err === 'object' && 'response' in err
          ? (err as { response: { data?: { error?: { message?: string } } } }).response?.data
              ?.error?.message
          : undefined;
      message.error(msg || 'Failed to update provider config');
    }
  };

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      {!editing && (
        <div>
          <Button type="primary" icon={<EditOutlined />} onClick={() => setEditing(true)}>
            Edit Provider Config
          </Button>
        </div>
      )}

      {editing ? (
        <EditForm
          config={p}
          availableModels={models.data?.models || []}
          onSave={handleSave}
          onCancel={() => setEditing(false)}
          saving={updateMut.isPending}
        />
      ) : (
        <ReadOnlyView config={p} />
      )}

      <AvailableModelsPanel config={p} llmModels={models} onRefreshLlm={() => models.refetch()} />

      <RecommendationsCard />

      <ChatPipelineCard scope={scope} />
    </Space>
  );
}
