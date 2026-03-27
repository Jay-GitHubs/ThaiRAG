import { useState, useEffect, useCallback } from 'react';
import {
  Card,
  Table,
  Button,
  Space,
  Typography,
  Statistic,
  Row,
  Col,
  Modal,
  Form,
  Input,
  message,
  Popconfirm,
  Tabs,
  Progress,
  Upload,
  Spin,
  Tag,
  Tooltip,
} from 'antd';
import {
  PlayCircleOutlined,
  DeleteOutlined,
  PlusOutlined,
  UploadOutlined,
  HistoryOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import {
  listQuerySets,
  createQuerySet,
  deleteQuerySet,
  runEvaluation,
  listResults,
  importQuerySet,
} from '../api/eval';
import type {
  EvalQuerySet,
  EvalResult,
  EvalMetrics,
  QueryEvalResult,
} from '../api/eval';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

// ── Metric Display Helpers ──────────────────────────────────────────

function metricColor(value: number): string {
  if (value >= 0.8) return '#52c41a';
  if (value >= 0.5) return '#faad14';
  return '#ff4d4f';
}

function MetricCard({ title, value, suffix }: { title: string; value: number; suffix?: string }) {
  const displayValue = suffix === 'ms' ? value.toFixed(0) : value.toFixed(3);
  return (
    <Card size="small">
      <Statistic
        title={title}
        value={displayValue}
        suffix={suffix}
        valueStyle={{
          color: suffix === 'ms' ? undefined : metricColor(value),
          fontSize: 20,
        }}
      />
    </Card>
  );
}

// ── Main Page ───────────────────────────────────────────────────────

export default function EvalPage() {
  const [querySets, setQuerySets] = useState<EvalQuerySet[]>([]);
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [selectedSet, setSelectedSet] = useState<EvalQuerySet | null>(null);
  const [results, setResults] = useState<EvalResult[]>([]);
  const [resultsLoading, setResultsLoading] = useState(false);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [latestResult, setLatestResult] = useState<EvalResult | null>(null);

  const [createForm] = Form.useForm();
  const [importForm] = Form.useForm();

  const loadSets = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listQuerySets();
      setQuerySets(data);
    } catch {
      message.error('Failed to load query sets');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSets();
  }, [loadSets]);

  const handleCreate = async (values: { name: string; queriesJson: string }) => {
    try {
      const queries = JSON.parse(values.queriesJson);
      if (!Array.isArray(queries) || queries.length === 0) {
        message.error('Queries must be a non-empty JSON array');
        return;
      }
      await createQuerySet({ name: values.name, queries });
      message.success('Query set created');
      setCreateOpen(false);
      createForm.resetFields();
      loadSets();
    } catch (err: unknown) {
      const errorMsg = err instanceof SyntaxError ? 'Invalid JSON' : 'Failed to create query set';
      message.error(errorMsg);
    }
  };

  const handleImport = async (values: { name: string; csvData: string }) => {
    try {
      await importQuerySet({ name: values.name, csv_data: values.csvData });
      message.success('Query set imported from CSV');
      setImportOpen(false);
      importForm.resetFields();
      loadSets();
    } catch {
      message.error('Failed to import CSV');
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteQuerySet(id);
      message.success('Query set deleted');
      if (selectedSet?.id === id) {
        setSelectedSet(null);
        setResults([]);
        setLatestResult(null);
      }
      loadSets();
    } catch {
      message.error('Failed to delete query set');
    }
  };

  const handleRun = async (qs: EvalQuerySet) => {
    setRunningId(qs.id);
    try {
      const result = await runEvaluation(qs.id);
      message.success('Evaluation complete');
      setLatestResult(result);
      setSelectedSet(qs);
      // Refresh results
      const allResults = await listResults(qs.id);
      setResults(allResults);
    } catch {
      message.error('Evaluation failed');
    } finally {
      setRunningId(null);
    }
  };

  const handleViewResults = async (qs: EvalQuerySet) => {
    setSelectedSet(qs);
    setResultsLoading(true);
    try {
      const data = await listResults(qs.id);
      setResults(data);
      setLatestResult(data.length > 0 ? data[0] : null);
    } catch {
      message.error('Failed to load results');
    } finally {
      setResultsLoading(false);
    }
  };

  // ── Query Sets Table ──────────────────────────────────────────────

  const setColumns: ColumnsType<EvalQuerySet> = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record) => (
        <a onClick={() => handleViewResults(record)}>{name}</a>
      ),
    },
    {
      title: 'Queries',
      key: 'query_count',
      width: 100,
      render: (_, record) => <Tag>{record.queries.length}</Tag>,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 200,
      render: (v: string) => new Date(v).toLocaleString(),
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 250,
      render: (_, record) => (
        <Space>
          <Tooltip title="Run Evaluation">
            <Button
              icon={<PlayCircleOutlined />}
              type="primary"
              size="small"
              loading={runningId === record.id}
              onClick={() => handleRun(record)}
            >
              Run
            </Button>
          </Tooltip>
          <Tooltip title="View Results">
            <Button
              icon={<HistoryOutlined />}
              size="small"
              onClick={() => handleViewResults(record)}
            >
              Results
            </Button>
          </Tooltip>
          <Popconfirm
            title="Delete this query set?"
            onConfirm={() => handleDelete(record.id)}
          >
            <Button icon={<DeleteOutlined />} size="small" danger />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  // ── Per-Query Results Table ───────────────────────────────────────

  const queryResultColumns: ColumnsType<QueryEvalResult> = [
    {
      title: 'Query',
      dataIndex: 'query',
      key: 'query',
      ellipsis: true,
    },
    {
      title: 'NDCG@5',
      dataIndex: 'ndcg_at_5',
      key: 'ndcg_at_5',
      width: 100,
      render: (v: number) => (
        <span style={{ color: metricColor(v) }}>{v.toFixed(3)}</span>
      ),
      sorter: (a, b) => a.ndcg_at_5 - b.ndcg_at_5,
    },
    {
      title: 'NDCG@10',
      dataIndex: 'ndcg_at_10',
      key: 'ndcg_at_10',
      width: 100,
      render: (v: number) => (
        <span style={{ color: metricColor(v) }}>{v.toFixed(3)}</span>
      ),
      sorter: (a, b) => a.ndcg_at_10 - b.ndcg_at_10,
    },
    {
      title: 'MRR',
      dataIndex: 'mrr',
      key: 'mrr',
      width: 80,
      render: (v: number) => (
        <span style={{ color: metricColor(v) }}>{v.toFixed(3)}</span>
      ),
      sorter: (a, b) => a.mrr - b.mrr,
    },
    {
      title: 'P@5',
      dataIndex: 'precision',
      key: 'precision',
      width: 80,
      render: (v: number) => (
        <span style={{ color: metricColor(v) }}>{v.toFixed(3)}</span>
      ),
      sorter: (a, b) => a.precision - b.precision,
    },
    {
      title: 'R@10',
      dataIndex: 'recall',
      key: 'recall',
      width: 80,
      render: (v: number) => (
        <span style={{ color: metricColor(v) }}>{v.toFixed(3)}</span>
      ),
      sorter: (a, b) => a.recall - b.recall,
    },
    {
      title: 'Latency',
      dataIndex: 'latency_ms',
      key: 'latency_ms',
      width: 100,
      render: (v: number) => `${v}ms`,
      sorter: (a, b) => a.latency_ms - b.latency_ms,
    },
    {
      title: 'Retrieved',
      key: 'retrieved',
      width: 90,
      render: (_, r) => <Tag>{r.retrieved_doc_ids.length} docs</Tag>,
    },
  ];

  // ── Historical Comparison Table ───────────────────────────────────

  const last5 = results.slice(0, 5);

  const historyColumns: ColumnsType<EvalResult> = [
    {
      title: 'Run',
      dataIndex: 'run_at',
      key: 'run_at',
      width: 200,
      render: (v: string) => new Date(v).toLocaleString(),
    },
    {
      title: 'NDCG@5',
      key: 'ndcg_at_5',
      width: 100,
      render: (_, r) => (
        <span style={{ color: metricColor(r.metrics.ndcg_at_5) }}>
          {r.metrics.ndcg_at_5.toFixed(3)}
        </span>
      ),
    },
    {
      title: 'NDCG@10',
      key: 'ndcg_at_10',
      width: 100,
      render: (_, r) => (
        <span style={{ color: metricColor(r.metrics.ndcg_at_10) }}>
          {r.metrics.ndcg_at_10.toFixed(3)}
        </span>
      ),
    },
    {
      title: 'MRR',
      key: 'mrr',
      width: 80,
      render: (_, r) => (
        <span style={{ color: metricColor(r.metrics.mrr) }}>
          {r.metrics.mrr.toFixed(3)}
        </span>
      ),
    },
    {
      title: 'P@5',
      key: 'p5',
      width: 80,
      render: (_, r) => (
        <span style={{ color: metricColor(r.metrics.precision_at_5) }}>
          {r.metrics.precision_at_5.toFixed(3)}
        </span>
      ),
    },
    {
      title: 'P@10',
      key: 'p10',
      width: 80,
      render: (_, r) => (
        <span style={{ color: metricColor(r.metrics.precision_at_10) }}>
          {r.metrics.precision_at_10.toFixed(3)}
        </span>
      ),
    },
    {
      title: 'R@10',
      key: 'r10',
      width: 80,
      render: (_, r) => (
        <span style={{ color: metricColor(r.metrics.recall_at_10) }}>
          {r.metrics.recall_at_10.toFixed(3)}
        </span>
      ),
    },
    {
      title: 'Avg Latency',
      key: 'latency',
      width: 110,
      render: (_, r) => `${r.metrics.mean_latency_ms.toFixed(0)}ms`,
    },
  ];

  return (
    <div>
      <Row justify="space-between" align="middle" style={{ marginBottom: 16 }}>
        <Col>
          <Title level={3} style={{ margin: 0 }}>
            Search Quality Evaluation
          </Title>
        </Col>
        <Col>
          <Space>
            <Button icon={<ReloadOutlined />} onClick={loadSets}>
              Refresh
            </Button>
            <Button icon={<UploadOutlined />} onClick={() => setImportOpen(true)}>
              Import CSV
            </Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
              New Query Set
            </Button>
          </Space>
        </Col>
      </Row>

      <Card title="Query Sets" style={{ marginBottom: 24 }}>
        <Table
          dataSource={querySets}
          columns={setColumns}
          rowKey="id"
          loading={loading}
          size="middle"
          pagination={{ pageSize: 10 }}
        />
      </Card>

      {/* Results Section */}
      {selectedSet && (
        <Card
          title={`Results: ${selectedSet.name}`}
          extra={
            <Button
              icon={<PlayCircleOutlined />}
              type="primary"
              loading={runningId === selectedSet.id}
              onClick={() => handleRun(selectedSet)}
            >
              Run Again
            </Button>
          }
        >
          <Spin spinning={resultsLoading}>
            <Tabs
              items={[
                {
                  key: 'latest',
                  label: 'Latest Run',
                  children: latestResult ? (
                    <>
                      <Paragraph type="secondary">
                        Run at: {new Date(latestResult.run_at).toLocaleString()}
                      </Paragraph>
                      <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
                        <Col span={4}>
                          <MetricCard title="NDCG@5" value={latestResult.metrics.ndcg_at_5} />
                        </Col>
                        <Col span={4}>
                          <MetricCard title="NDCG@10" value={latestResult.metrics.ndcg_at_10} />
                        </Col>
                        <Col span={3}>
                          <MetricCard title="MRR" value={latestResult.metrics.mrr} />
                        </Col>
                        <Col span={3}>
                          <MetricCard title="P@5" value={latestResult.metrics.precision_at_5} />
                        </Col>
                        <Col span={3}>
                          <MetricCard title="P@10" value={latestResult.metrics.precision_at_10} />
                        </Col>
                        <Col span={3}>
                          <MetricCard title="R@10" value={latestResult.metrics.recall_at_10} />
                        </Col>
                        <Col span={4}>
                          <MetricCard
                            title="Avg Latency"
                            value={latestResult.metrics.mean_latency_ms}
                            suffix="ms"
                          />
                        </Col>
                      </Row>
                      <Title level={5}>Per-Query Breakdown</Title>
                      <Table
                        dataSource={latestResult.per_query}
                        columns={queryResultColumns}
                        rowKey="query"
                        size="small"
                        pagination={{ pageSize: 20 }}
                      />
                    </>
                  ) : (
                    <Paragraph type="secondary">
                      No evaluation results yet. Click "Run" to evaluate.
                    </Paragraph>
                  ),
                },
                {
                  key: 'history',
                  label: `History (${results.length} runs)`,
                  children: (
                    <>
                      <Paragraph type="secondary">
                        Showing last {last5.length} runs for comparison.
                      </Paragraph>
                      <Table
                        dataSource={last5}
                        columns={historyColumns}
                        rowKey="run_at"
                        size="small"
                        pagination={false}
                      />
                    </>
                  ),
                },
              ]}
            />
          </Spin>
        </Card>
      )}

      {/* Create Modal */}
      <Modal
        title="Create Query Set"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
        width={640}
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item name="name" label="Name" rules={[{ required: true }]}>
            <Input placeholder="e.g., Tax Document Queries v1" />
          </Form.Item>
          <Form.Item
            name="queriesJson"
            label="Queries (JSON)"
            rules={[{ required: true }]}
            extra='Array of { "query": "...", "relevant_doc_ids": ["uuid1", "uuid2"] }'
          >
            <TextArea
              rows={10}
              placeholder={`[
  {
    "query": "What is the corporate tax rate?",
    "relevant_doc_ids": ["doc-uuid-1", "doc-uuid-2"]
  }
]`}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* Import CSV Modal */}
      <Modal
        title="Import Query Set from CSV"
        open={importOpen}
        onCancel={() => setImportOpen(false)}
        onOk={() => importForm.submit()}
        width={640}
      >
        <Form form={importForm} layout="vertical" onFinish={handleImport}>
          <Form.Item name="name" label="Name" rules={[{ required: true }]}>
            <Input placeholder="e.g., Imported Eval Set" />
          </Form.Item>
          <Form.Item
            name="csvData"
            label="CSV Data"
            rules={[{ required: true }]}
            extra="Format: query,doc_id (one row per query-document pair). Duplicate queries are merged."
          >
            <TextArea
              rows={10}
              placeholder={`query,doc_id
"What is the tax rate?",550e8400-e29b-41d4-a716-446655440000
"How to file taxes?",550e8400-e29b-41d4-a716-446655440001`}
            />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
