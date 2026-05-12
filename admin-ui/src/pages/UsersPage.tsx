import { useState, useMemo } from 'react';
import { Button, Form, Input, Modal, Popconfirm, Select, Space, Switch, Table, Tag, Tour, Typography, message } from 'antd';
import { PlusOutlined, SearchOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useUsers, useCreateUser, useDeleteUser, useUpdateUserRole, useUpdateUserStatus } from '../hooks/useUsers';
import { useAuth } from '../auth/useAuth';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getUsersSteps } from '../tours/steps/users';
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

export function UsersPage() {
  const { data, isLoading } = useUsers();
  const createMut = useCreateUser();
  const deleteMut = useDeleteUser();
  const roleMut = useUpdateUserRole();
  const statusMut = useUpdateUserStatus();
  const { user: currentUser } = useAuth();
  const [search, setSearch] = useState('');
  const [addOpen, setAddOpen] = useState(false);
  const [form] = Form.useForm<{ email: string; name: string; password: string; role: UserRole }>();
  const { t } = useI18n();
  const tour = useTour('users');

  const roleOptions: { label: string; value: UserRole }[] = [
    { label: t('role.viewer'), value: 'viewer' },
    { label: t('role.editor'), value: 'editor' },
    { label: t('role.admin'), value: 'admin' },
    { label: t('role.superAdmin'), value: 'super_admin' },
  ];

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
      message.success(t('users.deleted'));
    } catch {
      message.error(t('users.deleteFailed'));
    }
  };

  const handleRoleChange = async (userId: string, role: UserRole) => {
    try {
      await roleMut.mutateAsync({ id: userId, role });
      message.success(t('users.roleUpdated'));
    } catch {
      message.error(t('users.roleUpdateFailed'));
    }
  };

  const handleStatusChange = async (userId: string, enabled: boolean) => {
    try {
      await statusMut.mutateAsync({ id: userId, disabled: !enabled });
      message.success(enabled ? t('users.enabled') : t('users.disabled'));
    } catch {
      message.error(t('users.statusUpdateFailed'));
    }
  };

  const handleAddSubmit = async () => {
    try {
      const values = await form.validateFields();
      await createMut.mutateAsync({
        email: values.email.trim(),
        name: values.name.trim(),
        password: values.password,
        role: values.role,
      });
      message.success(t('users.addSuccess'));
      setAddOpen(false);
      form.resetFields();
    } catch (err: unknown) {
      // Form validation errors don't have a `response` field; only show the
      // error toast for backend failures.
      if (err && typeof err === 'object' && 'response' in err) {
        message.error(t('users.addFailed'));
      }
    }
  };

  const isSuperAdmin = currentUser?.role === 'super_admin' || currentUser?.is_super_admin;

  const columns = [
    { title: t('column.name'), dataIndex: 'name', key: 'name', sorter: (a: User, b: User) => a.name.localeCompare(b.name) },
    { title: t('column.email'), dataIndex: 'email', key: 'email', sorter: (a: User, b: User) => a.email.localeCompare(b.email) },
    {
      title: t('column.provider'),
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
      title: t('column.role'),
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
      title: t('column.status'),
      key: 'status',
      render: (_: unknown, record: User) => {
        const isActive = !record.disabled;
        if (!isSuperAdmin || record.is_super_admin || record.id === currentUser?.id) {
          return (
            <Tag color={isActive ? 'green' : 'red'}>
              {isActive ? t('status.active') : t('status.disabled')}
            </Tag>
          );
        }
        return (
          <Popconfirm
            title={isActive ? t('users.disableUser') : t('users.enableUser')}
            description={
              isActive
                ? t('users.disableDescription')
                : t('users.enableDescription')
            }
            onConfirm={() => handleStatusChange(record.id, !isActive)}
          >
            <Switch
              checked={isActive}
              checkedChildren={t('status.active')}
              unCheckedChildren={t('status.disabled')}
              loading={statusMut.isPending}
            />
          </Popconfirm>
        );
      },
      filters: [
        { text: t('status.active'), value: 'active' },
        { text: t('status.disabled'), value: 'disabled' },
      ],
      onFilter: (value: unknown, record: User) =>
        value === 'active' ? !record.disabled : record.disabled,
    },
    {
      title: t('column.created'),
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
      sorter: (a: User, b: User) => dayjs(a.created_at).unix() - dayjs(b.created_at).unix(),
    },
    {
      title: t('column.userId'),
      dataIndex: 'id',
      key: 'id',
      ellipsis: true,
      render: (id: string) => <Typography.Text copyable code>{id}</Typography.Text>,
    },
    {
      title: t('column.actions'),
      key: 'actions',
      render: (_: unknown, record: User) => (
        <Space>
          <Popconfirm
            title={t('users.deleteUser')}
            description={t('message.cannotUndo')}
            onConfirm={() => handleDelete(record.id)}
            disabled={record.is_super_admin || record.id === currentUser?.id}
          >
            <Button
              size="small"
              danger
              disabled={record.is_super_admin || record.id === currentUser?.id}
            >
              {t('action.delete')}
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>{t('users.title')}</Typography.Title>
        <TourGuideButton tourId="users" />
      </div>
      <Space style={{ marginBottom: 16 }} wrap data-tour="users-search">
        <Input
          placeholder={t('users.searchPlaceholder')}
          prefix={<SearchOutlined />}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: '100%', maxWidth: 320 }}
          allowClear
        />
        {isSuperAdmin && (
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={() => setAddOpen(true)}
          >
            {t('users.add')}
          </Button>
        )}
        <Typography.Text type="secondary">
          {t('users.userCount', { count: filteredData.length })}
        </Typography.Text>
      </Space>
      <Table<User>
        rowKey="id"
        columns={columns}
        dataSource={filteredData}
        loading={isLoading}
        pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: ['10', '20', '50'] }}
        size="middle"
        scroll={{ x: 'max-content' }}
        data-tour="users-table"
      />
      <Tour
        open={tour.isActive}
        steps={getUsersSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
      <Modal
        title={t('users.addTitle')}
        open={addOpen}
        onCancel={() => { setAddOpen(false); form.resetFields(); }}
        onOk={handleAddSubmit}
        confirmLoading={createMut.isPending}
        okText={t('action.create')}
        cancelText={t('action.cancel')}
        destroyOnHidden
      >
        <Form
          form={form}
          layout="vertical"
          initialValues={{ role: 'viewer' as UserRole }}
        >
          <Form.Item
            name="email"
            label={t('users.fieldEmail')}
            rules={[
              { required: true, message: t('users.fieldEmail') },
              { type: 'email', message: t('users.fieldEmail') },
            ]}
          >
            <Input autoComplete="off" />
          </Form.Item>
          <Form.Item
            name="name"
            label={t('users.fieldName')}
            rules={[{ required: true, message: t('users.fieldName') }]}
          >
            <Input autoComplete="off" />
          </Form.Item>
          <Form.Item
            name="password"
            label={t('users.fieldPassword')}
            extra={t('users.passwordHint')}
            rules={[
              { required: true, message: t('users.fieldPassword') },
              { min: 8, message: t('users.passwordHint') },
              {
                pattern: /(?=.*[a-z])(?=.*[A-Z])(?=.*\d)/,
                message: t('users.passwordHint'),
              },
            ]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item
            name="role"
            label={t('users.fieldRole')}
            rules={[{ required: true }]}
          >
            <Select
              options={roleOptions}
            />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}
