import { useState } from 'react';
import { Typography, Select, Breadcrumb, Space, Tour } from 'antd';
import { useOrgs } from '../hooks/useOrgs';
import { useDepts } from '../hooks/useDepts';
import { useWorkspaces } from '../hooks/useWorkspaces';
import { DocumentTable } from '../components/documents/DocumentTable';
import { JobsTable } from '../components/documents/JobsTable';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getDocumentsSteps } from '../tours/steps/documents';

export function DocumentsPage() {
  const [orgId, setOrgId] = useState<string>();
  const [deptId, setDeptId] = useState<string>();
  const [wsId, setWsId] = useState<string>();
  const { t } = useI18n();
  const tour = useTour('documents');

  const orgs = useOrgs();
  const depts = useDepts(orgId);
  const workspaces = useWorkspaces(orgId, deptId);

  const orgName = orgs.data?.data.find((o) => o.id === orgId)?.name;
  const deptName = depts.data?.data.find((d) => d.id === deptId)?.name;
  const wsName = workspaces.data?.data.find((w) => w.id === wsId)?.name;

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>{t('documents.title')}</Typography.Title>
        <TourGuideButton tourId="documents" />
      </div>
      <Space style={{ marginBottom: 16 }} wrap>
        <span data-tour="doc-org-select">
          <Select
            placeholder={t('documents.selectOrg')}
            style={{ width: '100%', maxWidth: 200, minWidth: 140 }}
            value={orgId}
            onChange={(v) => {
              setOrgId(v);
              setDeptId(undefined);
              setWsId(undefined);
            }}
            options={orgs.data?.data.map((o) => ({ label: o.name, value: o.id }))}
            allowClear
          />
        </span>
        <span data-tour="doc-dept-select">
          <Select
            placeholder={t('documents.selectDept')}
            style={{ width: '100%', maxWidth: 200, minWidth: 140 }}
            value={deptId}
            onChange={(v) => {
              setDeptId(v);
              setWsId(undefined);
            }}
            options={depts.data?.data.map((d) => ({ label: d.name, value: d.id }))}
            disabled={!orgId}
            allowClear
          />
        </span>
        <span data-tour="doc-ws-select">
          <Select
            placeholder={t('documents.selectWorkspace')}
            style={{ width: '100%', maxWidth: 200, minWidth: 140 }}
            value={wsId}
            onChange={setWsId}
            options={workspaces.data?.data.map((w) => ({ label: w.name, value: w.id }))}
            disabled={!deptId}
            allowClear
          />
        </span>
      </Space>

      {orgName && (
        <Breadcrumb
          style={{ marginBottom: 16 }}
          items={[
            { title: orgName },
            ...(deptName ? [{ title: deptName }] : []),
            ...(wsName ? [{ title: wsName }] : []),
          ]}
        />
      )}

      {wsId ? (
        <div data-tour="doc-table">
          <JobsTable workspaceId={wsId} />
          <DocumentTable workspaceId={wsId} />
        </div>
      ) : (
        <Typography.Text type="secondary">
          {t('documents.selectPrompt')}
        </Typography.Text>
      )}
      <Tour
        open={tour.isActive}
        steps={getDocumentsSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
