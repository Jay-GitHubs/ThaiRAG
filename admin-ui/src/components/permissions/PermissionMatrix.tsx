import { useMemo, useState } from 'react';
import { Table, Tag, Button, Popconfirm, Space, Select, message } from 'antd';
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons';
import { usePermissions, useRevokePermission } from '../../hooks/usePermissions';
import { useDepts } from '../../hooks/useDepts';
import { GrantModal } from './GrantModal';
import type { PermissionResponse, Role, PermissionScope, ScopeRequest } from '../../api/types';

interface Props {
  orgId: string;
}

const roleColors: Record<Role, string> = {
  owner: 'red',
  admin: 'orange',
  editor: 'blue',
  viewer: 'default',
};

type ScopeFilter = 'all' | 'Org' | 'Dept' | 'Workspace';

export function PermissionMatrix({ orgId }: Props) {
  const { data, isLoading } = usePermissions(orgId);
  const revoke = useRevokePermission();
  const [grantOpen, setGrantOpen] = useState(false);
  const [filter, setFilter] = useState<ScopeFilter>('all');

  const depts = useDepts(orgId);

  // Build lookup maps for dept/workspace names
  const deptMap = useMemo(() => {
    const map = new Map<string, string>();
    depts.data?.data?.forEach((d) => map.set(d.id, d.name));
    return map;
  }, [depts.data]);

  // For workspace names, we need to fetch workspaces per dept
  // We'll collect workspace IDs from permissions and show what we can
  const filteredData = useMemo(() => {
    if (!data?.data) return [];
    if (filter === 'all') return data.data;
    return data.data.filter((p) => p.scope.level === filter);
  }, [data, filter]);

  function scopeDisplay(scope: PermissionScope): { level: string; name: string } {
    switch (scope.level) {
      case 'Org':
        return { level: 'Org', name: '' };
      case 'Dept': {
        const name = deptMap.get(scope.dept_id);
        return { level: 'Dept', name: name ?? scope.dept_id.substring(0, 8) + '...' };
      }
      case 'Workspace': {
        const deptName = deptMap.get(scope.dept_id);
        const wsId = scope.workspace_id;
        return {
          level: 'Workspace',
          name: `${deptName ?? '...'} / ${wsId.substring(0, 8)}...`,
        };
      }
    }
  }

  async function handleRevoke(record: PermissionResponse) {
    const scope = record.scope;
    let scopeReq: ScopeRequest;
    if (scope.level === 'Workspace') {
      scopeReq = {
        level: 'Workspace' as const,
        dept_id: scope.dept_id,
        workspace_id: scope.workspace_id,
      };
    } else if (scope.level === 'Dept') {
      scopeReq = { level: 'Dept' as const, dept_id: scope.dept_id };
    } else {
      scopeReq = { level: 'Org' as const };
    }

    try {
      await revoke.mutateAsync({
        orgId,
        data: { email: record.email, scope: scopeReq },
      });
      message.success('Permission revoked');
    } catch (err: unknown) {
      const msg =
        err && typeof err === 'object' && 'response' in err
          ? (err as { response: { data?: { error?: { message?: string } } } }).response?.data
              ?.error?.message
          : undefined;
      message.error(msg || 'Failed to revoke permission');
    }
  }

  const columns = [
    { title: 'Email', dataIndex: 'email', key: 'email' },
    {
      title: 'Scope',
      key: 'scope',
      render: (_: unknown, record: PermissionResponse) => {
        const { level, name } = scopeDisplay(record.scope);
        const color =
          level === 'Org' ? 'green' : level === 'Dept' ? 'blue' : level === 'Workspace' ? 'purple' : 'default';
        return (
          <Space>
            <Tag color={color}>{level}</Tag>
            {name && <span style={{ fontSize: 12, color: '#666' }}>{name}</span>}
          </Space>
        );
      },
    },
    {
      title: 'Role',
      dataIndex: 'role',
      key: 'role',
      render: (role: Role) => <Tag color={roleColors[role]}>{role}</Tag>,
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: PermissionResponse) => (
        <Popconfirm
          title="Revoke this permission?"
          onConfirm={() => handleRevoke(record)}
        >
          <Button danger size="small" icon={<DeleteOutlined />} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <>
      <Space style={{ marginBottom: 16 }}>
        <Button icon={<PlusOutlined />} type="primary" onClick={() => setGrantOpen(true)}>
          Grant Permission
        </Button>
        <Select
          value={filter}
          onChange={setFilter}
          style={{ width: 160 }}
          options={[
            { label: 'All Scopes', value: 'all' },
            { label: 'Org Level', value: 'Org' },
            { label: 'Dept Level', value: 'Dept' },
            { label: 'Workspace Level', value: 'Workspace' },
          ]}
        />
      </Space>

      <Table<PermissionResponse>
        rowKey={(r) => `${r.user_id}-${JSON.stringify(r.scope)}`}
        columns={columns}
        dataSource={filteredData}
        loading={isLoading}
        pagination={{ pageSize: 20 }}
      />

      <GrantModal orgId={orgId} open={grantOpen} onClose={() => setGrantOpen(false)} />
    </>
  );
}
