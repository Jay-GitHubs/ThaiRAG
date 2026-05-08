import { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  DatePicker,
  Empty,
  Input,
  Row,
  Space,
  Spin,
  Statistic,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from 'antd';
import type { Dayjs } from 'dayjs';
import {
  SafetyOutlined,
  DashboardOutlined,
  FileTextOutlined,
  ExperimentOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
} from '@ant-design/icons';
import {
  getGuardrailsStats,
  listGuardrailViolations,
  previewGuardrails,
  type GuardrailsStats,
  type ViolationRow,
  type PreviewResponse,
} from '../api/guardrails';

const { RangePicker } = DatePicker;

function formatTimestamp(ts: string): string {
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? ts : d.toLocaleString();
}

function actionTag(action: string) {
  switch (action) {
    case 'pass':
      return <Tag color="green">Pass</Tag>;
    case 'sanitize':
      return <Tag color="orange">Sanitize</Tag>;
    case 'block':
      return <Tag color="red">Block</Tag>;
    case 'regenerate':
      return <Tag color="blue">Regenerate</Tag>;
    default:
      return <Tag>{action}</Tag>;
  }
}

// ── Dashboard tab ──────────────────────────────────────────────────

function DashboardTab({ from, to }: { from?: string; to?: string }) {
  const [loading, setLoading] = useState(true);
  const [stats, setStats] = useState<GuardrailsStats | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getGuardrailsStats({ from, to })
      .then((s) => {
        if (!cancelled) setStats(s);
      })
      .catch(() => message.error('Failed to load guardrail stats'))
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [from, to]);

  if (loading) return <Spin />;
  if (!stats) return <Empty description="No data" />;

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Input checks"
              value={stats.input_checks_total}
              prefix={<CheckCircleOutlined />}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Output checks"
              value={stats.output_checks_total}
              prefix={<CheckCircleOutlined />}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Violations"
              value={stats.violations_total}
              valueStyle={{ color: stats.violations_total > 0 ? '#cf1322' : undefined }}
              prefix={<CloseCircleOutlined />}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Blocks (in/out)"
              value={`${stats.input_blocks_total} / ${stats.output_blocks_total}`}
              prefix={<SafetyOutlined />}
            />
          </Card>
        </Col>
      </Row>

      <Card title="Top violation codes" size="small">
        {stats.by_code.length === 0 ? (
          <Empty description="No violations in this window" />
        ) : (
          <Table
            size="small"
            rowKey="code"
            pagination={false}
            dataSource={stats.by_code}
            columns={[
              {
                title: 'Code',
                dataIndex: 'code',
                render: (c: string) => <Tag color="red">{c}</Tag>,
              },
              {
                title: 'Count',
                dataIndex: 'count',
                align: 'right',
                width: 120,
              },
            ]}
          />
        )}
      </Card>
    </Space>
  );
}

// ── Violation Log tab ──────────────────────────────────────────────

function ViolationLogTab({ from, to }: { from?: string; to?: string }) {
  const [loading, setLoading] = useState(true);
  const [rows, setRows] = useState<ViolationRow[]>([]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listGuardrailViolations({ from, to, limit: 500 })
      .then((r) => {
        if (!cancelled) setRows(r.entries);
      })
      .catch(() => message.error('Failed to load violations'))
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [from, to]);

  if (loading) return <Spin />;
  return (
    <Table<ViolationRow>
      size="small"
      rowKey="response_id"
      dataSource={rows}
      pagination={{ pageSize: 25 }}
      locale={{ emptyText: 'No violations recorded' }}
      columns={[
        {
          title: 'Time',
          dataIndex: 'timestamp',
          render: formatTimestamp,
          width: 170,
        },
        {
          title: 'Codes',
          dataIndex: 'codes',
          render: (codes: string[]) => (
            <Space size={[4, 4]} wrap>
              {codes.map((c) => (
                <Tag color="red" key={c}>
                  {c}
                </Tag>
              ))}
            </Space>
          ),
        },
        {
          title: 'In/Out',
          width: 110,
          render: (_, r) => (
            <Space size={4}>
              <Tag color={r.input_pass === false ? 'red' : r.input_pass ? 'green' : 'default'}>
                in: {r.input_pass === null ? '—' : r.input_pass ? 'pass' : 'fail'}
              </Tag>
              <Tag color={r.output_pass === false ? 'red' : r.output_pass ? 'green' : 'default'}>
                out: {r.output_pass === null ? '—' : r.output_pass ? 'pass' : 'fail'}
              </Tag>
            </Space>
          ),
        },
        {
          title: 'Query (preview)',
          dataIndex: 'query_preview',
          ellipsis: true,
        },
        {
          title: 'Workspace',
          dataIndex: 'workspace_id',
          width: 130,
          render: (w: string | null) => w || '—',
        },
        {
          title: 'Response ID',
          dataIndex: 'response_id',
          width: 220,
          render: (r: string) => <Typography.Text code>{r.slice(0, 8)}…</Typography.Text>,
        },
      ]}
    />
  );
}

// ── Policy Preview tab ────────────────────────────────────────────

function PreviewTab() {
  const [query, setQuery] = useState('');
  const [response, setResponse] = useState('');
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<PreviewResponse | null>(null);

  async function run() {
    if (!query && !response) {
      message.warning('Provide a query or response to preview');
      return;
    }
    setLoading(true);
    try {
      const r = await previewGuardrails({
        query: query || undefined,
        response: response || undefined,
      });
      setResult(r);
    } catch {
      message.error('Preview failed');
    } finally {
      setLoading(false);
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Alert
        type="info"
        showIcon
        message="Test your guardrail policy"
        description="Paste a sample user query and/or model response to see what your effective policy would do. Nothing is recorded."
      />
      <Card size="small" title="Sample input">
        <Space direction="vertical" size="small" style={{ width: '100%' }}>
          <Typography.Text type="secondary">User query (input guard)</Typography.Text>
          <Input.TextArea
            rows={3}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="e.g. My ID is 1101700230708 — please remember it"
          />
          <Typography.Text type="secondary">Model response (output guard)</Typography.Text>
          <Input.TextArea
            rows={3}
            value={response}
            onChange={(e) => setResponse(e.target.value)}
            placeholder="e.g. Sure — your card 4242 4242 4242 4242 is on file."
          />
          <Space>
            <Button type="primary" loading={loading} onClick={run}>
              Run preview
            </Button>
            <Button
              onClick={() => {
                setQuery('');
                setResponse('');
                setResult(null);
              }}
            >
              Clear
            </Button>
          </Space>
        </Space>
      </Card>

      {result && (
        <Row gutter={[16, 16]}>
          <Col xs={24} md={12}>
            <Card size="small" title="Input verdict">
              {result.input ? (
                <Space direction="vertical" size="small">
                  <div>{actionTag(result.input.action)}</div>
                  <div>
                    <Typography.Text type="secondary">Codes: </Typography.Text>
                    {result.input.codes.length === 0 ? (
                      <Tag>none</Tag>
                    ) : (
                      result.input.codes.map((c) => (
                        <Tag color="red" key={c}>
                          {c}
                        </Tag>
                      ))
                    )}
                  </div>
                  {result.input.output && (
                    <Card type="inner" size="small" title="Resulting text / refusal">
                      <Typography.Paragraph style={{ margin: 0 }}>
                        {result.input.output}
                      </Typography.Paragraph>
                    </Card>
                  )}
                </Space>
              ) : (
                <Empty description="No query supplied" />
              )}
            </Card>
          </Col>
          <Col xs={24} md={12}>
            <Card size="small" title="Output verdict">
              {result.output ? (
                <Space direction="vertical" size="small">
                  <div>{actionTag(result.output.action)}</div>
                  <div>
                    <Typography.Text type="secondary">Codes: </Typography.Text>
                    {result.output.codes.length === 0 ? (
                      <Tag>none</Tag>
                    ) : (
                      result.output.codes.map((c) => (
                        <Tag color="red" key={c}>
                          {c}
                        </Tag>
                      ))
                    )}
                  </div>
                  {result.output.output && (
                    <Card type="inner" size="small" title="Resulting text / refusal">
                      <Typography.Paragraph style={{ margin: 0 }}>
                        {result.output.output}
                      </Typography.Paragraph>
                    </Card>
                  )}
                </Space>
              ) : (
                <Empty description="No response supplied" />
              )}
            </Card>
          </Col>
        </Row>
      )}
    </Space>
  );
}

// ── Page shell ───────────────────────────────────────────────────

export default function GuardrailsPage() {
  const [range, setRange] = useState<[Dayjs, Dayjs] | null>(null);
  const from = range?.[0]?.toISOString();
  const to = range?.[1]?.toISOString();

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space align="center">
        <Typography.Title level={4} style={{ margin: 0 }}>
          <SafetyOutlined /> Guardrails
        </Typography.Title>
        <RangePicker
          showTime
          value={range as [Dayjs, Dayjs] | null}
          onChange={(v) => setRange(v as [Dayjs, Dayjs] | null)}
          allowClear
        />
      </Space>
      <Tabs
        defaultActiveKey="dashboard"
        items={[
          {
            key: 'dashboard',
            label: (
              <span>
                <DashboardOutlined /> Dashboard
              </span>
            ),
            children: <DashboardTab from={from} to={to} />,
          },
          {
            key: 'violations',
            label: (
              <span>
                <FileTextOutlined /> Violation Log
              </span>
            ),
            children: <ViolationLogTab from={from} to={to} />,
          },
          {
            key: 'preview',
            label: (
              <span>
                <ExperimentOutlined /> Policy Preview
              </span>
            ),
            children: <PreviewTab />,
          },
        ]}
      />
    </Space>
  );
}
