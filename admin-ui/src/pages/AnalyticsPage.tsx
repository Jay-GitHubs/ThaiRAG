import { useState, useMemo } from 'react';
import {
  Typography,
  Card,
  Row,
  Col,
  Statistic,
  Table,
  Tag,
  Space,
  Tooltip,
  Select,
  Spin,
  Empty,
  Progress,
  theme,
} from 'antd';
import {
  ThunderboltOutlined,
  FieldTimeOutlined,
  RobotOutlined,
  UserOutlined,
  QuestionCircleOutlined,
  FireOutlined,
  ClockCircleOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { useInferenceAnalytics } from '../hooks/useInferenceLogs';
import type { InferenceLogEntry } from '../api/types';
import { useQuery } from '@tanstack/react-query';
import { exportInferenceLogs } from '../api/inferenceLogs';

// ── Helpers ────────────────────────────────────────────────────────

function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatMs(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms)}ms`;
}

/** Compute a percentile from sorted array of numbers. */
function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const idx = (p / 100) * (sorted.length - 1);
  const lower = Math.floor(idx);
  const upper = Math.ceil(idx);
  if (lower === upper) return sorted[lower];
  return sorted[lower] + (sorted[upper] - sorted[lower]) * (idx - lower);
}

type TimeRange = '24h' | '7d' | '30d';

function rangeToFilter(range: TimeRange): { from: string; to: string } {
  const now = dayjs();
  const map: Record<TimeRange, number> = { '24h': 1, '7d': 7, '30d': 30 };
  return {
    from: now.subtract(map[range], 'day').toISOString(),
    to: now.toISOString(),
  };
}

/** Group entries into time buckets. */
function bucketByTime(
  entries: InferenceLogEntry[],
  range: TimeRange,
): { label: string; count: number; promptTokens: number; completionTokens: number }[] {
  const useHours = range === '24h';
  const buckets = new Map<string, { count: number; promptTokens: number; completionTokens: number }>();

  for (const e of entries) {
    const d = dayjs(e.timestamp);
    const key = useHours ? d.format('HH:00') : d.format('MM-DD');
    const prev = buckets.get(key) ?? { count: 0, promptTokens: 0, completionTokens: 0 };
    prev.count += 1;
    prev.promptTokens += e.prompt_tokens;
    prev.completionTokens += e.completion_tokens;
    buckets.set(key, prev);
  }

  // Fill missing buckets
  const now = dayjs();
  const results: { label: string; count: number; promptTokens: number; completionTokens: number }[] = [];
  if (useHours) {
    for (let i = 23; i >= 0; i--) {
      const label = now.subtract(i, 'hour').format('HH:00');
      results.push({ label, ...(buckets.get(label) ?? { count: 0, promptTokens: 0, completionTokens: 0 }) });
    }
  } else {
    const days = range === '7d' ? 7 : 30;
    for (let i = days - 1; i >= 0; i--) {
      const label = now.subtract(i, 'day').format('MM-DD');
      results.push({ label, ...(buckets.get(label) ?? { count: 0, promptTokens: 0, completionTokens: 0 }) });
    }
  }
  return results;
}

/** Extract top query patterns (by lowercased first 80 chars). */
function topQueries(entries: InferenceLogEntry[], limit = 10): { query: string; count: number; avgMs: number }[] {
  const map = new Map<string, { count: number; totalMs: number; original: string }>();
  for (const e of entries) {
    const key = e.query_text.toLowerCase().slice(0, 80).trim();
    if (!key) continue;
    const prev = map.get(key) ?? { count: 0, totalMs: 0, original: e.query_text.slice(0, 80) };
    prev.count += 1;
    prev.totalMs += e.total_ms;
    map.set(key, prev);
  }
  return Array.from(map.values())
    .sort((a, b) => b.count - a.count)
    .slice(0, limit)
    .map((v) => ({ query: v.original, count: v.count, avgMs: v.totalMs / v.count }));
}

/** Count unique users in last 24h. */
function countActiveUsers(entries: InferenceLogEntry[]): number {
  const users = new Set<string>();
  const cutoff = dayjs().subtract(24, 'hour');
  for (const e of entries) {
    if (e.user_id && dayjs(e.timestamp).isAfter(cutoff)) {
      users.add(e.user_id);
    }
  }
  return users.size;
}

/** Group entries by intent. */
function countByIntent(entries: InferenceLogEntry[]): { intent: string; count: number }[] {
  const map = new Map<string, number>();
  for (const e of entries) {
    const intent = e.intent ?? 'Unknown';
    map.set(intent, (map.get(intent) ?? 0) + 1);
  }
  return Array.from(map.entries())
    .map(([intent, count]) => ({ intent, count }))
    .sort((a, b) => b.count - a.count);
}

// ── CSS Bar Chart Components ────────────────────────────────────────

function BarChart({
  data,
  labelKey,
  valueKey,
  color = '#1677ff',
  height = 200,
  suffix = '',
}: {
  data: Record<string, unknown>[];
  labelKey: string;
  valueKey: string;
  color?: string;
  height?: number;
  suffix?: string;
}) {
  const max = Math.max(...data.map((d) => Number(d[valueKey]) || 0), 1);
  const { token } = theme.useToken();

  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height, padding: '0 4px' }}>
      {data.map((d, i) => {
        const val = Number(d[valueKey]) || 0;
        const barHeight = (val / max) * (height - 24);
        const label = String(d[labelKey]);
        return (
          <Tooltip key={i} title={`${label}: ${val.toLocaleString()}${suffix}`}>
            <div
              style={{
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'flex-end',
                height: '100%',
              }}
            >
              <div
                style={{
                  width: '100%',
                  height: Math.max(barHeight, 1),
                  backgroundColor: color,
                  borderRadius: '2px 2px 0 0',
                  minWidth: 4,
                  transition: 'height 0.3s ease',
                }}
              />
              <div
                style={{
                  fontSize: 9,
                  color: token.colorTextSecondary,
                  marginTop: 2,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  maxWidth: '100%',
                  textAlign: 'center',
                }}
              >
                {data.length <= 14 ? label : i % Math.ceil(data.length / 10) === 0 ? label : ''}
              </div>
            </div>
          </Tooltip>
        );
      })}
    </div>
  );
}

function StackedBarChart({
  data,
  labelKey,
  keys,
  colors,
  height = 200,
}: {
  data: Record<string, unknown>[];
  labelKey: string;
  keys: string[];
  colors: string[];
  height?: number;
}) {
  const max = Math.max(
    ...data.map((d) => keys.reduce((sum, k) => sum + (Number(d[k]) || 0), 0)),
    1,
  );
  const { token } = theme.useToken();

  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height, padding: '0 4px' }}>
      {data.map((d, i) => {
        const total = keys.reduce((sum, k) => sum + (Number(d[k]) || 0), 0);
        const barHeight = (total / max) * (height - 24);
        const label = String(d[labelKey]);
        return (
          <Tooltip
            key={i}
            title={
              <div>
                <div>{label}</div>
                {keys.map((k) => (
                  <div key={k}>
                    {k}: {formatTokenCount(Number(d[k]) || 0)}
                  </div>
                ))}
              </div>
            }
          >
            <div
              style={{
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'flex-end',
                height: '100%',
              }}
            >
              <div style={{ width: '100%', display: 'flex', flexDirection: 'column-reverse', borderRadius: '2px 2px 0 0', overflow: 'hidden' }}>
                {keys.map((k, ki) => {
                  const val = Number(d[k]) || 0;
                  const segHeight = total > 0 ? (val / total) * Math.max(barHeight, 1) : 0;
                  return (
                    <div
                      key={k}
                      style={{
                        width: '100%',
                        height: segHeight,
                        backgroundColor: colors[ki],
                        minWidth: 4,
                      }}
                    />
                  );
                })}
              </div>
              <div
                style={{
                  fontSize: 9,
                  color: token.colorTextSecondary,
                  marginTop: 2,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  maxWidth: '100%',
                  textAlign: 'center',
                }}
              >
                {data.length <= 14 ? label : i % Math.ceil(data.length / 10) === 0 ? label : ''}
              </div>
            </div>
          </Tooltip>
        );
      })}
    </div>
  );
}

// ── Main Page ────────────────────────────────────────────────────────

export default function AnalyticsPage() {
  const [range, setRange] = useState<TimeRange>('7d');
  const filter = useMemo(() => rangeToFilter(range), [range]);

  // Fetch aggregate stats from analytics endpoint
  const { data: stats, isLoading: statsLoading } = useInferenceAnalytics(filter);

  // Fetch raw entries for client-side time-series and grouping
  const { data: entries, isLoading: entriesLoading } = useQuery({
    queryKey: ['analytics-entries', filter],
    queryFn: () => exportInferenceLogs(filter),
    staleTime: 60_000,
  });

  const isLoading = statsLoading || entriesLoading;

  // Derived data
  const timeBuckets = useMemo(
    () => (entries ? bucketByTime(entries, range) : []),
    [entries, range],
  );

  const latencyPercentiles = useMemo(() => {
    if (!entries || entries.length === 0) return null;
    const sorted = entries.map((e) => e.total_ms).sort((a, b) => a - b);
    return {
      p50: percentile(sorted, 50),
      p90: percentile(sorted, 90),
      p95: percentile(sorted, 95),
      p99: percentile(sorted, 99),
      min: sorted[0],
      max: sorted[sorted.length - 1],
    };
  }, [entries]);

  const topQueriesData = useMemo(
    () => (entries ? topQueries(entries) : []),
    [entries],
  );

  const intentData = useMemo(
    () => (entries ? countByIntent(entries) : []),
    [entries],
  );

  const activeUsers = useMemo(
    () => (entries ? countActiveUsers(entries) : 0),
    [entries],
  );

  if (isLoading) {
    return (
      <div style={{ textAlign: 'center', paddingTop: 80 }}>
        <Spin size="large" />
        <div style={{ marginTop: 16 }}>
          <Typography.Text type="secondary">Loading analytics data...</Typography.Text>
        </div>
      </div>
    );
  }

  if (!stats || !entries) {
    return <Empty description="No analytics data available" />;
  }

  return (
    <>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16, flexWrap: 'wrap', gap: 8 }}>
        <Space align="baseline">
          <Typography.Title level={4} style={{ margin: 0 }}>
            Analytics
          </Typography.Title>
          <Tooltip title="Query volume, latency trends, popular topics, and token usage derived from inference logs.">
            <QuestionCircleOutlined style={{ fontSize: 16 }} />
          </Tooltip>
        </Space>
        <Select
          value={range}
          onChange={setRange}
          style={{ width: 140 }}
          options={[
            { label: 'Last 24 Hours', value: '24h' },
            { label: 'Last 7 Days', value: '7d' },
            { label: 'Last 30 Days', value: '30d' },
          ]}
        />
      </div>

      {/* ── Top Stats Row ─────────────────────────────────────────── */}
      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Total Queries"
              value={stats.total_requests}
              prefix={<ThunderboltOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Avg Latency"
              value={stats.avg_total_ms.toFixed(0)}
              suffix="ms"
              prefix={<FieldTimeOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Total Tokens Used"
              value={formatTokenCount(stats.total_prompt_tokens + stats.total_completion_tokens)}
              prefix={<RobotOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={
                <Tooltip title="Unique users who made queries in the last 24 hours">
                  <span>Active Users (24h) <QuestionCircleOutlined /></span>
                </Tooltip>
              }
              value={activeUsers}
              prefix={<UserOutlined />}
            />
          </Card>
        </Col>
      </Row>

      {/* ── Query Volume Chart ────────────────────────────────────── */}
      <Card
        title={
          <Space>
            <FireOutlined />
            <span>Query Volume</span>
            <Tag>{range === '24h' ? 'Hourly' : 'Daily'}</Tag>
          </Space>
        }
        style={{ marginTop: 16 }}
      >
        {timeBuckets.length > 0 ? (
          <BarChart
            data={timeBuckets}
            labelKey="label"
            valueKey="count"
            color="#1677ff"
            height={220}
            suffix=" queries"
          />
        ) : (
          <Empty description="No query data" />
        )}
      </Card>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        {/* ── Latency Distribution ──────────────────────────────── */}
        <Col xs={24} lg={12}>
          <Card
            title={
              <Space>
                <ClockCircleOutlined />
                <span>Latency Percentiles</span>
              </Space>
            }
          >
            {latencyPercentiles ? (
              <div>
                <Row gutter={[16, 12]}>
                  {[
                    { label: 'p50 (Median)', value: latencyPercentiles.p50, color: '#52c41a' },
                    { label: 'p90', value: latencyPercentiles.p90, color: '#1677ff' },
                    { label: 'p95', value: latencyPercentiles.p95, color: '#faad14' },
                    { label: 'p99', value: latencyPercentiles.p99, color: '#cf1322' },
                  ].map((p) => (
                    <Col xs={12} sm={6} key={p.label}>
                      <Statistic
                        title={p.label}
                        value={formatMs(p.value)}
                        valueStyle={{ color: p.color, fontSize: 20 }}
                      />
                    </Col>
                  ))}
                </Row>
                <div style={{ marginTop: 20 }}>
                  {[
                    { label: 'p50', value: latencyPercentiles.p50, color: '#52c41a' },
                    { label: 'p90', value: latencyPercentiles.p90, color: '#1677ff' },
                    { label: 'p95', value: latencyPercentiles.p95, color: '#faad14' },
                    { label: 'p99', value: latencyPercentiles.p99, color: '#cf1322' },
                  ].map((p) => (
                    <div key={p.label} style={{ marginBottom: 8 }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                        <Typography.Text type="secondary">{p.label}</Typography.Text>
                        <Typography.Text>{formatMs(p.value)}</Typography.Text>
                      </div>
                      <Progress
                        percent={Math.min(100, (p.value / latencyPercentiles.max) * 100)}
                        showInfo={false}
                        strokeColor={p.color}
                        size="small"
                      />
                    </div>
                  ))}
                  <div style={{ marginTop: 8 }}>
                    <Typography.Text type="secondary">
                      Range: {formatMs(latencyPercentiles.min)} &ndash; {formatMs(latencyPercentiles.max)}
                    </Typography.Text>
                  </div>
                </div>
              </div>
            ) : (
              <Empty description="No latency data" />
            )}
          </Card>
        </Col>

        {/* ── Intent Distribution ──────────────────────────────── */}
        <Col xs={24} lg={12}>
          <Card title="Query Intent Distribution">
            {intentData.length > 0 ? (
              <div>
                {intentData.map((item) => {
                  const pct = entries.length > 0 ? (item.count / entries.length) * 100 : 0;
                  return (
                    <div key={item.intent} style={{ marginBottom: 10 }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                        <Tag>{item.intent}</Tag>
                        <Typography.Text>
                          {item.count} ({pct.toFixed(1)}%)
                        </Typography.Text>
                      </div>
                      <Progress percent={pct} showInfo={false} size="small" />
                    </div>
                  );
                })}
              </div>
            ) : (
              <Empty description="No intent data" />
            )}
          </Card>
        </Col>
      </Row>

      {/* ── Token Usage Over Time ─────────────────────────────────── */}
      <Card
        title={
          <Space>
            <RobotOutlined />
            <span>Token Usage Over Time</span>
            <Space size={4} style={{ marginLeft: 8 }}>
              <div style={{ width: 12, height: 12, backgroundColor: '#1677ff', borderRadius: 2, display: 'inline-block' }} />
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>Prompt</Typography.Text>
              <div style={{ width: 12, height: 12, backgroundColor: '#52c41a', borderRadius: 2, display: 'inline-block', marginLeft: 8 }} />
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>Completion</Typography.Text>
            </Space>
          </Space>
        }
        style={{ marginTop: 16 }}
      >
        {timeBuckets.length > 0 ? (
          <StackedBarChart
            data={timeBuckets}
            labelKey="label"
            keys={['promptTokens', 'completionTokens']}
            colors={['#1677ff', '#52c41a']}
            height={200}
          />
        ) : (
          <Empty description="No token data" />
        )}
      </Card>

      {/* ── Top Queries ──────────────────────────────────────────── */}
      <Card title="Top Queries / Topics" style={{ marginTop: 16 }}>
        {topQueriesData.length > 0 ? (
          <Table<{ query: string; count: number; avgMs: number }>
            dataSource={topQueriesData}
            rowKey="query"
            pagination={false}
            size="small"
            scroll={{ x: 'max-content' }}
            columns={[
              {
                title: '#',
                width: 50,
                render: (_: unknown, __: unknown, idx: number) => idx + 1,
              },
              {
                title: 'Query',
                dataIndex: 'query',
                ellipsis: true,
              },
              {
                title: 'Count',
                dataIndex: 'count',
                width: 80,
                sorter: (a, b) => a.count - b.count,
                defaultSortOrder: 'descend',
              },
              {
                title: 'Avg Latency',
                dataIndex: 'avgMs',
                width: 110,
                render: (v: number) => formatMs(v),
                sorter: (a, b) => a.avgMs - b.avgMs,
              },
              {
                title: 'Share',
                width: 120,
                render: (_: unknown, r: { count: number }) => {
                  const pct = entries.length > 0 ? (r.count / entries.length) * 100 : 0;
                  return <Progress percent={pct} size="small" format={(p) => `${p?.toFixed(1)}%`} />;
                },
              },
            ]}
          />
        ) : (
          <Empty description="No query data" />
        )}
      </Card>

      {/* ── Model Performance Summary ──────────────────────────── */}
      {stats.by_model.length > 0 && (
        <Card title="Model Performance" style={{ marginTop: 16 }}>
          <Table
            dataSource={stats.by_model}
            rowKey="model"
            pagination={false}
            size="small"
            scroll={{ x: 'max-content' }}
            columns={[
              { title: 'Model', dataIndex: 'model', render: (v: string) => <Tag color="blue">{v}</Tag> },
              { title: 'Requests', dataIndex: 'count', sorter: (a, b) => a.count - b.count, defaultSortOrder: 'descend' as const },
              { title: 'Avg Latency', dataIndex: 'avg_ms', render: (v: number) => formatMs(v), sorter: (a, b) => a.avg_ms - b.avg_ms },
              { title: 'Avg Quality', dataIndex: 'avg_quality', render: (v: number) => <span style={{ color: v >= 0.8 ? '#52c41a' : v >= 0.5 ? '#faad14' : '#cf1322' }}>{v.toFixed(3)}</span> },
              { title: 'Total Tokens', dataIndex: 'total_tokens', render: (v: number) => formatTokenCount(v) },
            ]}
          />
        </Card>
      )}
    </>
  );
}
