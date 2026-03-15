import { Row, Col, Card, Statistic, Badge, Typography, Spin } from 'antd';
import {
  BankOutlined,
  TeamOutlined,
  AppstoreOutlined,
  FolderOutlined,
  FileTextOutlined,
} from '@ant-design/icons';
import { useOrgs } from '../hooks/useOrgs';
import { useUsers } from '../hooks/useUsers';
import { useHealth, useMetrics } from '../hooks/useHealth';
import { parsePrometheusMetric } from '../api/metrics';

export function DashboardPage() {
  const orgs = useOrgs();
  const users = useUsers();
  const health = useHealth();
  const metrics = useMetrics();

  const metricsText = metrics.data || '';
  const activeSessions = parsePrometheusMetric(metricsText, 'active_sessions_total');
  const llmTokens = parsePrometheusMetric(metricsText, 'llm_tokens_total');
  const httpRequests = parsePrometheusMetric(metricsText, 'http_requests_total');

  const isHealthy = health.data?.status === 'ok';

  return (
    <>
      <Typography.Title level={4}>Dashboard</Typography.Title>
      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Organizations"
              value={orgs.data?.total ?? '-'}
              prefix={<BankOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Users"
              value={users.data?.total ?? '-'}
              prefix={<TeamOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Active Sessions"
              value={activeSessions}
              prefix={<AppstoreOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="HTTP Requests"
              value={httpRequests}
              prefix={<FolderOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="LLM Tokens Used"
              value={llmTokens}
              prefix={<FileTextOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>Health Status</span>
              {health.isLoading ? (
                <Spin size="small" />
              ) : (
                <Badge status={isHealthy ? 'success' : 'error'} text={isHealthy ? 'OK' : 'Down'} />
              )}
            </div>
            {health.data?.version && (
              <Typography.Text type="secondary">v{health.data.version}</Typography.Text>
            )}
          </Card>
        </Col>
      </Row>
    </>
  );
}
