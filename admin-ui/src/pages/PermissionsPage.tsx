import { useState } from 'react';
import { Typography, Select, Space } from 'antd';
import { useOrgs } from '../hooks/useOrgs';
import { PermissionMatrix } from '../components/permissions/PermissionMatrix';

export function PermissionsPage() {
  const [orgId, setOrgId] = useState<string>();
  const orgs = useOrgs();

  return (
    <>
      <Typography.Title level={4}>Permissions</Typography.Title>
      <Space style={{ marginBottom: 16 }}>
        <Select
          placeholder="Select Organization"
          style={{ width: 300 }}
          value={orgId}
          onChange={setOrgId}
          options={orgs.data?.data.map((o) => ({ label: o.name, value: o.id }))}
          allowClear
        />
      </Space>

      {orgId ? (
        <PermissionMatrix orgId={orgId} />
      ) : (
        <Typography.Text type="secondary">
          Select an organization to manage permissions.
        </Typography.Text>
      )}
    </>
  );
}
