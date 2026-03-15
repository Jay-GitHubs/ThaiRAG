import { useState, useEffect } from 'react';
import {
  Card,
  Descriptions,
  Tag,
  Table,
  Spin,
  Alert,
  Button,
  Space,
  Typography,
  Form,
  Select,
  Input,
  message,
  Divider,
} from 'antd';
import { EditOutlined, ReloadOutlined, SaveOutlined, CloseOutlined, SyncOutlined } from '@ant-design/icons';
import {
  useProviderConfig,
  useAvailableModels,
  useUpdateProviderConfig,
} from '../../hooks/useSettings';
import { syncModels, syncRerankerModels } from '../../api/settings';
import type { AvailableModel, ProviderConfigResponse } from '../../api/types';
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

function ReadOnlyView({ config }: { config: ProviderConfigResponse }) {
  const p = config;
  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Card title="LLM Provider" size="small">
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
      </Card>

      <Card title="Reranker" size="small">
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
      </Card>
    </Space>
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

  useEffect(() => {
    form.setFieldsValue({
      llm_kind: config.llm.kind,
      llm_model: config.llm.model,
      llm_base_url: config.llm.base_url || '',
      llm_api_key: '',
      rr_kind: config.reranker.kind,
      rr_model: config.reranker.model || '',
      rr_api_key: '',
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

  const toOptions = (models: AvailableModel[]) =>
    models.map((m) => ({
      label: m.size ? `${m.name} (${formatBytes(m.size)})` : m.name,
      value: m.id,
    }));

  // Build model options — prefer synced, then API-fetched, then static
  const getModelOptions = () => {
    if (syncedModels && syncedModels.length > 0) {
      return toOptions(syncedModels);
    }
    if (llmKind === config.llm.kind && availableModels.length > 0) {
      return toOptions(availableModels);
    }
    return staticModels[llmKind] || [];
  };
  const modelOptions = getModelOptions();
  const useModelSelect = modelOptions.length > 0;

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

  // Reset synced models and model selection when switching provider kind
  useEffect(() => {
    setSyncedModels(null);
    if (llmKind !== config.llm.kind) {
      form.setFieldValue('llm_model', undefined);
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

  const rrModelOptions = (() => {
    if (syncedRrModels && syncedRrModels.length > 0) {
      return toOptions(syncedRrModels);
    }
    return staticRrModels[rrKind] || [];
  })();
  const useRrModelSelect = rrModelOptions.length > 0;

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
          <Form.Item label="Model" required style={{ marginBottom: 0 }}>
            <Space.Compact style={{ width: '100%' }}>
              <Form.Item name="llm_model" noStyle rules={[{ required: true, message: 'Model is required' }]}>
                {useModelSelect ? (
                  <Select showSearch optionFilterProp="label" options={modelOptions} allowClear={false} placeholder="Select a model" style={{ width: '100%' }} />
                ) : (
                  <Input placeholder={
                    llmKind === 'OpenAiCompatible' ? 'e.g. deepseek-chat, mistral-large-latest' :
                    'Enter model name'
                  } style={{ width: '100%' }} />
                )}
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
              <Input placeholder="http://localhost:11434" />
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
                  {useRrModelSelect ? (
                    <Select showSearch optionFilterProp="label" options={rrModelOptions} allowClear={false} placeholder="Select a reranker model" style={{ width: '100%' }} />
                  ) : (
                    <Input placeholder="Enter model name" style={{ width: '100%' }} />
                  )}
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

export function ProvidersTab() {
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

  const modelColumns = [
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

      <Card
        title="Available LLM Models"
        size="small"
        extra={
          <Button
            icon={<ReloadOutlined />}
            size="small"
            onClick={() => models.refetch()}
            loading={models.isFetching}
          >
            Refresh
          </Button>
        }
      >
        {models.data && (
          <Typography.Text type="secondary" style={{ marginBottom: 8, display: 'block' }}>
            Source: <Tag>{models.data.provider}</Tag>
            {models.data.models.length} model(s) available
          </Typography.Text>
        )}
        <Table<AvailableModel>
          rowKey="id"
          columns={modelColumns}
          dataSource={models.data?.models}
          loading={models.isLoading}
          pagination={false}
          size="small"
        />
      </Card>

      <ChatPipelineCard />
    </Space>
  );
}
