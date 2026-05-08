import { useState, useMemo } from 'react';
import {
  Typography,
  Tabs,
  Card,
  Row,
  Col,
  Statistic,
  Table,
  Tag,
  Button,
  Space,
  Tooltip,
  Popconfirm,
  Input,
  Select,
  DatePicker,
  Descriptions,
  Alert,
  Empty,
  Spin,
  message,
} from 'antd';
import {
  CheckCircleOutlined,
  CloseCircleOutlined,
  DashboardOutlined,
  SearchOutlined,
  CloudDownloadOutlined,
  DeleteOutlined,
  QuestionCircleOutlined,
  LikeOutlined,
  DislikeOutlined,
  FieldTimeOutlined,
  SafetyCertificateOutlined,
  RobotOutlined,
  ThunderboltOutlined,
  DatabaseOutlined,
  FileTextOutlined,
  BarChartOutlined,
  ExperimentOutlined,
  CustomerServiceOutlined,
  ToolOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  useInferenceLogs,
  useInferenceAnalytics,
  useDeleteInferenceLogs,
  useExportInferenceLogs,
} from '../hooks/useInferenceLogs';
import type {
  InferenceLogEntry,
  InferenceLogFilter,
  InferenceStats,
  ModelStats,
} from '../api/types';

const { RangePicker } = DatePicker;

// ── Helpers ───────────────────────────────────────────────────────

function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function rateColor(rate: number): string {
  if (rate >= 0.95) return '#52c41a';
  if (rate >= 0.8) return '#faad14';
  return '#cf1322';
}

function scoreColor(score: number): string {
  if (score >= 0.8) return '#52c41a';
  if (score >= 0.5) return '#faad14';
  return '#cf1322';
}

function inferenceLogsToCsv(entries: InferenceLogEntry[]): string {
  const headers = [
    'id', 'timestamp', 'user_id', 'workspace_id', 'org_id', 'dept_id',
    'session_id', 'response_id', 'query_text', 'detected_language', 'intent',
    'complexity', 'llm_kind', 'llm_model', 'settings_scope', 'prompt_tokens',
    'completion_tokens', 'total_ms', 'search_ms', 'generation_ms',
    'chunks_retrieved', 'avg_chunk_score', 'self_rag_decision',
    'self_rag_confidence', 'quality_guard_pass', 'relevance_score',
    'hallucination_score', 'completeness_score', 'pipeline_route',
    'agents_used', 'status', 'error_message', 'response_length', 'feedback_score',
    'input_guardrails_pass', 'output_guardrails_pass', 'guardrail_violation_codes',
  ] as const;
  const rows = entries.map(e =>
    headers.map(h => {
      const val = e[h as keyof InferenceLogEntry];
      if (val === null || val === undefined) return '';
      const str = String(val);
      return str.includes(',') || str.includes('"') || str.includes('\n')
        ? `"${str.replace(/"/g, '""')}"` : str;
    }).join(','),
  );
  return [headers.join(','), ...rows].join('\n');
}

function downloadBlob(content: string, filename: string, mime: string) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

// ── Main Page ─────────────────────────────────────────────────────

export default function InferenceLogsPage() {
  return (
    <>
      <Space align="baseline" style={{ marginBottom: 16 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          Inference Logs
        </Typography.Title>
        <Tooltip title="Per-request inference logs for compliance, audit, performance analysis, and support investigation. Captures model, tokens, timing, quality scores, and pipeline details for every query.">
          <QuestionCircleOutlined style={{ fontSize: 16 }} />
        </Tooltip>
      </Space>

      <Tabs
        defaultActiveKey="dashboard"
        items={[
          { key: 'dashboard', label: <span><DashboardOutlined /> Dashboard</span>, children: <DashboardTab /> },
          { key: 'logs', label: <span><FileTextOutlined /> Log Browser</span>, children: <LogBrowserTab /> },
          { key: 'models', label: <span><BarChartOutlined /> Model Breakdown</span>, children: <ModelBreakdownTab /> },
          { key: 'support', label: <span><CustomerServiceOutlined /> Support</span>, children: <SupportTab /> },
          { key: 'management', label: <span><ToolOutlined /> Management</span>, children: <ManagementTab /> },
        ]}
      />
    </>
  );
}

// ── Tab 1: Dashboard ──────────────────────────────────────────────

function DashboardTab() {
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);

  const filter = useMemo(() => {
    const f: Partial<InferenceLogFilter> = {};
    if (dateRange) {
      f.from = dateRange[0].toISOString();
      f.to = dateRange[1].toISOString();
    }
    return f;
  }, [dateRange]);

  const { data: stats, isLoading } = useInferenceAnalytics(filter);

  if (isLoading) return <Spin />;
  if (!stats) return <Empty description="No inference log data" />;

  return (
    <>
      <div style={{ marginBottom: 16 }}>
        <Space>
          <Typography.Text strong>Date Range:</Typography.Text>
          <RangePicker
            value={dateRange}
            onChange={(v) => setDateRange(v as [Dayjs, Dayjs] | null)}
            presets={[
              { label: 'Today', value: [dayjs().startOf('day'), dayjs()] },
              { label: 'Last 7 Days', value: [dayjs().subtract(7, 'day'), dayjs()] },
              { label: 'Last 30 Days', value: [dayjs().subtract(30, 'day'), dayjs()] },
              { label: 'Last 90 Days', value: [dayjs().subtract(90, 'day'), dayjs()] },
            ]}
          />
        </Space>
      </div>

      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          <Card><Statistic title="Total Requests" value={stats.total_requests} prefix={<ThunderboltOutlined />} /></Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={<Tooltip title="Percentage of requests that completed without error"><span>Success Rate <QuestionCircleOutlined /></span></Tooltip>}
              value={`${(stats.success_rate * 100).toFixed(1)}%`}
              valueStyle={{ color: rateColor(stats.success_rate) }}
              prefix={<SafetyCertificateOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={<Tooltip title="Average end-to-end response time"><span>Avg Response Time <QuestionCircleOutlined /></span></Tooltip>}
              value={stats.avg_total_ms.toFixed(0)}
              suffix="ms"
              prefix={<FieldTimeOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={<Tooltip title="Average relevance score from quality guard"><span>Avg Relevance <QuestionCircleOutlined /></span></Tooltip>}
              value={stats.avg_relevance_score.toFixed(3)}
              valueStyle={{ color: scoreColor(stats.avg_relevance_score) }}
              prefix={<ExperimentOutlined />}
            />
          </Card>
        </Col>
      </Row>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col xs={24} sm={12} lg={6}>
          <Card><Statistic title="Total Prompt Tokens" value={formatTokenCount(stats.total_prompt_tokens)} prefix={<RobotOutlined />} /></Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card><Statistic title="Total Completion Tokens" value={formatTokenCount(stats.total_completion_tokens)} prefix={<RobotOutlined />} /></Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={<Tooltip title="Percentage of responses that passed quality guard"><span>Quality Pass Rate <QuestionCircleOutlined /></span></Tooltip>}
              value={`${(stats.quality_pass_rate * 100).toFixed(1)}%`}
              valueStyle={{ color: rateColor(stats.quality_pass_rate) }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={<Tooltip title="Percentage of positive user feedback (thumbs up)"><span>Feedback Positive Rate <QuestionCircleOutlined /></span></Tooltip>}
              value={`${(stats.feedback_positive_rate * 100).toFixed(1)}%`}
              valueStyle={{ color: rateColor(stats.feedback_positive_rate) }}
              prefix={<LikeOutlined />}
            />
          </Card>
        </Col>
      </Row>

      {stats.by_model.length > 0 && (
        <Card title="Model Performance" style={{ marginTop: 16 }}>
          <Table<ModelStats>
            dataSource={stats.by_model}
            rowKey="model"
            pagination={false}
            size="small"
            columns={[
              { title: 'Model', dataIndex: 'model', render: (v: string) => <Tag color="blue">{v}</Tag> },
              { title: 'Requests', dataIndex: 'count', sorter: (a, b) => a.count - b.count },
              { title: 'Avg Latency (ms)', dataIndex: 'avg_ms', render: (v: number) => v.toFixed(0), sorter: (a, b) => a.avg_ms - b.avg_ms },
              { title: 'Avg Quality', dataIndex: 'avg_quality', render: (v: number) => <span style={{ color: scoreColor(v) }}>{v.toFixed(3)}</span>, sorter: (a, b) => a.avg_quality - b.avg_quality },
              { title: 'Total Tokens', dataIndex: 'total_tokens', render: (v: number) => formatTokenCount(v), sorter: (a, b) => a.total_tokens - b.total_tokens },
              { title: 'Tokens/Request', render: (_: unknown, r: ModelStats) => r.count > 0 ? formatTokenCount(Math.round(r.total_tokens / r.count)) : '-' },
            ]}
          />
        </Card>
      )}

      {stats.by_workspace.length > 0 && (
        <Card title="Workspace Activity" style={{ marginTop: 16 }}>
          <Table
            dataSource={stats.by_workspace}
            rowKey="workspace_id"
            pagination={false}
            size="small"
            columns={[
              { title: 'Workspace ID', dataIndex: 'workspace_id', ellipsis: true },
              { title: 'Requests', dataIndex: 'count', sorter: (a: { count: number }, b: { count: number }) => a.count - b.count },
              { title: 'Avg Latency (ms)', dataIndex: 'avg_ms', render: (v: number) => v.toFixed(0) },
              { title: 'Total Tokens', dataIndex: 'total_tokens', render: (v: number) => formatTokenCount(v) },
            ]}
          />
        </Card>
      )}
    </>
  );
}

// ── Tab 2: Log Browser ────────────────────────────────────────────

function LogBrowserTab() {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);
  const [workspace, setWorkspace] = useState<string | undefined>();
  const [userId, setUserId] = useState<string | undefined>();
  const [model, setModel] = useState<string | undefined>();
  const [status, setStatus] = useState<string | undefined>();
  const [intent, setIntent] = useState<string | undefined>();

  const filter: InferenceLogFilter = useMemo(() => ({
    workspace_id: workspace,
    user_id: userId,
    llm_model: model,
    status,
    intent,
    from: dateRange?.[0]?.toISOString(),
    to: dateRange?.[1]?.toISOString(),
    limit: pageSize,
    offset: (page - 1) * pageSize,
  }), [workspace, userId, model, status, intent, dateRange, page, pageSize]);

  const { data, isLoading } = useInferenceLogs(filter);

  const columns = [
    {
      title: 'Time',
      dataIndex: 'timestamp',
      width: 170,
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: 'Status',
      dataIndex: 'status',
      width: 90,
      render: (v: string) => v === 'success'
        ? <Tag color="success" icon={<CheckCircleOutlined />}>OK</Tag>
        : <Tag color="error" icon={<CloseCircleOutlined />}>Error</Tag>,
    },
    {
      title: 'Model',
      dataIndex: 'llm_model',
      width: 180,
      ellipsis: true,
      render: (v: string) => <Tag color="blue">{v}</Tag>,
    },
    {
      title: 'Query',
      dataIndex: 'query_text',
      ellipsis: true,
    },
    {
      title: 'Tokens',
      width: 100,
      render: (_: unknown, r: InferenceLogEntry) => formatTokenCount(r.prompt_tokens + r.completion_tokens),
    },
    {
      title: 'Latency',
      dataIndex: 'total_ms',
      width: 90,
      render: (v: number) => `${v}ms`,
    },
    {
      title: 'Relevance',
      dataIndex: 'relevance_score',
      width: 90,
      render: (v: number | null) => v != null
        ? <span style={{ color: scoreColor(v) }}>{v.toFixed(3)}</span>
        : <Typography.Text type="secondary">-</Typography.Text>,
    },
    {
      title: 'Feedback',
      dataIndex: 'feedback_score',
      width: 80,
      render: (v: number | null) => {
        if (v === 1) return <Tag color="green" icon={<LikeOutlined />}>+1</Tag>;
        if (v === -1) return <Tag color="red" icon={<DislikeOutlined />}>-1</Tag>;
        return <Typography.Text type="secondary">-</Typography.Text>;
      },
    },
    {
      title: 'Guardrails',
      width: 140,
      render: (_: unknown, r: InferenceLogEntry) => {
        const codes = (r.guardrail_violation_codes || '').split(',').filter(Boolean);
        if (codes.length === 0) {
          if (r.input_guardrails_pass == null && r.output_guardrails_pass == null) {
            return <Typography.Text type="secondary">-</Typography.Text>;
          }
          return <Tag color="success">clean</Tag>;
        }
        return (
          <Tooltip title={codes.join(', ')}>
            <Tag color="red">{codes.length} violation{codes.length === 1 ? '' : 's'}</Tag>
          </Tooltip>
        );
      },
    },
  ];

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space wrap>
          <RangePicker
            value={dateRange}
            onChange={(v) => { setDateRange(v as [Dayjs, Dayjs] | null); setPage(1); }}
            placeholder={['From', 'To']}
            size="small"
          />
          <Input
            placeholder="Workspace ID"
            value={workspace}
            onChange={(e) => { setWorkspace(e.target.value || undefined); setPage(1); }}
            allowClear
            size="small"
            style={{ width: 160 }}
          />
          <Input
            placeholder="User ID"
            value={userId}
            onChange={(e) => { setUserId(e.target.value || undefined); setPage(1); }}
            allowClear
            size="small"
            style={{ width: 160 }}
          />
          <Input
            placeholder="Model"
            value={model}
            onChange={(e) => { setModel(e.target.value || undefined); setPage(1); }}
            allowClear
            size="small"
            style={{ width: 160 }}
          />
          <Select
            placeholder="Status"
            value={status}
            onChange={(v) => { setStatus(v); setPage(1); }}
            allowClear
            size="small"
            style={{ width: 110 }}
            options={[
              { label: 'Success', value: 'success' },
              { label: 'Error', value: 'error' },
            ]}
          />
          <Select
            placeholder="Intent"
            value={intent}
            onChange={(v) => { setIntent(v); setPage(1); }}
            allowClear
            size="small"
            style={{ width: 140 }}
            options={[
              { label: 'Retrieval', value: 'Retrieval' },
              { label: 'DirectAnswer', value: 'DirectAnswer' },
              { label: 'Greeting', value: 'Greeting' },
              { label: 'Clarification', value: 'Clarification' },
              { label: 'Comparison', value: 'Comparison' },
              { label: 'Summarization', value: 'Summarization' },
            ]}
          />
        </Space>
      </Card>

      <Table<InferenceLogEntry>
        dataSource={data?.entries ?? []}
        columns={columns}
        rowKey="id"
        loading={isLoading}
        pagination={{
          current: page,
          pageSize,
          total: data?.total ?? 0,
          onChange: (p, ps) => { setPage(p); setPageSize(ps); },
          showTotal: (t) => `${t} logs`,
          showSizeChanger: true,
          pageSizeOptions: ['10', '20', '50', '100'],
        }}
        expandable={{
          expandedRowRender: (record) => <LogEntryDetail entry={record} />,
        }}
        size="small"
        scroll={{ x: 900 }}
      />
    </>
  );
}

function LogEntryDetail({ entry }: { entry: InferenceLogEntry }) {
  let agentsParsed: { name: string; ms?: number }[] = [];
  try {
    agentsParsed = JSON.parse(entry.agents_used);
  } catch { /* ignore */ }

  return (
    <div style={{ padding: 8 }}>
      {entry.error_message && (
        <Alert type="error" message={entry.error_message} style={{ marginBottom: 12 }} />
      )}
      <Row gutter={16}>
        <Col xs={24} md={12}>
          <Descriptions column={1} size="small" bordered title="Identifiers">
            <Descriptions.Item label="Log ID">{entry.id}</Descriptions.Item>
            <Descriptions.Item label="Response ID">{entry.response_id}</Descriptions.Item>
            <Descriptions.Item label="Session ID">{entry.session_id ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="User ID">{entry.user_id ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Workspace">{entry.workspace_id ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Org">{entry.org_id ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Dept">{entry.dept_id ?? '-'}</Descriptions.Item>
          </Descriptions>

          <Descriptions column={1} size="small" bordered title="Query" style={{ marginTop: 12 }}>
            <Descriptions.Item label="Text">
              <div style={{ whiteSpace: 'pre-wrap', maxHeight: 120, overflowY: 'auto', fontSize: 13 }}>
                {entry.query_text}
              </div>
            </Descriptions.Item>
            <Descriptions.Item label="Language">{entry.detected_language ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Intent"><Tag>{entry.intent ?? '-'}</Tag></Descriptions.Item>
            <Descriptions.Item label="Complexity"><Tag>{entry.complexity ?? '-'}</Tag></Descriptions.Item>
          </Descriptions>
        </Col>

        <Col xs={24} md={12}>
          <Descriptions column={1} size="small" bordered title="Model & Timing">
            <Descriptions.Item label="Provider">{entry.llm_kind}</Descriptions.Item>
            <Descriptions.Item label="Model"><Tag color="blue">{entry.llm_model}</Tag></Descriptions.Item>
            <Descriptions.Item label="Scope">{entry.settings_scope}</Descriptions.Item>
            <Descriptions.Item label="Total Time">{entry.total_ms}ms</Descriptions.Item>
            <Descriptions.Item label="Search Time">{entry.search_ms != null ? `${entry.search_ms}ms` : '-'}</Descriptions.Item>
            <Descriptions.Item label="Generation Time">{entry.generation_ms != null ? `${entry.generation_ms}ms` : '-'}</Descriptions.Item>
            <Descriptions.Item label="Prompt Tokens">{entry.prompt_tokens}</Descriptions.Item>
            <Descriptions.Item label="Completion Tokens">{entry.completion_tokens}</Descriptions.Item>
          </Descriptions>

          <Descriptions column={1} size="small" bordered title="Search & Quality" style={{ marginTop: 12 }}>
            <Descriptions.Item label="Chunks Retrieved">{entry.chunks_retrieved ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Avg Chunk Score">{entry.avg_chunk_score?.toFixed(4) ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Self-RAG">{entry.self_rag_decision ?? '-'} {entry.self_rag_confidence != null ? `(${entry.self_rag_confidence.toFixed(2)})` : ''}</Descriptions.Item>
            <Descriptions.Item label="Quality Guard">
              {entry.quality_guard_pass === true ? <Tag color="green">Pass</Tag> : entry.quality_guard_pass === false ? <Tag color="red">Fail</Tag> : '-'}
            </Descriptions.Item>
            <Descriptions.Item label="Relevance">{entry.relevance_score != null ? <span style={{ color: scoreColor(entry.relevance_score) }}>{entry.relevance_score.toFixed(3)}</span> : '-'}</Descriptions.Item>
            <Descriptions.Item label="Hallucination">{entry.hallucination_score?.toFixed(3) ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Completeness">{entry.completeness_score?.toFixed(3) ?? '-'}</Descriptions.Item>
          </Descriptions>

          <Descriptions column={1} size="small" bordered title="Pipeline" style={{ marginTop: 12 }}>
            <Descriptions.Item label="Route"><Tag>{entry.pipeline_route ?? '-'}</Tag></Descriptions.Item>
            <Descriptions.Item label="Response Length">{entry.response_length} chars</Descriptions.Item>
            <Descriptions.Item label="Feedback">
              {entry.feedback_score === 1 ? <Tag color="green" icon={<LikeOutlined />}>Positive</Tag>
                : entry.feedback_score === -1 ? <Tag color="red" icon={<DislikeOutlined />}>Negative</Tag>
                : '-'}
            </Descriptions.Item>
            {agentsParsed.length > 0 && (
              <Descriptions.Item label="Agents Used">
                <Space wrap>
                  {agentsParsed.map((a, i) => (
                    <Tag key={i}>{a.name}{a.ms != null ? ` (${a.ms}ms)` : ''}</Tag>
                  ))}
                </Space>
              </Descriptions.Item>
            )}
          </Descriptions>
        </Col>
      </Row>
    </div>
  );
}

// ── Tab 3: Model Breakdown ────────────────────────────────────────

function ModelBreakdownTab() {
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);
  const filter = useMemo(() => {
    const f: Partial<InferenceLogFilter> = {};
    if (dateRange) {
      f.from = dateRange[0].toISOString();
      f.to = dateRange[1].toISOString();
    }
    return f;
  }, [dateRange]);

  const { data: stats, isLoading } = useInferenceAnalytics(filter);

  if (isLoading) return <Spin />;
  if (!stats || stats.by_model.length === 0) return <Empty description="No model data" />;

  const models = stats.by_model;
  const bestQuality = models.reduce((a, b) => a.avg_quality > b.avg_quality ? a : b);
  const mostUsed = models.reduce((a, b) => a.count > b.count ? a : b);
  const highestTokens = models.reduce((a, b) => a.total_tokens > b.total_tokens ? a : b);

  return (
    <>
      <div style={{ marginBottom: 16 }}>
        <Space>
          <Typography.Text strong>Date Range:</Typography.Text>
          <RangePicker
            value={dateRange}
            onChange={(v) => setDateRange(v as [Dayjs, Dayjs] | null)}
            presets={[
              { label: 'Last 7 Days', value: [dayjs().subtract(7, 'day'), dayjs()] },
              { label: 'Last 30 Days', value: [dayjs().subtract(30, 'day'), dayjs()] },
            ]}
          />
        </Space>
      </div>

      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          <Card><Statistic title="Active Models" value={models.length} prefix={<RobotOutlined />} /></Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={<Tooltip title="Model with highest average quality score"><span>Best Quality <QuestionCircleOutlined /></span></Tooltip>}
              value={bestQuality.model}
              valueStyle={{ fontSize: 16 }}
            />
            <Tag color="green">{bestQuality.avg_quality.toFixed(3)}</Tag>
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Most Used"
              value={mostUsed.model}
              valueStyle={{ fontSize: 16 }}
            />
            <Tag color="blue">{mostUsed.count} requests</Tag>
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Highest Token Usage"
              value={highestTokens.model}
              valueStyle={{ fontSize: 16 }}
            />
            <Tag color="orange">{formatTokenCount(highestTokens.total_tokens)}</Tag>
          </Card>
        </Col>
      </Row>

      <Card title="Model Comparison" style={{ marginTop: 16 }}>
        <Table<ModelStats>
          dataSource={models}
          rowKey="model"
          pagination={false}
          columns={[
            { title: 'Model', dataIndex: 'model', render: (v: string) => <Tag color="blue">{v}</Tag> },
            { title: 'Requests', dataIndex: 'count', sorter: (a, b) => a.count - b.count, defaultSortOrder: 'descend' },
            { title: 'Avg Latency (ms)', dataIndex: 'avg_ms', render: (v: number) => v.toFixed(0), sorter: (a, b) => a.avg_ms - b.avg_ms },
            {
              title: 'Avg Quality',
              dataIndex: 'avg_quality',
              render: (v: number) => <span style={{ color: scoreColor(v) }}>{v.toFixed(3)}</span>,
              sorter: (a, b) => a.avg_quality - b.avg_quality,
            },
            { title: 'Total Tokens', dataIndex: 'total_tokens', render: (v: number) => formatTokenCount(v), sorter: (a, b) => a.total_tokens - b.total_tokens },
            {
              title: 'Tokens/Request',
              render: (_: unknown, r: ModelStats) => r.count > 0 ? formatTokenCount(Math.round(r.total_tokens / r.count)) : '-',
              sorter: (a, b) => (a.count > 0 ? a.total_tokens / a.count : 0) - (b.count > 0 ? b.total_tokens / b.count : 0),
            },
          ]}
        />
      </Card>
    </>
  );
}

// ── Tab 4: Support ────────────────────────────────────────────────

function SupportTab() {
  const [searchType, setSearchType] = useState<'response_id' | 'session_id'>('response_id');
  const [searchValue, setSearchValue] = useState('');
  const [activeSearch, setActiveSearch] = useState<InferenceLogFilter | null>(null);

  const { data, isLoading } = useInferenceLogs(activeSearch ?? { limit: 0 });

  const handleSearch = () => {
    if (!searchValue.trim()) return;
    setActiveSearch({
      [searchType]: searchValue.trim(),
      limit: 100,
    });
  };

  const entries = activeSearch ? (data?.entries ?? []) : [];

  return (
    <>
      <Card title="Investigate Request" size="small" style={{ marginBottom: 16 }}>
        <Space wrap>
          <Select
            value={searchType}
            onChange={(v) => { setSearchType(v); setActiveSearch(null); }}
            style={{ width: 150 }}
            options={[
              { label: 'Response ID', value: 'response_id' },
              { label: 'Session ID', value: 'session_id' },
            ]}
          />
          <Input
            placeholder={searchType === 'response_id' ? 'Enter response ID...' : 'Enter session ID...'}
            value={searchValue}
            onChange={(e) => setSearchValue(e.target.value)}
            onPressEnter={handleSearch}
            style={{ width: 360 }}
            allowClear
          />
          <Button type="primary" icon={<SearchOutlined />} onClick={handleSearch} loading={isLoading}>
            Search
          </Button>
        </Space>
      </Card>

      {isLoading && <Spin />}

      {!isLoading && activeSearch && entries.length === 0 && (
        <Empty description={`No logs found for ${searchType}: ${searchValue}`} />
      )}

      {entries.length === 1 && (
        <Card title="Request Detail">
          <LogEntryDetail entry={entries[0]} />
        </Card>
      )}

      {entries.length > 1 && (
        <>
          <Alert
            type="info"
            message={`Found ${entries.length} logs in session`}
            style={{ marginBottom: 12 }}
          />
          <Table<InferenceLogEntry>
            dataSource={entries}
            rowKey="id"
            size="small"
            pagination={false}
            columns={[
              { title: 'Time', dataIndex: 'timestamp', width: 170, render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm:ss') },
              { title: 'Status', dataIndex: 'status', width: 80, render: (v: string) => v === 'success' ? <Tag color="success">OK</Tag> : <Tag color="error">Error</Tag> },
              { title: 'Model', dataIndex: 'llm_model', width: 160, render: (v: string) => <Tag color="blue">{v}</Tag> },
              { title: 'Query', dataIndex: 'query_text', ellipsis: true },
              { title: 'Latency', dataIndex: 'total_ms', width: 90, render: (v: number) => `${v}ms` },
              { title: 'Tokens', width: 80, render: (_: unknown, r: InferenceLogEntry) => r.prompt_tokens + r.completion_tokens },
            ]}
            expandable={{ expandedRowRender: (record) => <LogEntryDetail entry={record} /> }}
          />
        </>
      )}
    </>
  );
}

// ── Tab 5: Management ─────────────────────────────────────────────

function ManagementTab() {
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);
  const [workspace, setWorkspace] = useState<string | undefined>();
  const [model, setModel] = useState<string | undefined>();
  const [status, setStatus] = useState<string | undefined>();
  const [matchCount, setMatchCount] = useState<number | null>(null);

  const exportMutation = useExportInferenceLogs();
  const deleteMutation = useDeleteInferenceLogs();

  const { data: stats } = useInferenceAnalytics();

  // Get oldest/newest log
  const { data: oldestData } = useInferenceLogs({ limit: 1, offset: 0 });
  const { data: newestData } = useInferenceLogs({ limit: 1, offset: 0 });

  const buildFilter = (): Partial<InferenceLogFilter> => ({
    workspace_id: workspace,
    llm_model: model,
    status,
    from: dateRange?.[0]?.toISOString(),
    to: dateRange?.[1]?.toISOString(),
  });

  const handleCountMatch = async () => {
    try {
      const result = await exportMutation.mutateAsync(buildFilter());
      setMatchCount(result.length);
    } catch {
      message.error('Failed to count matching logs');
    }
  };

  const handleExportCsv = async () => {
    try {
      const entries = await exportMutation.mutateAsync(buildFilter());
      if (entries.length === 0) {
        message.warning('No logs match the current filter');
        return;
      }
      const csv = inferenceLogsToCsv(entries);
      const timestamp = dayjs().format('YYYYMMDD-HHmmss');
      downloadBlob(csv, `inference-logs-${timestamp}.csv`, 'text/csv');
      message.success(`Exported ${entries.length} logs as CSV`);
    } catch {
      message.error('Export failed');
    }
  };

  const handleExportJson = async () => {
    try {
      const entries = await exportMutation.mutateAsync(buildFilter());
      if (entries.length === 0) {
        message.warning('No logs match the current filter');
        return;
      }
      const json = JSON.stringify(entries, null, 2);
      const timestamp = dayjs().format('YYYYMMDD-HHmmss');
      downloadBlob(json, `inference-logs-${timestamp}.json`, 'application/json');
      message.success(`Exported ${entries.length} logs as JSON`);
    } catch {
      message.error('Export failed');
    }
  };

  const handlePurge = async () => {
    try {
      const result = await deleteMutation.mutateAsync(buildFilter());
      message.success(`Purged ${result.deleted} inference logs`);
      setMatchCount(null);
    } catch {
      message.error('Purge failed');
    }
  };

  const filterControls = (
    <Space wrap>
      <RangePicker
        value={dateRange}
        onChange={(v) => { setDateRange(v as [Dayjs, Dayjs] | null); setMatchCount(null); }}
        placeholder={['From', 'To']}
        size="small"
        presets={[
          { label: 'Older than 30 days', value: [dayjs('2020-01-01'), dayjs().subtract(30, 'day')] },
          { label: 'Older than 90 days', value: [dayjs('2020-01-01'), dayjs().subtract(90, 'day')] },
          { label: 'Older than 1 year', value: [dayjs('2020-01-01'), dayjs().subtract(1, 'year')] },
        ]}
      />
      <Input placeholder="Workspace ID" value={workspace} onChange={(e) => { setWorkspace(e.target.value || undefined); setMatchCount(null); }} allowClear size="small" style={{ width: 160 }} />
      <Input placeholder="Model" value={model} onChange={(e) => { setModel(e.target.value || undefined); setMatchCount(null); }} allowClear size="small" style={{ width: 160 }} />
      <Select placeholder="Status" value={status} onChange={(v) => { setStatus(v); setMatchCount(null); }} allowClear size="small" style={{ width: 110 }} options={[{ label: 'Success', value: 'success' }, { label: 'Error', value: 'error' }]} />
    </Space>
  );

  return (
    <>
      {/* Retention Info */}
      <Card title={<span><DatabaseOutlined /> Retention Info</span>} style={{ marginBottom: 16 }}>
        <Row gutter={16}>
          <Col xs={12} sm={6}>
            <Statistic title="Total Logs" value={stats?.total_requests ?? 0} />
          </Col>
          <Col xs={12} sm={6}>
            <Statistic title="Max Retention" value="50,000" suffix="logs" />
          </Col>
          <Col xs={12} sm={6}>
            <Statistic
              title="Oldest Log"
              value={oldestData?.entries?.[0]?.timestamp ? dayjs(oldestData.entries[0].timestamp).format('YYYY-MM-DD') : '-'}
            />
          </Col>
          <Col xs={12} sm={6}>
            <Statistic
              title="Newest Log"
              value={newestData?.entries?.[0]?.timestamp ? dayjs(newestData.entries[0].timestamp).format('YYYY-MM-DD') : '-'}
            />
          </Col>
        </Row>
        <Alert
          type="info"
          style={{ marginTop: 12 }}
          message="The system automatically prunes the oldest 10% when the 50,000-log limit is reached. For compliance, archive logs before they are pruned."
        />
      </Card>

      {/* Export */}
      <Card title={<span><CloudDownloadOutlined /> Export & Archive</span>} style={{ marginBottom: 16 }}>
        <Typography.Paragraph type="secondary">
          Export inference logs for archival, compliance reporting, or offline analysis. CSV format is compatible with Excel and data analysis tools. JSON preserves full data fidelity.
        </Typography.Paragraph>
        {filterControls}
        <div style={{ marginTop: 12 }}>
          <Space>
            <Button icon={<CloudDownloadOutlined />} onClick={handleExportCsv} loading={exportMutation.isPending}>
              Export as CSV
            </Button>
            <Button icon={<CloudDownloadOutlined />} onClick={handleExportJson} loading={exportMutation.isPending}>
              Export as JSON
            </Button>
          </Space>
        </div>
      </Card>

      {/* Purge */}
      <Card
        title={<span style={{ color: '#cf1322' }}><DeleteOutlined /> Purge Logs</span>}
        style={{ borderColor: '#ff4d4f' }}
      >
        <Alert
          type="warning"
          message="Purging permanently deletes inference logs. This action cannot be undone. Consider exporting/archiving logs first."
          style={{ marginBottom: 12 }}
        />
        {filterControls}
        <div style={{ marginTop: 12 }}>
          <Space>
            <Button onClick={handleCountMatch} loading={exportMutation.isPending}>
              Count Matching Logs
            </Button>
            {matchCount !== null && (
              <Tag color="orange" style={{ fontSize: 14, padding: '4px 8px' }}>
                {matchCount} logs match
              </Tag>
            )}
            <Popconfirm
              title="Permanently delete matching logs?"
              description={matchCount !== null ? `This will delete ${matchCount} inference logs. This cannot be undone.` : 'This will delete matching logs permanently.'}
              onConfirm={handlePurge}
              okText="Yes, Purge"
              okButtonProps={{ danger: true }}
            >
              <Button danger icon={<DeleteOutlined />} loading={deleteMutation.isPending}>
                Purge Logs
              </Button>
            </Popconfirm>
          </Space>
        </div>
      </Card>
    </>
  );
}
