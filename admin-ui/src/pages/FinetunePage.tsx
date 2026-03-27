import { useState, useEffect, useCallback } from 'react';
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
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  ExperimentOutlined,
  ReloadOutlined,
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
} from '../api/finetune';
import type {
  TrainingDataset,
  TrainingPair,
  FinetuneJob,
} from '../api/finetune';

const { Title, Text } = Typography;
const { TextArea } = Input;

// ── Status Tag ───────────────────────────────────────────────────────

function StatusTag({ status }: { status: string }) {
  const colorMap: Record<string, string> = {
    pending: 'gold',
    running: 'blue',
    completed: 'green',
    failed: 'red',
  };
  return <Tag color={colorMap[status] ?? 'default'}>{status}</Tag>;
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
    { title: 'Description', dataIndex: 'description', key: 'description' },
    { title: 'Pairs', dataIndex: 'pair_count', key: 'pair_count' },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
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
      render: (v?: string) => v ?? <Text type="secondary">—</Text>,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
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
            <Button
              type="primary"
              icon={<PlusOutlined />}
              onClick={() => setAddPairOpen(true)}
            >
              Add Pair
            </Button>
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
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [form] = Form.useForm();

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
  }, [loadJobs]);

  const handleCreateJob = async () => {
    try {
      const values = await form.validateFields();
      setCreating(true);
      await createJob(values);
      message.success('Fine-tuning job created (status: pending)');
      setCreateOpen(false);
      form.resetFields();
      loadJobs();
    } catch {
      message.error('Failed to create job');
    } finally {
      setCreating(false);
    }
  };

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
      title: 'Output Path',
      dataIndex: 'output_model_path',
      key: 'output_model_path',
      render: (v?: string) =>
        v ? <Text code>{v}</Text> : <Text type="secondary">—</Text>,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => new Date(v).toLocaleDateString(),
    },
  ];

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
        />
      </Card>

      <Modal
        title="Create Fine-tuning Job"
        open={createOpen}
        onOk={handleCreateJob}
        onCancel={() => {
          setCreateOpen(false);
          form.resetFields();
        }}
        confirmLoading={creating}
      >
        <Form form={form} layout="vertical">
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
          <Form.Item
            name="base_model"
            label="Base Model"
            rules={[{ required: true, message: 'Base model is required' }]}
          >
            <Input placeholder="e.g. BAAI/bge-m3" />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}

// ── Main Page ─────────────────────────────────────────────────────────

export default function FinetunePage() {
  return (
    <div>
      <Title level={3}>
        <ExperimentOutlined /> Embedding Fine-tuning
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
