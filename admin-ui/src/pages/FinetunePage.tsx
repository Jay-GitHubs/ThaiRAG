import { useState, useEffect, useCallback, useRef } from 'react';
import {
  Card,
  Table,
  Button,
  Space,
  Typography,
  Modal,
  Form,
  Input,
  message,
  Popconfirm,
  Tabs,
  Tag,
  Select,
  Drawer,
  Radio,
  Slider,
  Dropdown,
  Progress,
  Collapse,
  InputNumber,
  Descriptions,
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  ExperimentOutlined,
  ReloadOutlined,
  ImportOutlined,
  DownloadOutlined,
  PlayCircleOutlined,
  StopOutlined,
  FileTextOutlined,
  CopyOutlined,
} from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import {
  listDatasets,
  createDataset,
  deleteDataset,
  listPairs,
  addPair,
  deletePair,
  listJobs,
  createJob,
  startJob,
  cancelJob,
  getJobLogs,
  deleteJob,
  importFeedback,
  getExportUrl,
  listOllamaModels,
} from '../api/finetune';
import type {
  TrainingDataset,
  TrainingPair,
  FinetuneJob,
  TrainingConfig,
  ImportFeedbackRequest,
  OllamaModel,
} from '../api/finetune';
import { getToken } from '../api/client';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

// ── Status Tag ───────────────────────────────────────────────────────

function StatusTag({ status }: { status: string }) {
  const colorMap: Record<string, string> = {
    pending: 'gold',
    running: 'blue',
    completed: 'green',
    failed: 'red',
    cancelled: 'default',
  };
  return <Tag color={colorMap[status] ?? 'default'}>{status}</Tag>;
}

// ── Preset Descriptions ─────────────────────────────────────────────

const presets: { key: string; label: string; desc: string }[] = [
  { key: 'quick', label: 'Quick', desc: '1 epoch, higher LR, rank 8 — fast iteration' },
  { key: 'standard', label: 'Standard', desc: '3 epochs, balanced settings — recommended' },
  { key: 'thorough', label: 'Thorough', desc: '5 epochs, lower LR, rank 32 — best quality' },
];

// ── Parse Metrics ────────────────────────────────────────────────────

interface ProgressMetrics {
  step?: number;
  total_steps?: number;
  loss?: number;
  epoch?: number;
  lr?: number;
  output_path?: string;
  final_loss?: number;
  total_time_secs?: number;
  message?: string;
  type?: string;
}

function parseMetrics(metricsJson?: string): ProgressMetrics | null {
  if (!metricsJson) return null;
  try {
    return JSON.parse(metricsJson);
  } catch {
    return null;
  }
}

// ── Datasets Tab ─────────────────────────────────────────────────────

function DatasetsTab() {
  const [datasets, setDatasets] = useState<TrainingDataset[]>([]);
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [selectedDataset, setSelectedDataset] =
    useState<TrainingDataset | null>(null);
  const [pairs, setPairs] = useState<TrainingPair[]>([]);
  const [pairsLoading, setPairsLoading] = useState(false);
  const [addPairOpen, setAddPairOpen] = useState(false);
  const [addingPair, setAddingPair] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importSource, setImportSource] = useState<ImportFeedbackRequest['source']>('both');
  const [importMinScore, setImportMinScore] = useState<number>(0);
  const [createForm] = Form.useForm();
  const [pairForm] = Form.useForm();

  const loadDatasets = useCallback(async () => {
    setLoading(true);
    try {
      const res = await listDatasets();
      setDatasets(res.data);
    } catch {
      message.error('Failed to load datasets');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadDatasets();
  }, [loadDatasets]);

  const loadPairs = useCallback(async (datasetId: string) => {
    setPairsLoading(true);
    try {
      const res = await listPairs(datasetId);
      setPairs(res.data);
    } catch {
      message.error('Failed to load pairs');
    } finally {
      setPairsLoading(false);
    }
  }, []);

  const handleSelectDataset = (ds: TrainingDataset) => {
    setSelectedDataset(ds);
    loadPairs(ds.id);
  };

  const handleCreateDataset = async () => {
    try {
      const values = await createForm.validateFields();
      setCreating(true);
      await createDataset(values);
      message.success('Dataset created');
      setCreateOpen(false);
      createForm.resetFields();
      loadDatasets();
    } catch {
      message.error('Failed to create dataset');
    } finally {
      setCreating(false);
    }
  };

  const handleDeleteDataset = async (id: string) => {
    try {
      await deleteDataset(id);
      message.success('Dataset deleted');
      if (selectedDataset?.id === id) {
        setSelectedDataset(null);
        setPairs([]);
      }
      loadDatasets();
    } catch {
      message.error('Failed to delete dataset');
    }
  };

  const handleAddPair = async () => {
    if (!selectedDataset) return;
    try {
      const values = await pairForm.validateFields();
      setAddingPair(true);
      await addPair(selectedDataset.id, values);
      message.success('Training pair added');
      setAddPairOpen(false);
      pairForm.resetFields();
      loadPairs(selectedDataset.id);
      loadDatasets(); // to update pair_count
    } catch {
      message.error('Failed to add pair');
    } finally {
      setAddingPair(false);
    }
  };

  const handleDeletePair = async (pairId: string) => {
    if (!selectedDataset) return;
    try {
      await deletePair(selectedDataset.id, pairId);
      message.success('Pair deleted');
      loadPairs(selectedDataset.id);
      loadDatasets();
    } catch {
      message.error('Failed to delete pair');
    }
  };

  const handleImportFeedback = async () => {
    if (!selectedDataset) return;
    try {
      setImporting(true);
      const req: ImportFeedbackRequest = {
        source: importSource,
        min_score: importMinScore > 0 ? importMinScore : undefined,
      };
      const res = await importFeedback(selectedDataset.id, req);
      message.success(
        `Imported ${res.imported} pairs (${res.skipped_duplicates} duplicates skipped)`,
      );
      setImportOpen(false);
      loadPairs(selectedDataset.id);
      loadDatasets();
    } catch {
      message.error('Failed to import feedback');
    } finally {
      setImporting(false);
    }
  };

  const handleExport = (format: 'openai' | 'alpaca') => {
    if (!selectedDataset) return;
    const url = getExportUrl(selectedDataset.id, format);
    // Create a temporary link with auth header via fetch
    const token = getToken();
    fetch(url, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    })
      .then((res) => res.blob())
      .then((blob) => {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = `${selectedDataset.name.replace(/ /g, '_')}-${format}.jsonl`;
        a.click();
        URL.revokeObjectURL(a.href);
      })
      .catch(() => message.error('Export failed'));
  };

  const datasetColumns: ColumnsType<TrainingDataset> = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name, record) => (
        <Button type="link" onClick={() => handleSelectDataset(record)}>
          {name}
        </Button>
      ),
    },
    { title: 'Description', dataIndex: 'description', key: 'description', className: 'responsive-hide-sm' },
    { title: 'Pairs', dataIndex: 'pair_count', key: 'pair_count' },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      className: 'responsive-hide-sm',
      render: (v: string) => new Date(v).toLocaleDateString(),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_, record) => (
        <Popconfirm
          title="Delete this dataset and all its pairs?"
          onConfirm={() => handleDeleteDataset(record.id)}
        >
          <Button danger icon={<DeleteOutlined />} size="small" />
        </Popconfirm>
      ),
    },
  ];

  const pairColumns: ColumnsType<TrainingPair> = [
    {
      title: 'Query',
      dataIndex: 'query',
      key: 'query',
      ellipsis: true,
    },
    {
      title: 'Positive Doc',
      dataIndex: 'positive_doc',
      key: 'positive_doc',
      ellipsis: true,
    },
    {
      title: 'Negative Doc',
      dataIndex: 'negative_doc',
      key: 'negative_doc',
      ellipsis: true,
      className: 'responsive-hide-xs',
      render: (v?: string) => v ?? <Text type="secondary">—</Text>,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      className: 'responsive-hide-xs',
      render: (v: string) => new Date(v).toLocaleDateString(),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_, record) => (
        <Popconfirm
          title="Delete this training pair?"
          onConfirm={() => handleDeletePair(record.id)}
        >
          <Button danger icon={<DeleteOutlined />} size="small" />
        </Popconfirm>
      ),
    },
  ];

  return (
    <>
      <Card
        title="Training Datasets"
        extra={
          <Space>
            <Button icon={<ReloadOutlined />} onClick={loadDatasets}>
              Refresh
            </Button>
            <Button
              type="primary"
              icon={<PlusOutlined />}
              onClick={() => setCreateOpen(true)}
            >
              Create Dataset
            </Button>
          </Space>
        }
      >
        <Table
          dataSource={datasets}
          columns={datasetColumns}
          rowKey="id"
          loading={loading}
          pagination={{ pageSize: 10 }}
          size="small"
        />
      </Card>

      {selectedDataset && (
        <Card
          style={{ marginTop: 16 }}
          title={
            <Space>
              <span>Pairs in: {selectedDataset.name}</span>
              <Tag>{selectedDataset.pair_count} pairs</Tag>
            </Space>
          }
          extra={
            <Space>
              <Dropdown
                menu={{
                  items: [
                    {
                      key: 'openai',
                      label: 'Export OpenAI JSONL',
                      onClick: () => handleExport('openai'),
                    },
                    {
                      key: 'alpaca',
                      label: 'Export Alpaca JSONL',
                      onClick: () => handleExport('alpaca'),
                    },
                  ],
                }}
                disabled={selectedDataset.pair_count === 0}
              >
                <Button icon={<DownloadOutlined />}>Export</Button>
              </Dropdown>
              <Button
                icon={<ImportOutlined />}
                onClick={() => setImportOpen(true)}
              >
                Import from Feedback
              </Button>
              <Button
                type="primary"
                icon={<PlusOutlined />}
                onClick={() => setAddPairOpen(true)}
              >
                Add Pair
              </Button>
            </Space>
          }
        >
          <Table
            dataSource={pairs}
            columns={pairColumns}
            rowKey="id"
            loading={pairsLoading}
            pagination={{ pageSize: 10 }}
            size="small"
          />
        </Card>
      )}

      {/* Create Dataset Modal */}
      <Modal
        title="Create Training Dataset"
        open={createOpen}
        onOk={handleCreateDataset}
        onCancel={() => {
          setCreateOpen(false);
          createForm.resetFields();
        }}
        confirmLoading={creating}
      >
        <Form form={createForm} layout="vertical">
          <Form.Item
            name="name"
            label="Name"
            rules={[{ required: true, message: 'Name is required' }]}
          >
            <Input placeholder="e.g. Thai Legal QA v1" />
          </Form.Item>
          <Form.Item name="description" label="Description">
            <TextArea rows={3} placeholder="Describe the purpose of this dataset" />
          </Form.Item>
        </Form>
      </Modal>

      {/* Import Feedback Modal */}
      <Modal
        title="Import from Feedback"
        open={importOpen}
        onOk={handleImportFeedback}
        onCancel={() => setImportOpen(false)}
        confirmLoading={importing}
        okText="Import"
      >
        <Space direction="vertical" style={{ width: '100%' }} size="middle">
          <div>
            <Typography.Text strong>Source</Typography.Text>
            <Radio.Group
              style={{ display: 'block', marginTop: 8 }}
              value={importSource}
              onChange={(e) => setImportSource(e.target.value)}
            >
              <Radio value="positive_feedback">Positive Feedback</Radio>
              <Radio value="golden_examples">Golden Examples</Radio>
              <Radio value="both">Both</Radio>
            </Radio.Group>
          </div>
          <div>
            <Typography.Text strong>
              Min Chunk Score (0 = no filter)
            </Typography.Text>
            <Slider
              min={0}
              max={1}
              step={0.05}
              value={importMinScore}
              onChange={setImportMinScore}
              marks={{ 0: '0', 0.5: '0.5', 1: '1.0' }}
            />
          </div>
        </Space>
      </Modal>

      {/* Add Pair Drawer */}
      <Drawer
        title="Add Training Pair"
        open={addPairOpen}
        onClose={() => {
          setAddPairOpen(false);
          pairForm.resetFields();
        }}
        width={560}
        footer={
          <Space>
            <Button
              onClick={() => {
                setAddPairOpen(false);
                pairForm.resetFields();
              }}
            >
              Cancel
            </Button>
            <Button type="primary" loading={addingPair} onClick={handleAddPair}>
              Add Pair
            </Button>
          </Space>
        }
      >
        <Form form={pairForm} layout="vertical">
          <Form.Item
            name="query"
            label="Query"
            rules={[{ required: true, message: 'Query is required' }]}
          >
            <TextArea rows={3} placeholder="The user query" />
          </Form.Item>
          <Form.Item
            name="positive_doc"
            label="Positive Document"
            rules={[{ required: true, message: 'Positive document is required' }]}
          >
            <TextArea rows={4} placeholder="Relevant document text" />
          </Form.Item>
          <Form.Item name="negative_doc" label="Negative Document (optional)">
            <TextArea rows={4} placeholder="Non-relevant document text (hard negative)" />
          </Form.Item>
        </Form>
      </Drawer>
    </>
  );
}

// ── Jobs Tab ─────────────────────────────────────────────────────────

function JobsTab() {
  const [jobs, setJobs] = useState<FinetuneJob[]>([]);
  const [datasets, setDatasets] = useState<TrainingDataset[]>([]);
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [modelSource, setModelSource] = useState<'ollama' | 'huggingface'>('ollama');
  const [selectedPreset, setSelectedPreset] = useState<string>('standard');
  const [logsDrawerOpen, setLogsDrawerOpen] = useState(false);
  const [logsJobId, setLogsJobId] = useState<string>('');
  const [logLines, setLogLines] = useState<string[]>([]);
  const [form] = Form.useForm();
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const [jobsRes, datasetsRes] = await Promise.all([
        listJobs(),
        listDatasets(),
      ]);
      setJobs(jobsRes.data);
      setDatasets(datasetsRes.data);
    } catch {
      message.error('Failed to load jobs');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadJobs();
    listOllamaModels().then(setOllamaModels);
  }, [loadJobs]);

  // Auto-refresh while any job is running
  useEffect(() => {
    const hasRunning = jobs.some((j) => j.status === 'running');
    if (hasRunning && !pollRef.current) {
      pollRef.current = setInterval(() => {
        loadJobs();
      }, 5000);
    } else if (!hasRunning && pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [jobs, loadJobs]);

  const handleCreateJob = async () => {
    try {
      const values = await form.validateFields();
      setCreating(true);
      const config: TrainingConfig = {
        epochs: values.epochs ?? 3,
        learning_rate: values.learning_rate ?? 2e-4,
        lora_rank: values.lora_rank ?? 16,
        lora_alpha: values.lora_alpha ?? 16,
        batch_size: values.batch_size ?? 2,
        warmup_ratio: values.warmup_ratio ?? 0.03,
        max_seq_length: values.max_seq_length ?? 2048,
        quantization: values.quantization ?? 'q4_k_m',
        preset: selectedPreset,
      };
      await createJob({
        dataset_id: values.dataset_id,
        base_model: values.base_model,
        model_source: modelSource,
        config,
      });
      message.success('Fine-tuning job created');
      setCreateOpen(false);
      form.resetFields();
      loadJobs();
    } catch {
      message.error('Failed to create job');
    } finally {
      setCreating(false);
    }
  };

  const handleStart = async (id: string) => {
    try {
      await startJob(id);
      message.success('Training started');
      loadJobs();
    } catch {
      message.error('Failed to start job');
    }
  };

  const handleCancel = async (id: string) => {
    try {
      await cancelJob(id);
      message.success('Training cancelled');
      loadJobs();
    } catch {
      message.error('Failed to cancel job');
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteJob(id);
      message.success('Job deleted');
      loadJobs();
    } catch {
      message.error('Failed to delete job');
    }
  };

  const handleViewLogs = async (id: string) => {
    setLogsJobId(id);
    setLogsDrawerOpen(true);
    try {
      const res = await getJobLogs(id);
      setLogLines(res.lines);
    } catch {
      setLogLines(['Failed to load logs']);
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    message.success('Copied to clipboard');
  };

  // Apply preset defaults when preset changes
  useEffect(() => {
    const presetDefaults: Record<string, Partial<TrainingConfig>> = {
      quick: { epochs: 1, learning_rate: 5e-4, lora_rank: 8, lora_alpha: 8, batch_size: 4 },
      standard: { epochs: 3, learning_rate: 2e-4, lora_rank: 16, lora_alpha: 16, batch_size: 2 },
      thorough: { epochs: 5, learning_rate: 1e-4, lora_rank: 32, lora_alpha: 32, batch_size: 2 },
    };
    const defaults = presetDefaults[selectedPreset];
    if (defaults) {
      form.setFieldsValue(defaults);
    }
  }, [selectedPreset, form]);

  const datasetName = (id: string) =>
    datasets.find((d) => d.id === id)?.name ?? id;

  const columns: ColumnsType<FinetuneJob> = [
    {
      title: 'Dataset',
      dataIndex: 'dataset_id',
      key: 'dataset_id',
      render: (id: string) => datasetName(id),
    },
    { title: 'Base Model', dataIndex: 'base_model', key: 'base_model' },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (s: string) => <StatusTag status={s} />,
    },
    {
      title: 'Progress',
      key: 'progress',
      className: 'responsive-hide-sm',
      render: (_, record) => {
        const m = parseMetrics(record.metrics);
        if (!m) return <Text type="secondary">—</Text>;
        if (m.type === 'completed') {
          return <Progress percent={100} size="small" />;
        }
        if (m.step && m.total_steps) {
          const pct = Math.round((m.step / m.total_steps) * 100);
          return (
            <Space direction="vertical" size={0}>
              <Progress percent={pct} size="small" />
              <Text type="secondary" style={{ fontSize: 11 }}>
                Step {m.step}/{m.total_steps} | Loss: {m.loss?.toFixed(4) ?? '—'}
              </Text>
            </Space>
          );
        }
        if (m.message) return <Text type="secondary">{m.message}</Text>;
        return <Text type="secondary">—</Text>;
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_, record) => (
        <Space>
          {record.status === 'pending' && (
            <Button
              type="primary"
              size="small"
              icon={<PlayCircleOutlined />}
              onClick={() => handleStart(record.id)}
            >
              Start
            </Button>
          )}
          {record.status === 'running' && (
            <Button
              danger
              size="small"
              icon={<StopOutlined />}
              onClick={() => handleCancel(record.id)}
            >
              Cancel
            </Button>
          )}
          <Button
            size="small"
            icon={<FileTextOutlined />}
            onClick={() => handleViewLogs(record.id)}
          >
            Logs
          </Button>
          {['completed', 'failed', 'cancelled'].includes(record.status) && (
            <Popconfirm
              title="Delete this job?"
              onConfirm={() => handleDelete(record.id)}
            >
              <Button danger size="small" icon={<DeleteOutlined />} />
            </Popconfirm>
          )}
        </Space>
      ),
    },
  ];

  // Expandable row for completed jobs
  const expandedRowRender = (record: FinetuneJob) => {
    const m = parseMetrics(record.metrics);
    return (
      <div style={{ padding: '8px 0' }}>
        {m && (
          <Descriptions size="small" column={3} bordered>
            {m.final_loss !== undefined && (
              <Descriptions.Item label="Final Loss">{m.final_loss}</Descriptions.Item>
            )}
            {m.total_time_secs !== undefined && (
              <Descriptions.Item label="Duration">
                {Math.round(m.total_time_secs / 60)} min
              </Descriptions.Item>
            )}
            {m.step !== undefined && (
              <Descriptions.Item label="Steps">{m.step}/{m.total_steps}</Descriptions.Item>
            )}
            {m.loss !== undefined && (
              <Descriptions.Item label="Last Loss">{m.loss}</Descriptions.Item>
            )}
          </Descriptions>
        )}
        {record.output_model_path && (
          <Card size="small" style={{ marginTop: 8 }} title="Output Model">
            <Space direction="vertical" style={{ width: '100%' }}>
              <Space>
                <Text code>{record.output_model_path}</Text>
                <Button
                  size="small"
                  icon={<CopyOutlined />}
                  onClick={() => copyToClipboard(record.output_model_path!)}
                />
              </Space>
              <Paragraph
                style={{ background: '#f5f5f5', padding: 12, borderRadius: 4, fontSize: 12, fontFamily: 'monospace', margin: 0 }}
              >
                {`# Register with Ollama:\necho 'FROM ${record.output_model_path}' > Modelfile\nollama create my-finetuned-model -f Modelfile\nollama run my-finetuned-model`}
              </Paragraph>
            </Space>
          </Card>
        )}
        {m?.type === 'error' && m.message && (
          <Tag color="red" style={{ marginTop: 8 }}>Error: {m.message}</Tag>
        )}
      </div>
    );
  };

  return (
    <>
      <Card
        title="Fine-tuning Jobs"
        extra={
          <Space>
            <Button icon={<ReloadOutlined />} onClick={loadJobs}>
              Refresh
            </Button>
            <Button
              type="primary"
              icon={<PlusOutlined />}
              onClick={() => setCreateOpen(true)}
            >
              Create Job
            </Button>
          </Space>
        }
      >
        <Table
          dataSource={jobs}
          columns={columns}
          rowKey="id"
          loading={loading}
          pagination={{ pageSize: 10 }}
          size="small"
          expandable={{
            expandedRowRender,
            rowExpandable: (r) => r.status !== 'pending',
          }}
        />
      </Card>

      {/* Create Job Modal */}
      <Modal
        title="Create Fine-tuning Job"
        open={createOpen}
        onOk={handleCreateJob}
        onCancel={() => {
          setCreateOpen(false);
          form.resetFields();
        }}
        confirmLoading={creating}
        width={640}
      >
        <Form form={form} layout="vertical" initialValues={{
          epochs: 3, learning_rate: 2e-4, lora_rank: 16, lora_alpha: 16,
          batch_size: 2, warmup_ratio: 0.03, max_seq_length: 2048, quantization: 'q4_k_m',
        }}>
          <Form.Item
            name="dataset_id"
            label="Training Dataset"
            rules={[{ required: true, message: 'Select a dataset' }]}
          >
            <Select
              placeholder="Select dataset"
              options={datasets.map((d) => ({
                label: `${d.name} (${d.pair_count} pairs)`,
                value: d.id,
              }))}
            />
          </Form.Item>

          <Form.Item label="Model Source">
            <Radio.Group value={modelSource} onChange={(e) => setModelSource(e.target.value)}>
              <Radio.Button value="ollama">Ollama</Radio.Button>
              <Radio.Button value="huggingface">HuggingFace</Radio.Button>
            </Radio.Group>
          </Form.Item>

          <Form.Item
            name="base_model"
            label="Base Model"
            rules={[{ required: true, message: 'Base model is required' }]}
          >
            {modelSource === 'ollama' ? (
              <Select
                placeholder="Select an Ollama model"
                showSearch
                options={ollamaModels.map((m) => ({
                  label: m.name,
                  value: m.name,
                }))}
                notFoundContent="No Ollama models found"
              />
            ) : (
              <Input placeholder="e.g. unsloth/llama-3.2-3b-bnb-4bit" />
            )}
          </Form.Item>

          <Form.Item label="Quality Preset">
            <Radio.Group
              value={selectedPreset}
              onChange={(e) => setSelectedPreset(e.target.value)}
              optionType="button"
              buttonStyle="solid"
            >
              {presets.map((p) => (
                <Radio.Button key={p.key} value={p.key}>
                  {p.label}
                </Radio.Button>
              ))}
            </Radio.Group>
            <div style={{ marginTop: 4 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {presets.find((p) => p.key === selectedPreset)?.desc}
              </Text>
            </div>
          </Form.Item>

          <Collapse
            ghost
            items={[{
              key: 'advanced',
              label: 'Advanced Settings',
              children: (
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 16px' }}>
                  <Form.Item name="epochs" label="Epochs">
                    <InputNumber min={1} max={50} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="learning_rate" label="Learning Rate">
                    <InputNumber min={0.000001} max={0.01} step={0.00005} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="lora_rank" label="LoRA Rank">
                    <InputNumber min={4} max={128} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="lora_alpha" label="LoRA Alpha">
                    <InputNumber min={4} max={128} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="batch_size" label="Batch Size">
                    <InputNumber min={1} max={32} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="warmup_ratio" label="Warmup Ratio">
                    <InputNumber min={0} max={0.5} step={0.01} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="max_seq_length" label="Max Seq Length">
                    <InputNumber min={256} max={8192} step={256} style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name="quantization" label="GGUF Quantization">
                    <Select options={[
                      { label: 'Q4_K_M (recommended)', value: 'q4_k_m' },
                      { label: 'Q8_0 (higher quality)', value: 'q8_0' },
                      { label: 'F16 (full precision)', value: 'f16' },
                    ]} />
                  </Form.Item>
                </div>
              ),
            }]}
          />
        </Form>
      </Modal>

      {/* Logs Drawer */}
      <Drawer
        title={`Logs: ${logsJobId.slice(0, 8)}...`}
        open={logsDrawerOpen}
        onClose={() => setLogsDrawerOpen(false)}
        width={640}
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined />}
            onClick={() => handleViewLogs(logsJobId)}
          >
            Refresh
          </Button>
        }
      >
        <pre style={{
          background: '#1e1e1e',
          color: '#d4d4d4',
          padding: 12,
          borderRadius: 4,
          fontSize: 11,
          fontFamily: 'monospace',
          maxHeight: '80vh',
          overflow: 'auto',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-all',
        }}>
          {logLines.length > 0 ? logLines.join('\n') : 'No logs available'}
        </pre>
      </Drawer>
    </>
  );
}

// ── Main Page ─────────────────────────────────────────────────────────

export default function FinetunePage() {
  return (
    <div>
      <Title level={3}>
        <ExperimentOutlined /> Fine-tuning
      </Title>
      <Tabs
        defaultActiveKey="datasets"
        items={[
          {
            key: 'datasets',
            label: 'Datasets',
            children: <DatasetsTab />,
          },
          {
            key: 'jobs',
            label: 'Jobs',
            children: <JobsTab />,
          },
        ]}
      />
    </div>
  );
}
