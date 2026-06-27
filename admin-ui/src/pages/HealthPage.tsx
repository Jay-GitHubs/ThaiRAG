import { useState } from 'react';
import { Card, Button, Switch, Space, Row, Col } from 'antd';
import {
  ClockCircleOutlined,
  TagOutlined,
  DeploymentUnitOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useHealth, useMetrics } from '../hooks/useHealth';
import { PageHeader } from '../components/PageHeader';
import { StatCard } from '../components/StatCard';

export function HealthPage() {
  const [deep, setDeep] = useState(false);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const health = useHealth(deep);
  const metrics = useMetrics(autoRefresh);

  const isHealthy = health.data?.status === 'ok';
  const uptime =
    health.data?.uptime_secs != null
      ? `${Math.floor(health.data.uptime_secs / 3600)}h ${Math.floor(
          (health.data.uptime_secs % 3600) / 60,
        )}m`
      : '-';

  return (
    <>
      <PageHeader eyebrow="System" title="System Health" />

      {/* Status hero */}
      <Card size="small" style={{ marginBottom: 16 }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 16,
            flexWrap: 'wrap',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            <span
              aria-hidden
              style={{
                width: 14,
                height: 14,
                borderRadius: '50%',
                flexShrink: 0,
                background: isHealthy ? 'var(--success)' : 'var(--danger)',
                boxShadow: `0 0 0 4px ${isHealthy ? 'var(--celadon-tint)' : 'transparent'}`,
              }}
            />
            <div>
              <div className="eyebrow">Status</div>
              <div style={{ fontFamily: 'var(--font-display)', fontSize: 22, fontWeight: 600 }}>
                {health.data?.status ?? 'unknown'}
              </div>
            </div>
          </div>
          <Space>
            <Button type="primary" onClick={() => setDeep(true)} loading={health.isLoading && deep}>
              Run Deep Check
            </Button>
            {deep && <Button onClick={() => setDeep(false)}>Back to Shallow</Button>}
          </Space>
        </div>

        <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
          <Col xs={24} sm={8}>
            <StatCard label="Version" value={health.data?.version ?? '-'} icon={<TagOutlined />} />
          </Col>
          <Col xs={24} sm={8}>
            <StatCard label="Uptime" value={uptime} icon={<ClockCircleOutlined />} />
          </Col>
          <Col xs={24} sm={8}>
            <StatCard
              label="Embedding"
              value={health.data?.embedding ?? '-'}
              icon={<DeploymentUnitOutlined />}
            />
          </Col>
        </Row>
      </Card>

      <Card
        size="small"
        title="Prometheus Metrics"
        extra={
          <Space>
            <span style={{ color: 'var(--text-muted)', fontSize: 12 }}>Auto-refresh (30s)</span>
            <Switch checked={autoRefresh} onChange={setAutoRefresh} />
            <Button
              icon={<ReloadOutlined />}
              onClick={() => metrics.refetch()}
              loading={metrics.isFetching}
            >
              Refresh
            </Button>
          </Space>
        }
      >
        <pre
          style={{
            margin: 0,
            background: 'var(--code-bg)',
            color: 'var(--code-text)',
            padding: '14px 16px',
            borderRadius: 10,
            maxHeight: 600,
            overflow: 'auto',
            fontFamily: 'var(--font-mono)',
            fontSize: 12,
            whiteSpace: 'pre-wrap',
          }}
          className="thin-scroll"
        >
          {metrics.data || 'Loading…'}
        </pre>
      </Card>
    </>
  );
}
