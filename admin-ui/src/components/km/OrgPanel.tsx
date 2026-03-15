import { useState } from 'react';
import {
  Card,
  Descriptions,
  Table,
  Tabs,
  Button,
  Modal,
  Input,
  Popconfirm,
  Space,
  message,
} from 'antd';
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useQuery } from '@tanstack/react-query';
import { getOrg } from '../../api/km';
import { useDepts, useCreateDept, useDeleteDept } from '../../hooks/useDepts';
import { useDeleteOrg } from '../../hooks/useOrgs';
import {
  useOrgPermissions,
  useGrantOrgPermission,
  useRevokeOrgPermission,
} from '../../hooks/useScopedPermissions';
import { ScopedPermissions } from './ScopedPermissions';
import type { Department, Role } from '../../api/types';

interface Props {
  orgId: string;
  onMutated: () => void;
}

export function OrgPanel({ orgId, onMutated }: Props) {
  const org = useQuery({ queryKey: ['org', orgId], queryFn: () => getOrg(orgId) });
  const depts = useDepts(orgId);
  const createDept = useCreateDept();
  const deleteDept = useDeleteDept();
  const deleteOrg = useDeleteOrg();
  const [createOpen, setCreateOpen] = useState(false);
  const [newName, setNewName] = useState('');

  const perms = useOrgPermissions(orgId);
  const grantPerm = useGrantOrgPermission();
  const revokePerm = useRevokeOrgPermission();

  async function handleCreate() {
    if (!newName.trim()) return;
    try {
      await createDept.mutateAsync({ orgId, name: newName.trim() });
      setCreateOpen(false);
      setNewName('');
      onMutated();
    } catch {
      message.error('Failed to create department');
    }
  }

  async function handleDeleteDept(deptId: string) {
    try {
      await deleteDept.mutateAsync({ orgId, deptId });
      onMutated();
    } catch {
      message.error('Failed to delete department');
    }
  }

  async function handleDeleteOrg() {
    try {
      await deleteOrg.mutateAsync(orgId);
      onMutated();
    } catch {
      message.error('Failed to delete organization');
    }
  }

  const columns = [
    { title: 'Name', dataIndex: 'name', key: 'name' },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: Department) => (
        <Popconfirm title="Delete department?" onConfirm={() => handleDeleteDept(record.id)}>
          <Button danger size="small" icon={<DeleteOutlined />} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <Card
      title={`Organization: ${org.data?.name ?? '...'}`}
      loading={org.isLoading}
      extra={
        <Popconfirm title="Delete this organization?" onConfirm={handleDeleteOrg}>
          <Button danger icon={<DeleteOutlined />}>
            Delete Org
          </Button>
        </Popconfirm>
      }
    >
      {org.data && (
        <Descriptions size="small" column={2} style={{ marginBottom: 16 }}>
          <Descriptions.Item label="ID">{org.data.id}</Descriptions.Item>
          <Descriptions.Item label="Created">
            {dayjs(org.data.created_at).format('YYYY-MM-DD HH:mm')}
          </Descriptions.Item>
        </Descriptions>
      )}

      <Tabs
        defaultActiveKey="departments"
        items={[
          {
            key: 'departments',
            label: 'Departments',
            children: (
              <>
                <Space style={{ marginBottom: 8 }}>
                  <Button icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
                    New Department
                  </Button>
                </Space>
                <Table<Department>
                  rowKey="id"
                  columns={columns}
                  dataSource={depts.data?.data}
                  loading={depts.isLoading}
                  size="small"
                  pagination={false}
                />
              </>
            ),
          },
          {
            key: 'permissions',
            label: 'Permissions',
            children: (
              <ScopedPermissions
                permissions={perms.data?.data}
                loading={perms.isLoading}
                scopeLabel="Org"
                onGrant={async (email: string, role: Role) => {
                  await grantPerm.mutateAsync({
                    orgId,
                    data: { email, role, scope: { level: 'Org' } },
                  });
                }}
                onRevoke={async (email: string) => {
                  await revokePerm.mutateAsync({
                    orgId,
                    data: { email, scope: { level: 'Org' } },
                  });
                }}
              />
            ),
          },
        ]}
      />

      <Modal
        title="Create Department"
        open={createOpen}
        onOk={handleCreate}
        onCancel={() => setCreateOpen(false)}
        confirmLoading={createDept.isPending}
      >
        <Input
          placeholder="Department name"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onPressEnter={handleCreate}
        />
      </Modal>
    </Card>
  );
}
