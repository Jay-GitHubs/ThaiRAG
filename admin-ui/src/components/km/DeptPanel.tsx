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
import { getDept } from '../../api/km';
import { useWorkspaces, useCreateWorkspace, useDeleteWorkspace } from '../../hooks/useWorkspaces';
import { useDeleteDept } from '../../hooks/useDepts';
import {
  useDeptPermissions,
  useGrantDeptPermission,
  useRevokeDeptPermission,
} from '../../hooks/useScopedPermissions';
import { ScopedPermissions } from './ScopedPermissions';
import type { Workspace, Role } from '../../api/types';

interface Props {
  orgId: string;
  deptId: string;
  onMutated: () => void;
}

export function DeptPanel({ orgId, deptId, onMutated }: Props) {
  const dept = useQuery({
    queryKey: ['dept', orgId, deptId],
    queryFn: () => getDept(orgId, deptId),
  });
  const workspaces = useWorkspaces(orgId, deptId);
  const createWs = useCreateWorkspace();
  const deleteWs = useDeleteWorkspace();
  const deleteDept = useDeleteDept();
  const [createOpen, setCreateOpen] = useState(false);
  const [newName, setNewName] = useState('');

  const perms = useDeptPermissions(orgId, deptId);
  const grantPerm = useGrantDeptPermission();
  const revokePerm = useRevokeDeptPermission();

  async function handleCreate() {
    if (!newName.trim()) return;
    try {
      await createWs.mutateAsync({ orgId, deptId, name: newName.trim() });
      setCreateOpen(false);
      setNewName('');
      onMutated();
    } catch {
      message.error('Failed to create workspace');
    }
  }

  async function handleDeleteWs(wsId: string) {
    try {
      await deleteWs.mutateAsync({ orgId, deptId, wsId });
      onMutated();
    } catch {
      message.error('Failed to delete workspace');
    }
  }

  async function handleDeleteDept() {
    try {
      await deleteDept.mutateAsync({ orgId, deptId });
      onMutated();
    } catch {
      message.error('Failed to delete department');
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
      render: (_: unknown, record: Workspace) => (
        <Popconfirm title="Delete workspace?" onConfirm={() => handleDeleteWs(record.id)}>
          <Button danger size="small" icon={<DeleteOutlined />} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <Card
      title={`Department: ${dept.data?.name ?? '...'}`}
      loading={dept.isLoading}
      extra={
        <Popconfirm title="Delete this department?" onConfirm={handleDeleteDept}>
          <Button danger icon={<DeleteOutlined />}>
            Delete Dept
          </Button>
        </Popconfirm>
      }
    >
      {dept.data && (
        <Descriptions size="small" column={2} style={{ marginBottom: 16 }}>
          <Descriptions.Item label="ID">{dept.data.id}</Descriptions.Item>
          <Descriptions.Item label="Created">
            {dayjs(dept.data.created_at).format('YYYY-MM-DD HH:mm')}
          </Descriptions.Item>
        </Descriptions>
      )}

      <Tabs
        defaultActiveKey="workspaces"
        items={[
          {
            key: 'workspaces',
            label: 'Workspaces',
            children: (
              <>
                <Space style={{ marginBottom: 8 }}>
                  <Button icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
                    New Workspace
                  </Button>
                </Space>
                <Table<Workspace>
                  rowKey="id"
                  columns={columns}
                  dataSource={workspaces.data?.data}
                  loading={workspaces.isLoading}
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
                scopeLabel="Dept"
                onGrant={async (email: string, role: Role) => {
                  await grantPerm.mutateAsync({ orgId, deptId, data: { email, role } });
                }}
                onRevoke={async (email: string) => {
                  await revokePerm.mutateAsync({ orgId, deptId, data: { email } });
                }}
              />
            ),
          },
        ]}
      />

      <Modal
        title="Create Workspace"
        open={createOpen}
        onOk={handleCreate}
        onCancel={() => setCreateOpen(false)}
        confirmLoading={createWs.isPending}
      >
        <Input
          placeholder="Workspace name"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onPressEnter={handleCreate}
        />
      </Modal>
    </Card>
  );
}
