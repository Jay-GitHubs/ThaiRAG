import { useEffect, useState } from 'react';
import {
  Card,
  Button,
  Typography,
  Space,
  Select,
  Input,
  InputNumber,
  Tag,
  Tooltip,
  Alert,
  Spin,
  message,
} from 'antd';
import {
  SettingOutlined,
  SaveOutlined,
  SyncOutlined,
  QuestionCircleOutlined,
} from '@ant-design/icons';
import { theme } from 'antd';
import { getProviderConfig, updateProviderConfig, syncEmbeddingModels } from '../../api/settings';
import type { AvailableModel, ProviderConfigResponse } from '../../api/types';

const { Text } = Typography;

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
  'BAAI/bge-small-en-v1.5': 384,
  'BAAI/bge-base-en-v1.5': 768,
  'BAAI/bge-large-en-v1.5': 1024,
  'sentence-transformers/all-MiniLM-L6-v2': 384,
  'sentence-transformers/all-MiniLM-L12-v2': 384,
  'sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2': 384,
  'jinaai/jina-embeddings-v2-small-en': 512,
  'jinaai/jina-embeddings-v2-base-en': 768,
  'text-embedding-3-small': 1536,
  'text-embedding-3-large': 3072,
  'text-embedding-ada-002': 1536,
  'embed-v4.0': 1024,
  'embed-english-v3.0': 1024,
  'embed-multilingual-v3.0': 1024,
  'embed-english-light-v3.0': 384,
  'embed-multilingual-light-v3.0': 384,
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
  if (knownEmbDimensions[model + ':latest']) return knownEmbDimensions[model + ':latest'];
  const base = model.replace(/:.*$/, '');
  if (knownEmbDimensions[base + ':latest']) return knownEmbDimensions[base + ':latest'];
  return undefined;
}

const embKindColors: Record<string, string> = {
  Fastembed: 'cyan', OpenAi: 'green', Ollama: 'blue', Cohere: 'magenta',
};

function formatModelSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/**
 * Global embedding-model editor. Embedding is shared across the whole platform
 * (chat-time query embedding + document-ingestion chunk embedding), so it lives
 * in the Shared / Common tab and is not scope-aware. Changing it invalidates all
 * existing vectors, hence the re-index warning.
 */
export function EmbeddingEditor() {
  const { token } = theme.useToken();
  const [providerConfig, setProviderConfig] = useState<ProviderConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const [embKind, setEmbKind] = useState('Fastembed');
  const [embModel, setEmbModel] = useState('');
  const [embDimension, setEmbDimension] = useState(384);
  const [embBaseUrl, setEmbBaseUrl] = useState('');
  const [embApiKey, setEmbApiKey] = useState('');
  const [syncedEmbModels, setSyncedEmbModels] = useState<AvailableModel[] | null>(null);
  const [syncingEmb, setSyncingEmb] = useState(false);

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
    } catch {
      message.error('Failed to load embedding config');
    } finally {
      setLoading(false);
    }
  }

  const dirty =
    !!providerConfig &&
    (embKind !== providerConfig.embedding.kind ||
      embModel !== providerConfig.embedding.model ||
      embDimension !== providerConfig.embedding.dimension);

  async function handleSave() {
    setSaving(true);
    try {
      const emb: Record<string, unknown> = {};
      if (embKind !== providerConfig?.embedding.kind) emb.kind = embKind;
      if (embModel !== providerConfig?.embedding.model) emb.model = embModel;
      if (embDimension !== providerConfig?.embedding.dimension) emb.dimension = embDimension;
      if (embBaseUrl !== (providerConfig?.embedding.base_url || '')) emb.base_url = embBaseUrl;
      if (embApiKey) emb.api_key = embApiKey;

      if (Object.keys(emb).length === 0) {
        message.info('No changes to save');
        setSaving(false);
        return;
      }

      const updated = await updateProviderConfig({ embedding: emb });
      setProviderConfig(updated);
      setEmbKind(updated.embedding.kind);
      setEmbModel(updated.embedding.model);
      setEmbDimension(updated.embedding.dimension);
      setEmbBaseUrl(updated.embedding.base_url || '');
      setEmbApiKey('');
      message.success('Embedding settings saved');
    } catch {
      message.error('Failed to save embedding settings');
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

  if (loading) return <Spin tip="Loading embedding config..." />;

  return (
    <Card
      title={
        <Space>
          <SettingOutlined />
          <span>Embedding Model</span>
          <Tooltip title="The embedding model converts text into numerical vectors for similarity search. It is shared platform-wide: both chat-time query embedding and document-ingestion chunk embedding use it, so it must stay consistent across every path.">
            <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
          </Tooltip>
          {providerConfig && (
            <Tag color={embKindColors[providerConfig.embedding.kind] || 'default'}>
              {providerConfig.embedding.kind}
            </Tag>
          )}
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
        <Alert
          type="info"
          showIcon
          style={{ fontSize: 12 }}
          message="Shared across the whole platform"
          description="Changing the embedding model or dimension invalidates every stored vector. All existing documents must be re-indexed before search returns correct results again."
        />

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
          <div>
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
          <div>
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

        {dirty && (
          <Alert
            type="warning"
            showIcon
            style={{ fontSize: 12 }}
            message="Re-indexing required after saving"
            description="You changed the embedding provider, model, or dimension. Existing vectors were produced by the previous model and will no longer match. Re-index all documents after saving."
          />
        )}

        <div style={{
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
      </Space>
    </Card>
  );
}
