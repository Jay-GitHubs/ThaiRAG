import { useState, useEffect } from 'react';
import { Card, Steps, Button, Typography, Space, Spin } from 'antd';
import {
  SettingOutlined,
  ApartmentOutlined,
  FileTextOutlined,
  MessageOutlined,
  TeamOutlined,
  CheckCircleOutlined,
  CloseOutlined,
  RocketOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useI18n } from '../../i18n';
import client from '../../api/client';

const DISMISSED_KEY = 'thairag-quickstart-dismissed';

interface SetupStatus {
  hasProvider: boolean;
  hasOrg: boolean;
  hasWorkspace: boolean;
  hasDocuments: boolean;
  hasMultipleUsers: boolean;
}

export function QuickStartCard() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [dismissed, setDismissed] = useState(
    () => localStorage.getItem(DISMISSED_KEY) === 'true',
  );

  useEffect(() => {
    if (dismissed) return;
    let cancelled = false;

    (async () => {
      try {
        const [providersRes, orgsRes] = await Promise.all([
          client.get('/api/km/settings/providers').catch(() => null),
          client.get('/api/km/orgs').catch(() => null),
        ]);

        const providers = providersRes?.data;
        const hasProvider = !!(
          providers?.llm?.provider && providers?.llm?.provider !== 'none'
        );

        const orgs = orgsRes?.data?.items || orgsRes?.data || [];
        const hasOrg = orgs.length > 0;

        // Check workspaces and documents if we have orgs
        let hasWorkspace = false;
        let hasDocuments = false;

        if (hasOrg) {
          for (const org of orgs) {
            const deptsRes = await client
              .get(`/api/km/orgs/${org.id}/depts`)
              .catch(() => null);
            const depts = deptsRes?.data?.items || deptsRes?.data || [];
            for (const dept of depts) {
              const wsRes = await client
                .get(`/api/km/orgs/${org.id}/depts/${dept.id}/workspaces`)
                .catch(() => null);
              const workspaces = wsRes?.data?.items || wsRes?.data || [];
              if (workspaces.length > 0) {
                hasWorkspace = true;
                // Check if any workspace has documents
                for (const ws of workspaces) {
                  const docsRes = await client
                    .get(`/api/km/workspaces/${ws.id}/documents`)
                    .catch(() => null);
                  const docs = docsRes?.data?.items || docsRes?.data || [];
                  if (docs.length > 0) {
                    hasDocuments = true;
                    break;
                  }
                }
                if (hasDocuments) break;
              }
            }
            if (hasDocuments) break;
          }
        }

        // Check users
        const usersRes = await client.get('/api/admin/users').catch(() => null);
        const users = usersRes?.data?.items || usersRes?.data || [];
        const hasMultipleUsers = users.length > 1;

        if (!cancelled) {
          setStatus({ hasProvider, hasOrg, hasWorkspace, hasDocuments, hasMultipleUsers });
          setLoading(false);
        }
      } catch {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, [dismissed]);

  const handleDismiss = () => {
    localStorage.setItem(DISMISSED_KEY, 'true');
    setDismissed(true);
  };

  if (dismissed) return null;

  // If everything is done, auto-dismiss
  if (
    status &&
    status.hasProvider &&
    status.hasOrg &&
    status.hasWorkspace &&
    status.hasDocuments
  ) {
    return null;
  }

  const steps = [
    {
      key: 'providers',
      title: t('quickstart.step1.title'),
      description: t('quickstart.step1.desc'),
      icon: <SettingOutlined />,
      done: status?.hasProvider ?? false,
      route: '/settings',
    },
    {
      key: 'hierarchy',
      title: t('quickstart.step2.title'),
      description: t('quickstart.step2.desc'),
      icon: <ApartmentOutlined />,
      done: (status?.hasOrg && status?.hasWorkspace) ?? false,
      route: '/km',
    },
    {
      key: 'documents',
      title: t('quickstart.step3.title'),
      description: t('quickstart.step3.desc'),
      icon: <FileTextOutlined />,
      done: status?.hasDocuments ?? false,
      route: '/documents',
    },
    {
      key: 'test',
      title: t('quickstart.step4.title'),
      description: t('quickstart.step4.desc'),
      icon: <MessageOutlined />,
      done: false, // Can't auto-detect
      route: '/test-chat',
    },
    {
      key: 'users',
      title: t('quickstart.step5.title'),
      description: t('quickstart.step5.desc'),
      icon: <TeamOutlined />,
      done: status?.hasMultipleUsers ?? false,
      route: '/users',
    },
  ];

  // Current step = first incomplete
  const currentStep = steps.findIndex((s) => !s.done);

  return (
    <Card
      title={
        <Space>
          <RocketOutlined style={{ color: '#1677ff' }} />
          <span>{t('quickstart.title')}</span>
        </Space>
      }
      extra={
        <Button type="text" size="small" icon={<CloseOutlined />} onClick={handleDismiss} />
      }
      style={{ marginBottom: 16 }}
    >
      {loading ? (
        <div style={{ textAlign: 'center', padding: 24 }}>
          <Spin />
        </div>
      ) : (
        <>
          <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 16 }}>
            {t('quickstart.subtitle')}
          </Typography.Text>
          <Steps
            direction="vertical"
            size="small"
            current={currentStep === -1 ? steps.length : currentStep}
            items={steps.map((step) => ({
              title: (
                <Space>
                  <span style={step.done ? { textDecoration: 'line-through', opacity: 0.5 } : undefined}>
                    {step.title}
                  </span>
                  {step.done && <CheckCircleOutlined style={{ color: '#52c41a' }} />}
                </Space>
              ),
              description: (
                <div>
                  <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                    {step.description}
                  </Typography.Text>
                  {!step.done && (
                    <div style={{ marginTop: 4 }}>
                      <Button
                        type="link"
                        size="small"
                        style={{ padding: 0 }}
                        onClick={() => navigate(step.route)}
                      >
                        {t('quickstart.goTo')} →
                      </Button>
                    </div>
                  )}
                </div>
              ),
              icon: step.done ? <CheckCircleOutlined style={{ color: '#52c41a' }} /> : step.icon,
              status: step.done ? 'finish' as const : undefined,
            }))}
          />
        </>
      )}
    </Card>
  );
}
