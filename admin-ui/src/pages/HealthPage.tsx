import { useState } from 'react';
import { Typography, Card, Descriptions, Badge, Button, Switch, Space } from 'antd';
import { useHealth, useMetrics } from '../hooks/useHealth';

export function HealthPage() {
  const [deep, setDeep] = useState(false);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const health = useHealth(deep);
  const metrics = useMetrics(autoRefresh);

  const isHealthy = health.data?.status === 'ok';

  return (
    <>
      <Typography.Title level={4}>System Health</Typography.Title>

      <Card title="Health Check" style={{ marginBottom: 16 }}>
        <Descriptions column={2}>
          <Descriptions.Item label="Status">
            <Badge status={isHealthy ? 'success' : 'error'} text={health.data?.status ?? 'unknown'} />
          </Descriptions.Item>
          <Descriptions.Item label="Version">{health.data?.version ?? '-'}</Descriptions.Item>
          {health.data?.uptime_secs != null && (
            <Descriptions.Item label="Uptime">
              {Math.floor(health.data.uptime_secs / 3600)}h{' '}
              {Math.floor((health.data.uptime_secs % 3600) / 60)}m
            </Descriptions.Item>
          )}
          {health.data?.embedding && (
            <Descriptions.Item label="Embedding">{health.data.embedding}</Descriptions.Item>
          )}
        </Descriptions>
        <Space style={{ marginTop: 16 }}>
          <Button
            type="primary"
            onClick={() => setDeep(true)}
            loading={health.isLoading && deep}
          >
            Run Deep Check
          </Button>
          {deep && (
            <Button onClick={() => setDeep(false)}>Back to Shallow</Button>
          )}
        </Space>
      </Card>

      <Card
        title="Prometheus Metrics"
        extra={
          <Space>
            <span>Auto-refresh (30s)</span>
            <Switch checked={autoRefresh} onChange={setAutoRefresh} />
            <Button onClick={() => metrics.refetch()} loading={metrics.isFetching}>
              Refresh
            </Button>
          </Space>
        }
      >
        <Typography.Text
          code
          style={{
            display: 'block',
            whiteSpace: 'pre-wrap',
            maxHeight: 600,
            overflow: 'auto',
            fontSize: 12,
          }}
        >
          {metrics.data || 'Loading...'}
        </Typography.Text>
      </Card>
    </>
  );
}
