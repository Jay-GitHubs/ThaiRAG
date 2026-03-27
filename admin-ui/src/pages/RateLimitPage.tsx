import { useState, useEffect, useCallback } from 'react';
import { Typography, Card, Table, Statistic, Row, Col, Tag, Space, Switch, Alert, Spin } from 'antd';
import {
  DashboardOutlined,
  StopOutlined,
  UserOutlined,
  GlobalOutlined,
  ClockCircleOutlined,
} from '@ant-design/icons';
import client from '../api/client';

const { Title } = Typography;

// ── Types ───────────────────────────────────────────────────────────

interface IpBucketStats {
  ip: string;
  request_count: number;
  tokens_remaining: number;
  last_seen_secs_ago: number;
}

interface UserBucketStats {
  user_id: string;
  request_count: number;
  tokens_remaining: number;
  last_seen_secs_ago: number;
}

interface GlobalStats {
  ip_rate_limiting_enabled: boolean;
  total_ip_blocked: number;
  total_user_blocked: number;
  active_ip_limiters: number;
  active_user_limiters: number;
}

interface RateLimitStatsResponse {
  global: GlobalStats;
  ip_stats: IpBucketStats[];
  user_stats: UserBucketStats[];
}

interface BlockedEvent {
  timestamp: string;
  source: string;
  source_type: string;
  endpoint: string;
  reason: string;
}

interface BlockedEventsResponse {
  events: BlockedEvent[];
}

// ── Helpers ─────────────────────────────────────────────────────────

function formatSecsAgo(secs: number): string {
  if (secs < 60) return `${Math.round(secs)}s ago`;
  if (secs < 3600) return `${Math.round(secs / 60)}m ago`;
  return `${Math.round(secs / 3600)}h ago`;
}

function formatTimestamp(ts: string): string {
  const d = new Date(ts);
  return d.toLocaleString();
}

// ── Component ───────────────────────────────────────────────────────

export default function RateLimitPage() {
  const [stats, setStats] = useState<RateLimitStatsResponse | null>(null);
  const [blocked, setBlocked] = useState<BlockedEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [autoRefresh, setAutoRefresh] = useState(true);

  const fetchData = useCallback(async () => {
    try {
      const [statsRes, blockedRes] = await Promise.all([
        client.get<RateLimitStatsResponse>('/api/km/admin/rate-limits/stats'),
        client.get<BlockedEventsResponse>('/api/km/admin/rate-limits/blocked'),
      ]);
      setStats(statsRes.data);
      setBlocked(blockedRes.data.events);
      setError(null);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Failed to load rate limit data';
      setError(msg);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  useEffect(() => {
    if (!autoRefresh) return;
    const interval = setInterval(fetchData, 10_000);
    return () => clearInterval(interval);
  }, [autoRefresh, fetchData]);

  if (loading && !stats) {
    return (
      <div style={{ textAlign: 'center', padding: 80 }}>
        <Spin size="large" />
      </div>
    );
  }

  if (error && !stats) {
    return <Alert type="error" message="Error" description={error} showIcon />;
  }

  const g = stats?.global;

  // ── Table Columns ─────────────────────────────────────────────

  const ipColumns = [
    {
      title: 'IP Address',
      dataIndex: 'ip',
      key: 'ip',
      render: (ip: string) => <Tag icon={<GlobalOutlined />}>{ip}</Tag>,
    },
    {
      title: 'Requests',
      dataIndex: 'request_count',
      key: 'request_count',
      sorter: (a: IpBucketStats, b: IpBucketStats) => a.request_count - b.request_count,
    },
    {
      title: 'Tokens Remaining',
      dataIndex: 'tokens_remaining',
      key: 'tokens_remaining',
      render: (v: number) => v.toFixed(1),
    },
    {
      title: 'Last Seen',
      dataIndex: 'last_seen_secs_ago',
      key: 'last_seen_secs_ago',
      render: (v: number) => (
        <span>
          <ClockCircleOutlined style={{ marginRight: 4 }} />
          {formatSecsAgo(v)}
        </span>
      ),
    },
  ];

  const userColumns = [
    {
      title: 'User ID',
      dataIndex: 'user_id',
      key: 'user_id',
      render: (id: string) => <Tag icon={<UserOutlined />}>{id.slice(0, 12)}...</Tag>,
    },
    {
      title: 'Requests',
      dataIndex: 'request_count',
      key: 'request_count',
      sorter: (a: UserBucketStats, b: UserBucketStats) => a.request_count - b.request_count,
    },
    {
      title: 'Tokens Remaining',
      dataIndex: 'tokens_remaining',
      key: 'tokens_remaining',
      render: (v: number) => v.toFixed(1),
    },
    {
      title: 'Last Seen',
      dataIndex: 'last_seen_secs_ago',
      key: 'last_seen_secs_ago',
      render: (v: number) => (
        <span>
          <ClockCircleOutlined style={{ marginRight: 4 }} />
          {formatSecsAgo(v)}
        </span>
      ),
    },
  ];

  const blockedColumns = [
    {
      title: 'Timestamp',
      dataIndex: 'timestamp',
      key: 'timestamp',
      render: (ts: string) => formatTimestamp(ts),
      width: 200,
    },
    {
      title: 'Type',
      dataIndex: 'source_type',
      key: 'source_type',
      width: 80,
      render: (t: string) => (
        <Tag color={t === 'ip' ? 'blue' : 'orange'}>{t.toUpperCase()}</Tag>
      ),
    },
    {
      title: 'Source',
      dataIndex: 'source',
      key: 'source',
      render: (s: string, record: BlockedEvent) =>
        record.source_type === 'ip' ? s : `${s.slice(0, 12)}...`,
    },
    {
      title: 'Endpoint',
      dataIndex: 'endpoint',
      key: 'endpoint',
      render: (ep: string) => ep || '-',
    },
    {
      title: 'Reason',
      dataIndex: 'reason',
      key: 'reason',
    },
  ];

  return (
    <div>
      <Space
        style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}
        align="center"
      >
        <Title level={3} style={{ margin: 0 }}>
          <DashboardOutlined /> Rate Limiting Dashboard
        </Title>
        <Space>
          <span>Auto-refresh (10s):</span>
          <Switch checked={autoRefresh} onChange={setAutoRefresh} />
        </Space>
      </Space>

      {error && <Alert type="warning" message={error} showIcon style={{ marginBottom: 16 }} />}

      {/* Global Stats */}
      <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic
              title="Total Blocked (IP)"
              value={g?.total_ip_blocked ?? 0}
              prefix={<StopOutlined style={{ color: '#f5222d' }} />}
              valueStyle={{ color: '#f5222d' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic
              title="Total Blocked (User)"
              value={g?.total_user_blocked ?? 0}
              prefix={<StopOutlined style={{ color: '#fa8c16' }} />}
              valueStyle={{ color: '#fa8c16' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Row gutter={16}>
              <Col span={12}>
                <Statistic
                  title="Active IP Limiters"
                  value={g?.active_ip_limiters ?? 0}
                  prefix={<GlobalOutlined />}
                />
              </Col>
              <Col span={12}>
                <Statistic
                  title="Active User Limiters"
                  value={g?.active_user_limiters ?? 0}
                  prefix={<UserOutlined />}
                />
              </Col>
            </Row>
          </Card>
        </Col>
      </Row>

      {!g?.ip_rate_limiting_enabled && (
        <Alert
          type="info"
          message="IP rate limiting is disabled"
          description="Enable it in config under [server.rate_limit] to see per-IP statistics."
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      {/* Per-IP Stats */}
      <Card
        title={
          <span>
            <GlobalOutlined /> Per-IP Usage (Top 20)
          </span>
        }
        style={{ marginBottom: 16 }}
      >
        <Table
          dataSource={stats?.ip_stats ?? []}
          columns={ipColumns}
          rowKey="ip"
          pagination={false}
          size="small"
          locale={{ emptyText: 'No active IP limiters' }}
        />
      </Card>

      {/* Per-User Stats */}
      <Card
        title={
          <span>
            <UserOutlined /> Per-User Usage (Top 20)
          </span>
        }
        style={{ marginBottom: 16 }}
      >
        <Table
          dataSource={stats?.user_stats ?? []}
          columns={userColumns}
          rowKey="user_id"
          pagination={false}
          size="small"
          locale={{ emptyText: 'No active user limiters' }}
        />
      </Card>

      {/* Recently Blocked */}
      <Card
        title={
          <span>
            <StopOutlined style={{ color: '#f5222d' }} /> Recently Blocked Requests (Last 100)
          </span>
        }
      >
        <Table
          dataSource={blocked}
          columns={blockedColumns}
          rowKey={(_, i) => String(i)}
          pagination={{ pageSize: 20, showSizeChanger: false }}
          size="small"
          locale={{ emptyText: 'No blocked requests' }}
        />
      </Card>
    </div>
  );
}
