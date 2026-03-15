import { useState } from 'react';
import { Typography, Select, Breadcrumb, Space } from 'antd';
import { useOrgs } from '../hooks/useOrgs';
import { useDepts } from '../hooks/useDepts';
import { useWorkspaces } from '../hooks/useWorkspaces';
import { DocumentTable } from '../components/documents/DocumentTable';

export function DocumentsPage() {
  const [orgId, setOrgId] = useState<string>();
  const [deptId, setDeptId] = useState<string>();
  const [wsId, setWsId] = useState<string>();

  const orgs = useOrgs();
  const depts = useDepts(orgId);
  const workspaces = useWorkspaces(orgId, deptId);

  const orgName = orgs.data?.data.find((o) => o.id === orgId)?.name;
  const deptName = depts.data?.data.find((d) => d.id === deptId)?.name;
  const wsName = workspaces.data?.data.find((w) => w.id === wsId)?.name;

  return (
    <>
      <Typography.Title level={4}>Documents</Typography.Title>
      <Space style={{ marginBottom: 16 }} wrap>
        <Select
          placeholder="Select Organization"
          style={{ width: 200 }}
          value={orgId}
          onChange={(v) => {
            setOrgId(v);
            setDeptId(undefined);
            setWsId(undefined);
          }}
          options={orgs.data?.data.map((o) => ({ label: o.name, value: o.id }))}
          allowClear
        />
        <Select
          placeholder="Select Department"
          style={{ width: 200 }}
          value={deptId}
          onChange={(v) => {
            setDeptId(v);
            setWsId(undefined);
          }}
          options={depts.data?.data.map((d) => ({ label: d.name, value: d.id }))}
          disabled={!orgId}
          allowClear
        />
        <Select
          placeholder="Select Workspace"
          style={{ width: 200 }}
          value={wsId}
          onChange={setWsId}
          options={workspaces.data?.data.map((w) => ({ label: w.name, value: w.id }))}
          disabled={!deptId}
          allowClear
        />
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
        <DocumentTable workspaceId={wsId} />
      ) : (
        <Typography.Text type="secondary">
          Select an organization, department, and workspace to view documents.
        </Typography.Text>
      )}
    </>
  );
}
