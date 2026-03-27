import { useState, useEffect, useCallback } from 'react';
import {
  Typography, Card, Button, Table, Tag, Space, Modal, Form, Input, InputNumber,
  message, Popconfirm, Descriptions, Row, Col, Statistic, Collapse, Spin,
} from 'antd';
import {
  PlusOutlined, PlayCircleOutlined, DeleteOutlined, EyeOutlined,
  TrophyOutlined, SwapOutlined,
} from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import {
  listAbTests, createAbTest, deleteAbTest, runAbTest, compareAbTest,
} from '../api/abTest';
import type { AbTest, AbQueryResult, CreateAbTestRequest } from '../api/abTest';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

export default function AbTestPage() {
  const [tests, setTests] = useState<AbTest[]>([]);
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [runOpen, setRunOpen] = useState(false);
  const [compareOpen, setCompareOpen] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const [selectedTest, setSelectedTest] = useState<AbTest | null>(null);
  const [running, setRunning] = useState(false);
  const [comparing, setComparing] = useState(false);
  const [compareResult, setCompareResult] = useState<AbQueryResult | null>(null);
  const [createForm] = Form.useForm();
  const [runForm] = Form.useForm();
  const [compareForm] = Form.useForm();

  const fetchTests = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listAbTests();
      setTests(data);
    } catch (err) {
      message.error('Failed to load A/B tests');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchTests(); }, [fetchTests]);

  const handleCreate = async (values: Record<string, unknown>) => {
    try {
      const req: CreateAbTestRequest = {
        name: values.name as string,
        description: (values.description as string) || '',
        variant_a: {
          name: (values.variant_a_name as string) || 'Variant A',
          search_config: {
            top_k: values.a_top_k as number | undefined,
            vector_weight: values.a_vector_weight as number | undefined,
            text_weight: values.a_text_weight as number | undefined,
          },
          llm_model: values.a_llm_model as string | undefined,
          prompt_template: values.a_prompt as string | undefined,
        },
        variant_b: {
          name: (values.variant_b_name as string) || 'Variant B',
          search_config: {
            top_k: values.b_top_k as number | undefined,
            vector_weight: values.b_vector_weight as number | undefined,
            text_weight: values.b_text_weight as number | undefined,
          },
          llm_model: values.b_llm_model as string | undefined,
          prompt_template: values.b_prompt as string | undefined,
        },
      };
      await createAbTest(req);
      message.success('A/B test created');
      setCreateOpen(false);
      createForm.resetFields();
      fetchTests();
    } catch {
      message.error('Failed to create A/B test');
    }
  };

  const handleRun = async (values: Record<string, unknown>) => {
    if (!selectedTest) return;
    const queriesRaw = values.queries as string;
    const queries = queriesRaw.split('\n').map(q => q.trim()).filter(Boolean);
    if (queries.length === 0) {
      message.warning('Enter at least one query');
      return;
    }
    setRunning(true);
    try {
      const updated = await runAbTest(selectedTest.id, queries);
      message.success('A/B test completed');
      setRunOpen(false);
      runForm.resetFields();
      // Show results
      setSelectedTest(updated);
      setDetailOpen(true);
      fetchTests();
    } catch {
      message.error('Failed to run A/B test');
    } finally {
      setRunning(false);
    }
  };

  const handleCompare = async (values: Record<string, unknown>) => {
    if (!selectedTest) return;
    const query = (values.query as string).trim();
    if (!query) return;
    setComparing(true);
    try {
      const result = await compareAbTest(selectedTest.id, query);
      setCompareResult(result);
    } catch {
      message.error('Failed to run comparison');
    } finally {
      setComparing(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteAbTest(id);
      message.success('Deleted');
      fetchTests();
    } catch {
      message.error('Failed to delete');
    }
  };

  const statusColor = (s: string) => {
    switch (s) {
      case 'draft': return 'default';
      case 'running': return 'processing';
      case 'completed': return 'success';
      default: return 'default';
    }
  };

  const columns: ColumnsType<AbTest> = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: AbTest) => (
        <Space direction="vertical" size={0}>
          <Text strong>{name}</Text>
          {record.description && <Text type="secondary" style={{ fontSize: 12 }}>{record.description}</Text>}
        </Space>
      ),
    },
    {
      title: 'Variants',
      key: 'variants',
      render: (_: unknown, record: AbTest) => (
        <Space>
          <Tag color="blue">{record.variant_a.name}</Tag>
          <Text type="secondary">vs</Text>
          <Tag color="orange">{record.variant_b.name}</Tag>
        </Space>
      ),
    },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (s: string) => <Tag color={statusColor(s)}>{s.toUpperCase()}</Tag>,
    },
    {
      title: 'Winner',
      key: 'winner',
      render: (_: unknown, record: AbTest) => {
        if (!record.results?.winner) return <Text type="secondary">-</Text>;
        return (
          <Space>
            <TrophyOutlined style={{ color: '#faad14' }} />
            <Text strong>{record.results.winner}</Text>
          </Space>
        );
      },
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (d: string) => new Date(d).toLocaleDateString(),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: AbTest) => (
        <Space>
          {record.status === 'draft' && (
            <Button
              size="small"
              icon={<PlayCircleOutlined />}
              onClick={() => { setSelectedTest(record); setRunOpen(true); }}
            >
              Run
            </Button>
          )}
          <Button
            size="small"
            icon={<SwapOutlined />}
            onClick={() => {
              setSelectedTest(record);
              setCompareResult(null);
              compareForm.resetFields();
              setCompareOpen(true);
            }}
          >
            Compare
          </Button>
          {record.results && (
            <Button
              size="small"
              icon={<EyeOutlined />}
              onClick={() => { setSelectedTest(record); setDetailOpen(true); }}
            >
              Results
            </Button>
          )}
          <Popconfirm title="Delete this test?" onConfirm={() => handleDelete(record.id)}>
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const results = selectedTest?.results;

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={3} style={{ margin: 0 }}>A/B Testing</Title>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
          New Test
        </Button>
      </div>

      <Card>
        <Table
          columns={columns}
          dataSource={tests}
          rowKey="id"
          loading={loading}
          pagination={false}
          locale={{ emptyText: 'No A/B tests yet. Create one to compare search configurations.' }}
        />
      </Card>

      {/* Create Modal */}
      <Modal
        title="Create A/B Test"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
        width={700}
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item name="name" label="Test Name" rules={[{ required: true }]}>
            <Input placeholder="e.g. Vector weight comparison" />
          </Form.Item>
          <Form.Item name="description" label="Description">
            <Input placeholder="Optional description" />
          </Form.Item>

          <Row gutter={24}>
            <Col span={12}>
              <Card size="small" title="Variant A" style={{ marginBottom: 16 }}>
                <Form.Item name="variant_a_name" label="Name" initialValue="Variant A">
                  <Input />
                </Form.Item>
                <Form.Item name="a_top_k" label="Top K">
                  <InputNumber min={1} max={50} style={{ width: '100%' }} placeholder="default" />
                </Form.Item>
                <Form.Item name="a_vector_weight" label="Vector Weight">
                  <InputNumber min={0} max={1} step={0.1} style={{ width: '100%' }} placeholder="default" />
                </Form.Item>
                <Form.Item name="a_text_weight" label="Text Weight">
                  <InputNumber min={0} max={1} step={0.1} style={{ width: '100%' }} placeholder="default" />
                </Form.Item>
                <Form.Item name="a_llm_model" label="LLM Model">
                  <Input placeholder="default" />
                </Form.Item>
                <Form.Item name="a_prompt" label="Prompt Template">
                  <TextArea rows={2} placeholder="Custom system prompt (optional)" />
                </Form.Item>
              </Card>
            </Col>
            <Col span={12}>
              <Card size="small" title="Variant B" style={{ marginBottom: 16 }}>
                <Form.Item name="variant_b_name" label="Name" initialValue="Variant B">
                  <Input />
                </Form.Item>
                <Form.Item name="b_top_k" label="Top K">
                  <InputNumber min={1} max={50} style={{ width: '100%' }} placeholder="default" />
                </Form.Item>
                <Form.Item name="b_vector_weight" label="Vector Weight">
                  <InputNumber min={0} max={1} step={0.1} style={{ width: '100%' }} placeholder="default" />
                </Form.Item>
                <Form.Item name="b_text_weight" label="Text Weight">
                  <InputNumber min={0} max={1} step={0.1} style={{ width: '100%' }} placeholder="default" />
                </Form.Item>
                <Form.Item name="b_llm_model" label="LLM Model">
                  <Input placeholder="default" />
                </Form.Item>
                <Form.Item name="b_prompt" label="Prompt Template">
                  <TextArea rows={2} placeholder="Custom system prompt (optional)" />
                </Form.Item>
              </Card>
            </Col>
          </Row>
        </Form>
      </Modal>

      {/* Run Modal */}
      <Modal
        title={`Run: ${selectedTest?.name ?? ''}`}
        open={runOpen}
        onCancel={() => { if (!running) setRunOpen(false); }}
        onOk={() => runForm.submit()}
        confirmLoading={running}
        okText={running ? 'Running...' : 'Run Test'}
        closable={!running}
        maskClosable={!running}
      >
        <Paragraph type="secondary">
          Enter test queries (one per line). Each query will be run through both variants and compared.
        </Paragraph>
        <Form form={runForm} layout="vertical" onFinish={handleRun}>
          <Form.Item name="queries" label="Queries" rules={[{ required: true }]}>
            <TextArea rows={8} placeholder={"What is the company leave policy?\nHow do I submit an expense report?\nWhat are the working hours?"} />
          </Form.Item>
        </Form>
      </Modal>

      {/* Compare Modal */}
      <Modal
        title={`Compare: ${selectedTest?.name ?? ''}`}
        open={compareOpen}
        onCancel={() => setCompareOpen(false)}
        footer={null}
        width={900}
      >
        <Form form={compareForm} layout="inline" onFinish={handleCompare} style={{ marginBottom: 16 }}>
          <Form.Item name="query" style={{ flex: 1 }} rules={[{ required: true }]}>
            <Input placeholder="Enter a query to compare..." />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit" loading={comparing} icon={<SwapOutlined />}>
              Compare
            </Button>
          </Form.Item>
        </Form>

        {comparing && <div style={{ textAlign: 'center', padding: 32 }}><Spin tip="Running comparison..." /></div>}

        {compareResult && !comparing && (
          <Row gutter={16}>
            <Col span={12}>
              <Card
                size="small"
                title={<Space><Tag color="blue">{selectedTest?.variant_a.name}</Tag></Space>}
              >
                <Descriptions column={1} size="small">
                  <Descriptions.Item label="Latency">{compareResult.variant_a.latency_ms} ms</Descriptions.Item>
                  <Descriptions.Item label="Tokens">{compareResult.variant_a.token_count}</Descriptions.Item>
                  <Descriptions.Item label="Chunks">{compareResult.variant_a.chunks_retrieved}</Descriptions.Item>
                  <Descriptions.Item label="Relevance">{compareResult.variant_a.relevance_score.toFixed(3)}</Descriptions.Item>
                </Descriptions>
                <div style={{ marginTop: 8, padding: 8, background: 'var(--ant-color-fill-tertiary)', borderRadius: 4, whiteSpace: 'pre-wrap', maxHeight: 300, overflow: 'auto' }}>
                  {compareResult.variant_a.answer}
                </div>
              </Card>
            </Col>
            <Col span={12}>
              <Card
                size="small"
                title={<Space><Tag color="orange">{selectedTest?.variant_b.name}</Tag></Space>}
              >
                <Descriptions column={1} size="small">
                  <Descriptions.Item label="Latency">{compareResult.variant_b.latency_ms} ms</Descriptions.Item>
                  <Descriptions.Item label="Tokens">{compareResult.variant_b.token_count}</Descriptions.Item>
                  <Descriptions.Item label="Chunks">{compareResult.variant_b.chunks_retrieved}</Descriptions.Item>
                  <Descriptions.Item label="Relevance">{compareResult.variant_b.relevance_score.toFixed(3)}</Descriptions.Item>
                </Descriptions>
                <div style={{ marginTop: 8, padding: 8, background: 'var(--ant-color-fill-tertiary)', borderRadius: 4, whiteSpace: 'pre-wrap', maxHeight: 300, overflow: 'auto' }}>
                  {compareResult.variant_b.answer}
                </div>
              </Card>
            </Col>
          </Row>
        )}
      </Modal>

      {/* Results Detail Modal */}
      <Modal
        title={`Results: ${selectedTest?.name ?? ''}`}
        open={detailOpen}
        onCancel={() => setDetailOpen(false)}
        footer={null}
        width={1000}
      >
        {results && (
          <>
            {/* Winner banner */}
            {results.winner && (
              <Card size="small" style={{ marginBottom: 16, textAlign: 'center' }}>
                <Space>
                  <TrophyOutlined style={{ fontSize: 24, color: '#faad14' }} />
                  <Title level={4} style={{ margin: 0 }}>Winner: {results.winner}</Title>
                </Space>
              </Card>
            )}

            {/* Metrics comparison */}
            <Row gutter={16} style={{ marginBottom: 16 }}>
              <Col span={12}>
                <Card size="small" title={<Tag color="blue">{selectedTest?.variant_a.name}</Tag>}>
                  <Row gutter={16}>
                    <Col span={12}>
                      <Statistic title="Avg Latency" value={results.variant_a_metrics.avg_latency_ms} precision={0} suffix="ms" />
                    </Col>
                    <Col span={12}>
                      <Statistic title="Avg Relevance" value={results.variant_a_metrics.avg_relevance_score} precision={3} />
                    </Col>
                    <Col span={12}>
                      <Statistic title="Avg Tokens" value={results.variant_a_metrics.avg_token_count} precision={0} />
                    </Col>
                    <Col span={12}>
                      <Statistic title="Queries" value={results.variant_a_metrics.total_queries} />
                    </Col>
                  </Row>
                </Card>
              </Col>
              <Col span={12}>
                <Card size="small" title={<Tag color="orange">{selectedTest?.variant_b.name}</Tag>}>
                  <Row gutter={16}>
                    <Col span={12}>
                      <Statistic title="Avg Latency" value={results.variant_b_metrics.avg_latency_ms} precision={0} suffix="ms" />
                    </Col>
                    <Col span={12}>
                      <Statistic title="Avg Relevance" value={results.variant_b_metrics.avg_relevance_score} precision={3} />
                    </Col>
                    <Col span={12}>
                      <Statistic title="Avg Tokens" value={results.variant_b_metrics.avg_token_count} precision={0} />
                    </Col>
                    <Col span={12}>
                      <Statistic title="Queries" value={results.variant_b_metrics.total_queries} />
                    </Col>
                  </Row>
                </Card>
              </Col>
            </Row>

            {/* Per-query results */}
            <Collapse
              items={results.per_query.map((q: AbQueryResult, i: number) => ({
                key: i,
                label: <Text strong>Q{i + 1}: {q.query}</Text>,
                children: (
                  <Row gutter={16}>
                    <Col span={12}>
                      <Card size="small" title={<Tag color="blue">{selectedTest?.variant_a.name}</Tag>}>
                        <Descriptions column={2} size="small">
                          <Descriptions.Item label="Latency">{q.variant_a.latency_ms} ms</Descriptions.Item>
                          <Descriptions.Item label="Tokens">{q.variant_a.token_count}</Descriptions.Item>
                          <Descriptions.Item label="Chunks">{q.variant_a.chunks_retrieved}</Descriptions.Item>
                          <Descriptions.Item label="Relevance">{q.variant_a.relevance_score.toFixed(3)}</Descriptions.Item>
                        </Descriptions>
                        <div style={{ marginTop: 8, padding: 8, background: 'var(--ant-color-fill-tertiary)', borderRadius: 4, whiteSpace: 'pre-wrap', maxHeight: 200, overflow: 'auto', fontSize: 13 }}>
                          {q.variant_a.answer}
                        </div>
                      </Card>
                    </Col>
                    <Col span={12}>
                      <Card size="small" title={<Tag color="orange">{selectedTest?.variant_b.name}</Tag>}>
                        <Descriptions column={2} size="small">
                          <Descriptions.Item label="Latency">{q.variant_b.latency_ms} ms</Descriptions.Item>
                          <Descriptions.Item label="Tokens">{q.variant_b.token_count}</Descriptions.Item>
                          <Descriptions.Item label="Chunks">{q.variant_b.chunks_retrieved}</Descriptions.Item>
                          <Descriptions.Item label="Relevance">{q.variant_b.relevance_score.toFixed(3)}</Descriptions.Item>
                        </Descriptions>
                        <div style={{ marginTop: 8, padding: 8, background: 'var(--ant-color-fill-tertiary)', borderRadius: 4, whiteSpace: 'pre-wrap', maxHeight: 200, overflow: 'auto', fontSize: 13 }}>
                          {q.variant_b.answer}
                        </div>
                      </Card>
                    </Col>
                  </Row>
                ),
              }))}
            />
          </>
        )}
      </Modal>
    </div>
  );
}
