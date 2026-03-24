import { useEffect, useState } from 'react';
import {
  Card, Button, Tag, Space, Typography, Table, Popconfirm, message,
  Alert, Spin, Badge, Tooltip, Modal, Input, Divider, Collapse,
} from 'antd';
import {
  RocketOutlined, CheckCircleOutlined, CloseCircleOutlined,
  DownloadOutlined, ThunderboltOutlined, CloudDownloadOutlined,
  MessageOutlined, FileTextOutlined, CloudOutlined, DesktopOutlined,
  DollarOutlined, ClockCircleOutlined, ApiOutlined, AppstoreOutlined,
} from '@ant-design/icons';
import { listPresets, applyPreset, listOllamaModels, pullOllamaModel } from '../../api/settings';
import type { PresetInfo, PresetModelInfo, AvailableModel, SettingsSummaryItem } from '../../api/types';

const { Text } = Typography;

const WEIGHT_COLORS: Record<string, string> = {
  heavy: 'red',
  medium: 'orange',
  light: 'green',
};

function CostTag({ cost }: { cost: string }) {
  const color = cost === 'Free' ? 'green' : cost.includes('0.0') ? 'blue' : 'orange';
  return <Tag color={color}><DollarOutlined /> {cost}</Tag>;
}

function LatencyTag({ latency }: { latency: string }) {
  const isfast = latency.match(/^[0-9]-/) && !latency.includes('min');
  const color = isfast ? 'green' : latency.includes('min') ? 'red' : 'orange';
  return <Tag color={color}><ClockCircleOutlined /> {latency}</Tag>;
}

function CallsTag({ calls }: { calls: string }) {
  return <Tag><ApiOutlined /> {calls}</Tag>;
}

function FeatureTags({ features }: { features: string[] }) {
  if (features.length === 0) {
    return <Tag color="default"><AppstoreOutlined /> No extra features</Tag>;
  }
  return (
    <>
      <Tag color={features.length <= 5 ? 'blue' : 'purple'}>
        <AppstoreOutlined /> {features.length} feature{features.length !== 1 ? 's' : ''}
      </Tag>
      {features.map(f => (
        <Tag key={f} style={{ fontSize: 11 }}>{f}</Tag>
      ))}
    </>
  );
}

export function PresetsCard() {
  const [presets, setPresets] = useState<PresetInfo[]>([]);
  const [ollamaModels, setOllamaModels] = useState<AvailableModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState<string | null>(null);
  const [pulling, setPulling] = useState<Set<string>>(new Set());
  const [ollamaUrl, setOllamaUrl] = useState('http://host.docker.internal:11435');
  const [cloudApiKey, setCloudApiKey] = useState('');
  const [showUrlModal, setShowUrlModal] = useState(false);
  const [showCloudModal, setShowCloudModal] = useState(false);
  const [pendingPreset, setPendingPreset] = useState<string | null>(null);

  async function loadData() {
    setLoading(true);
    try {
      const [p, m] = await Promise.all([listPresets(), listOllamaModels()]);
      setPresets(p);
      setOllamaModels(m.models || []);
    } catch {
      message.error('Failed to load presets');
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { loadData(); }, []);

  function isModelAvailable(model: string): boolean {
    return ollamaModels.some(m =>
      m.id === model || m.name === model ||
      m.id.startsWith(model.replace(':latest', ':'))
    );
  }

  function getAvailableCount(preset: PresetInfo): { available: number; total: number } {
    const total = preset.required_models.length;
    const available = preset.required_models.filter(m => isModelAvailable(m.model)).length;
    return { available, total };
  }

  async function handlePull(model: string) {
    setPulling(prev => new Set(prev).add(model));
    try {
      await pullOllamaModel(model, ollamaUrl);
      message.success(`Pulling ${model}... This may take a while. Refresh to check status.`);
      const interval = setInterval(async () => {
        try {
          const m = await listOllamaModels();
          setOllamaModels(m.models || []);
          if (m.models?.some(om => om.id === model || om.name === model || om.id.startsWith(model.replace(':latest', ':')))) {
            setPulling(prev => { const n = new Set(prev); n.delete(model); return n; });
            clearInterval(interval);
            message.success(`${model} downloaded successfully!`);
          }
        } catch { /* ignore */ }
      }, 10000);
      setTimeout(() => clearInterval(interval), 1800000);
    } catch {
      message.error(`Failed to pull ${model}`);
      setPulling(prev => { const n = new Set(prev); n.delete(model); return n; });
    }
  }

  async function handlePullAll(preset: PresetInfo) {
    const missing = preset.required_models.filter(m => !isModelAvailable(m.model));
    for (const m of missing) {
      await handlePull(m.model);
    }
  }

  async function handleApply(presetId: string, isCloud: boolean) {
    setApplying(presetId);
    try {
      await applyPreset(presetId, ollamaUrl, isCloud ? cloudApiKey : undefined);
      message.success('Preset applied! Switch to other tabs to see updated settings.');
    } catch (err) {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Failed to apply preset';
      message.error(msg);
    } finally {
      setApplying(null);
    }
  }

  function confirmApply(preset: PresetInfo) {
    setPendingPreset(preset.id);
    if (preset.provider_type === 'cloud') {
      setShowCloudModal(true);
    } else {
      setShowUrlModal(true);
    }
  }

  if (loading) return <Spin />;

  const chatPresets = presets.filter(p => p.category === 'chat');
  const docPresets = presets.filter(p => p.category === 'document');

  const ollamaChatPresets = chatPresets.filter(p => p.provider_type === 'ollama');
  const cloudChatPresets = chatPresets.filter(p => p.provider_type === 'cloud');
  const ollamaDocPresets = docPresets.filter(p => p.provider_type === 'ollama');
  const cloudDocPresets = docPresets.filter(p => p.provider_type === 'cloud');

  function renderPresetCard(preset: PresetInfo) {
    const isCloud = preset.provider_type === 'cloud';
    const { available, total } = getAvailableCount(preset);
    const allReady = isCloud || available === total;

    return (
      <Card
        key={preset.id}
        size="small"
        title={
          <Space wrap>
            <Text strong>{preset.name}</Text>
            {!isCloud && (
              <Badge
                count={`${available}/${total} models`}
                style={{ backgroundColor: allReady ? '#52c41a' : '#faad14' }}
              />
            )}
          </Space>
        }
        extra={
          <Space>
            {!isCloud && !allReady && (
              <Tooltip title="Pull all missing models">
                <Button
                  size="small"
                  icon={<CloudDownloadOutlined />}
                  onClick={() => handlePullAll(preset)}
                  loading={preset.required_models.some(m => pulling.has(m.model))}
                >
                  Pull Missing
                </Button>
              </Tooltip>
            )}
            <Popconfirm
              title="Apply this preset?"
              description={`This will override your current ${preset.category === 'chat' ? 'chat pipeline' : 'document processing'} settings.`}
              onConfirm={() => confirmApply(preset)}
            >
              <Button
                type="primary"
                size="small"
                icon={<ThunderboltOutlined />}
                loading={applying === preset.id}
                disabled={!allReady}
              >
                Apply
              </Button>
            </Popconfirm>
          </Space>
        }
      >
        <Typography.Paragraph type="secondary" style={{ marginBottom: 8 }}>
          {preset.description}
        </Typography.Paragraph>

        <div style={{ marginBottom: 8, display: 'flex', flexWrap: 'wrap', gap: 4 }}>
          <CostTag cost={preset.estimated_cost_per_query} />
          <LatencyTag latency={preset.estimated_latency} />
          <CallsTag calls={preset.llm_calls_per_query} />
          <FeatureTags features={preset.features || []} />
        </div>

        <Table<PresetModelInfo>
          dataSource={preset.required_models}
          rowKey="model"
          size="small"
          pagination={false}
          columns={[
            ...(isCloud ? [] : [{
              title: 'Status',
              width: 60,
              render: (_: unknown, r: PresetModelInfo) => isModelAvailable(r.model)
                ? <CheckCircleOutlined style={{ color: '#52c41a', fontSize: 16 }} />
                : pulling.has(r.model)
                  ? <Spin size="small" />
                  : <CloseCircleOutlined style={{ color: '#ff4d4f', fontSize: 16 }} />,
            }]),
            {
              title: 'Model',
              dataIndex: 'model',
              render: (v: string) => <Text code>{v}</Text>,
            },
            {
              title: 'Role',
              dataIndex: 'role',
            },
            {
              title: 'Weight',
              dataIndex: 'task_weight',
              width: 80,
              render: (v: string) => <Tag color={WEIGHT_COLORS[v] || 'default'}>{v}</Tag>,
            },
            {
              title: 'Description',
              dataIndex: 'description',
              responsive: ['md'] as const,
            },
            ...(isCloud ? [] : [{
              title: '',
              width: 80,
              render: (_: unknown, r: PresetModelInfo) => !isModelAvailable(r.model) && (
                <Button
                  size="small"
                  icon={<DownloadOutlined />}
                  onClick={() => handlePull(r.model)}
                  loading={pulling.has(r.model)}
                >
                  Pull
                </Button>
              ),
            }]),
          ]}
        />

        {preset.settings_summary?.length > 0 && (
          <div style={{ marginTop: 12, display: 'flex', flexWrap: 'wrap', gap: '4px 12px' }}>
            {preset.settings_summary.map((item: SettingsSummaryItem) => (
              <Text key={item.label} style={{ fontSize: 12 }} type="secondary">
                <strong>{item.label}:</strong> {item.value}
              </Text>
            ))}
          </div>
        )}
      </Card>
    );
  }

  function renderProviderSection(
    label: string,
    icon: React.ReactNode,
    ollamaPresets: PresetInfo[],
    cloudPresets: PresetInfo[],
  ) {
    return (
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        {ollamaPresets.length > 0 && (
          <>
            <Divider orientation="left" style={{ margin: '4px 0' }}>
              <Space><DesktopOutlined /> Local (Ollama) — Free, runs on your GPU</Space>
            </Divider>
            {ollamaPresets.map(renderPresetCard)}
          </>
        )}
        {cloudPresets.length > 0 && (
          <>
            <Divider orientation="left" style={{ margin: '4px 0' }}>
              <Space><CloudOutlined /> Cloud (API Key) — Fast, pay per query</Space>
            </Divider>
            {cloudPresets.map(renderPresetCard)}
          </>
        )}
      </Space>
    );
  }

  return (
    <Card
      title={<><RocketOutlined /> Quick Setup Presets</>}
      style={{ marginBottom: 24 }}
      extra={
        <Button size="small" onClick={loadData}>Refresh Models</Button>
      }
    >
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Local presets run free on your GPU (Ollama) but are slower with many features. Cloud presets use API keys (OpenAI) for faster responses at a per-query cost."
        description="Pick one preset from each section. Chat and Document presets are independent."
      />

      <Collapse
        defaultActiveKey={['chat-presets', 'doc-presets']}
        items={[
          {
            key: 'chat-presets',
            label: <Space><MessageOutlined /> Chat & Response Pipeline</Space>,
            children: renderProviderSection('Chat', <MessageOutlined />, ollamaChatPresets, cloudChatPresets),
          },
          {
            key: 'doc-presets',
            label: <Space><FileTextOutlined /> Document Processing</Space>,
            children: renderProviderSection('Document', <FileTextOutlined />, ollamaDocPresets, cloudDocPresets),
          },
        ]}
      />

      {/* Ollama URL modal (for local presets) */}
      <Modal
        title="Ollama Connection"
        open={showUrlModal}
        onOk={() => {
          setShowUrlModal(false);
          if (pendingPreset) handleApply(pendingPreset, false);
          setPendingPreset(null);
        }}
        onCancel={() => { setShowUrlModal(false); setPendingPreset(null); }}
        okText="Apply Preset"
      >
        <Typography.Paragraph>
          Confirm the Ollama URL that ThaiRAG (inside Docker) should use to connect:
        </Typography.Paragraph>
        <Input
          value={ollamaUrl}
          onChange={e => setOllamaUrl(e.target.value)}
          placeholder="http://host.docker.internal:11435"
        />
        <Typography.Paragraph type="secondary" style={{ marginTop: 8, fontSize: 12 }}>
          <strong>Docker Desktop (Mac/Windows):</strong> http://host.docker.internal:11435<br />
          <strong>Linux:</strong> http://172.17.0.1:11434 or http://host.docker.internal:11435<br />
          <strong>Same machine:</strong> http://localhost:11434
        </Typography.Paragraph>
      </Modal>

      {/* API Key modal (for cloud presets) */}
      <Modal
        title="Cloud API Key"
        open={showCloudModal}
        onOk={() => {
          if (!cloudApiKey.trim()) {
            message.warning('Please enter an API key');
            return;
          }
          setShowCloudModal(false);
          if (pendingPreset) handleApply(pendingPreset, true);
          setPendingPreset(null);
        }}
        onCancel={() => { setShowCloudModal(false); setPendingPreset(null); }}
        okText="Apply Preset"
      >
        <Typography.Paragraph>
          Enter your OpenAI API key. This will be stored in ThaiRAG settings and used for all cloud LLM calls.
        </Typography.Paragraph>
        <Input.Password
          value={cloudApiKey}
          onChange={e => setCloudApiKey(e.target.value)}
          placeholder="sk-..."
          style={{ width: '100%' }}
        />
        <Typography.Paragraph type="secondary" style={{ marginTop: 8, fontSize: 12 }}>
          Get your API key from <a href="https://platform.openai.com/api-keys" target="_blank" rel="noopener noreferrer">platform.openai.com/api-keys</a>. Embedding still uses local FastEmbed (free) — only LLM calls use the API key.
        </Typography.Paragraph>
      </Modal>
    </Card>
  );
}
