import { Button, Popconfirm, Space, Table, Tag, Typography, message } from 'antd';
import dayjs from 'dayjs';
import { useUsers, useDeleteUser } from '../hooks/useUsers';
import type { User } from '../api/types';

const providerColors: Record<string, string> = {
  local: 'blue',
  oidc: 'green',
  oauth2: 'purple',
  saml: 'orange',
  ldap: 'cyan',
};

const roleColors: Record<string, string> = {
  super_admin: 'red',
  admin: 'volcano',
  editor: 'blue',
  viewer: 'default',
};

export function UsersPage() {
  const { data, isLoading } = useUsers();
  const deleteMut = useDeleteUser();

  const handleDelete = async (id: string) => {
    try {
      await deleteMut.mutateAsync(id);
      message.success('User deleted');
    } catch {
      message.error('Failed to delete user');
    }
  };

  const columns = [
    { title: 'Name', dataIndex: 'name', key: 'name' },
    { title: 'Email', dataIndex: 'email', key: 'email' },
    {
      title: 'Provider',
      dataIndex: 'auth_provider',
      key: 'auth_provider',
      render: (v: string) => (
        <Tag color={providerColors[v] || 'default'}>{v.toUpperCase()}</Tag>
      ),
    },
    {
      title: 'Role',
      dataIndex: 'role',
      key: 'role',
      render: (role: string) => (
        <Tag color={roleColors[role] || 'default'}>
          {role.replace('_', ' ').toUpperCase()}
        </Tag>
      ),
    },
    {
      title: 'User ID',
      dataIndex: 'id',
      key: 'id',
      ellipsis: true,
      render: (id: string) => <Typography.Text copyable code>{id}</Typography.Text>,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: User) => (
        <Space>
          <Popconfirm
            title="Delete this user?"
            onConfirm={() => handleDelete(record.id)}
            disabled={record.is_super_admin}
          >
            <Button size="small" danger disabled={record.is_super_admin}>
              Delete
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <>
      <Typography.Title level={4}>Users</Typography.Title>
      <Table<User>
        rowKey="id"
        columns={columns}
        dataSource={data?.data}
        loading={isLoading}
        pagination={{ pageSize: 20 }}
      />
    </>
  );
}
