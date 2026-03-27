import { useState, useMemo } from 'react';
import { Button, Input, Popconfirm, Select, Space, Switch, Table, Tag, Typography, message } from 'antd';
import { SearchOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useUsers, useDeleteUser, useUpdateUserRole, useUpdateUserStatus } from '../hooks/useUsers';
import { useAuth } from '../auth/useAuth';
import type { User, UserRole } from '../api/types';

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

const roleOptions: { label: string; value: UserRole }[] = [
  { label: 'Viewer', value: 'viewer' },
  { label: 'Editor', value: 'editor' },
  { label: 'Admin', value: 'admin' },
  { label: 'Super Admin', value: 'super_admin' },
];

export function UsersPage() {
  const { data, isLoading } = useUsers();
  const deleteMut = useDeleteUser();
  const roleMut = useUpdateUserRole();
  const statusMut = useUpdateUserStatus();
  const { user: currentUser } = useAuth();
  const [search, setSearch] = useState('');

  const filteredData = useMemo(() => {
    if (!data?.data) return [];
    if (!search.trim()) return data.data;
    const q = search.toLowerCase();
    return data.data.filter(
      (u) => u.name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q),
    );
  }, [data, search]);

  const handleDelete = async (id: string) => {
    try {
      await deleteMut.mutateAsync(id);
      message.success('User deleted');
    } catch {
      message.error('Failed to delete user');
    }
  };

  const handleRoleChange = async (userId: string, role: UserRole) => {
    try {
      await roleMut.mutateAsync({ id: userId, role });
      message.success('Role updated');
    } catch {
      message.error('Failed to update role');
    }
  };

  const handleStatusChange = async (userId: string, enabled: boolean) => {
    try {
      await statusMut.mutateAsync({ id: userId, disabled: !enabled });
      message.success(enabled ? 'User enabled' : 'User disabled');
    } catch {
      message.error('Failed to update status');
    }
  };

  const isSuperAdmin = currentUser?.role === 'super_admin' || currentUser?.is_super_admin;

  const columns = [
    { title: 'Name', dataIndex: 'name', key: 'name', sorter: (a: User, b: User) => a.name.localeCompare(b.name) },
    { title: 'Email', dataIndex: 'email', key: 'email', sorter: (a: User, b: User) => a.email.localeCompare(b.email) },
    {
      title: 'Provider',
      dataIndex: 'auth_provider',
      key: 'auth_provider',
      render: (v: string) => (
        <Tag color={providerColors[v] || 'default'}>{v.toUpperCase()}</Tag>
      ),
      filters: [
        { text: 'Local', value: 'local' },
        { text: 'OIDC', value: 'oidc' },
        { text: 'OAuth2', value: 'oauth2' },
        { text: 'SAML', value: 'saml' },
        { text: 'LDAP', value: 'ldap' },
      ],
      onFilter: (value: unknown, record: User) => record.auth_provider === value,
    },
    {
      title: 'Role',
      dataIndex: 'role',
      key: 'role',
      render: (role: UserRole, record: User) => {
        if (!isSuperAdmin || record.id === currentUser?.id) {
          return (
            <Tag color={roleColors[role] || 'default'}>
              {role.replace('_', ' ').toUpperCase()}
            </Tag>
          );
        }
        return (
          <Select
            size="small"
            value={role}
            style={{ width: 140 }}
            onChange={(newRole: UserRole) => handleRoleChange(record.id, newRole)}
            loading={roleMut.isPending}
            options={roleOptions}
          />
        );
      },
      filters: roleOptions.map((r) => ({ text: r.label, value: r.value })),
      onFilter: (value: unknown, record: User) => record.role === value,
    },
    {
      title: 'Status',
      key: 'status',
      render: (_: unknown, record: User) => {
        const isActive = !record.disabled;
        if (!isSuperAdmin || record.is_super_admin || record.id === currentUser?.id) {
          return (
            <Tag color={isActive ? 'green' : 'red'}>
              {isActive ? 'Active' : 'Disabled'}
            </Tag>
          );
        }
        return (
          <Popconfirm
            title={isActive ? 'Disable this user?' : 'Enable this user?'}
            description={
              isActive
                ? 'The user will not be able to log in.'
                : 'The user will be able to log in again.'
            }
            onConfirm={() => handleStatusChange(record.id, !isActive)}
          >
            <Switch
              checked={isActive}
              checkedChildren="Active"
              unCheckedChildren="Disabled"
              loading={statusMut.isPending}
            />
          </Popconfirm>
        );
      },
      filters: [
        { text: 'Active', value: 'active' },
        { text: 'Disabled', value: 'disabled' },
      ],
      onFilter: (value: unknown, record: User) =>
        value === 'active' ? !record.disabled : record.disabled,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
      sorter: (a: User, b: User) => dayjs(a.created_at).unix() - dayjs(b.created_at).unix(),
    },
    {
      title: 'User ID',
      dataIndex: 'id',
      key: 'id',
      ellipsis: true,
      render: (id: string) => <Typography.Text copyable code>{id}</Typography.Text>,
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: User) => (
        <Space>
          <Popconfirm
            title="Delete this user?"
            description="This action cannot be undone."
            onConfirm={() => handleDelete(record.id)}
            disabled={record.is_super_admin || record.id === currentUser?.id}
          >
            <Button
              size="small"
              danger
              disabled={record.is_super_admin || record.id === currentUser?.id}
            >
              Delete
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <>
      <Typography.Title level={4}>User Management</Typography.Title>
      <Space style={{ marginBottom: 16 }} wrap>
        <Input
          placeholder="Search by name or email..."
          prefix={<SearchOutlined />}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: 320 }}
          allowClear
        />
        <Typography.Text type="secondary">
          {filteredData.length} user{filteredData.length !== 1 ? 's' : ''}
        </Typography.Text>
      </Space>
      <Table<User>
        rowKey="id"
        columns={columns}
        dataSource={filteredData}
        loading={isLoading}
        pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: ['10', '20', '50'] }}
        size="middle"
      />
    </>
  );
}
