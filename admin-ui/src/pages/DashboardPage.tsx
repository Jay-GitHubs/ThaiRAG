import { useEffect } from 'react';
import { Row, Col, Card, Badge, Typography, Spin, Tour } from 'antd';
import {
  BankOutlined,
  TeamOutlined,
  AppstoreOutlined,
  FolderOutlined,
  FileTextOutlined,
  HeartOutlined,
} from '@ant-design/icons';
import { useOrgs } from '../hooks/useOrgs';
import { useUsers } from '../hooks/useUsers';
import { useHealth, useMetrics } from '../hooks/useHealth';
import { parsePrometheusMetric } from '../api/metrics';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { QuickStartCard } from '../components/dashboard/QuickStartCard';
import { PageHeader } from '../components/PageHeader';
import { StatCard } from '../components/StatCard';
import { isFirstVisit } from '../tours/tourStorage';
import { getDashboardSteps } from '../tours/steps/dashboard';

export function DashboardPage() {
  const orgs = useOrgs();
  const users = useUsers();
  const health = useHealth();
  const metrics = useMetrics();
  const { t } = useI18n();
  const tour = useTour('dashboard');

  const metricsText = metrics.data || '';
  const activeSessions = parsePrometheusMetric(metricsText, 'active_sessions_total');
  const llmTokens = parsePrometheusMetric(metricsText, 'llm_tokens_total');
  const httpRequests = parsePrometheusMetric(metricsText, 'http_requests_total');

  const isHealthy = health.data?.status === 'ok';

  // Auto-start welcome tour on first visit
  useEffect(() => {
    if (isFirstVisit()) {
      const timer = setTimeout(() => tour.start(), 500);
      return () => clearTimeout(timer);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <>
      <PageHeader eyebrow="Overview" title="Dashboard">
        <TourGuideButton tourId="dashboard" />
      </PageHeader>
      <QuickStartCard />
      <Row gutter={[16, 16]} data-tour="stats-row">
        <Col xs={24} sm={12} lg={6}>
          <StatCard label="Organizations" value={orgs.data?.total ?? '-'} icon={<BankOutlined />} />
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <StatCard label="Users" value={users.data?.total ?? '-'} icon={<TeamOutlined />} />
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <StatCard label="Active Sessions" value={activeSessions} icon={<AppstoreOutlined />} />
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <StatCard label="HTTP Requests" value={httpRequests} icon={<FolderOutlined />} />
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <StatCard label="LLM Tokens Used" value={llmTokens} icon={<FileTextOutlined />} />
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card size="small" data-tour="health-card" style={{ height: '100%' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
              <span
                aria-hidden
                style={{
                  width: 42,
                  height: 42,
                  borderRadius: 10,
                  flexShrink: 0,
                  display: 'inline-flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  background: isHealthy ? 'var(--celadon-tint)' : 'transparent',
                  color: isHealthy ? 'var(--success)' : 'var(--danger)',
                  fontSize: 20,
                }}
              >
                <HeartOutlined />
              </span>
              <div>
                <div className="eyebrow" style={{ marginBottom: 3 }}>
                  Health Status
                </div>
                <div style={{ fontFamily: 'var(--font-display)', fontSize: 18, fontWeight: 600 }}>
                  {health.isLoading ? (
                    <Spin size="small" />
                  ) : (
                    <Badge
                      status={isHealthy ? 'success' : 'error'}
                      text={isHealthy ? 'OK' : 'Down'}
                    />
                  )}
                </div>
                {health.data?.version && (
                  <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 2 }}>
                    v{health.data.version}
                  </div>
                )}
              </div>
            </div>
          </Card>
        </Col>
      </Row>
      <Tour
        open={tour.isActive}
        steps={getDashboardSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
