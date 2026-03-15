import { useState } from 'react';
import { Table, Tag, Button, Popconfirm, Space, Select, Form, Modal, message } from 'antd';
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons';
import { useUsers } from '../../hooks/useUsers';
import type { PermissionResponse, Role } from '../../api/types';

interface Props {
  permissions: PermissionResponse[] | undefined;
  loading: boolean;
  scopeLabel: string;
  onGrant: (email: string, role: Role) => Promise<void>;
  onRevoke: (email: string) => Promise<void>;
}

const roleColors: Record<Role, string> = {
  owner: 'red',
  admin: 'orange',
  editor: 'blue',
  viewer: 'default',
};

const roleOptions: { label: string; value: Role }[] = [
  { label: 'Owner', value: 'owner' },
  { label: 'Admin', value: 'admin' },
  { label: 'Editor', value: 'editor' },
  { label: 'Viewer', value: 'viewer' },
];

export function ScopedPermissions({ permissions, loading, scopeLabel, onGrant, onRevoke }: Props) {
  const [modalOpen, setModalOpen] = useState(false);
  const [granting, setGranting] = useState(false);
  const [form] = Form.useForm();
  const users = useUsers();

  async function handleGrant() {
    try {
      const values = await form.validateFields();
      setGranting(true);
      await onGrant(values.email, values.role);
      message.success('Permission granted');
      form.resetFields();
      setModalOpen(false);
    } catch (err: unknown) {
      const msg =
        err && typeof err === 'object' && 'response' in err
          ? (err as { response: { data?: { error?: { message?: string } } } }).response?.data
              ?.error?.message
          : undefined;
      if (msg) message.error(msg);
    } finally {
      setGranting(false);
    }
  }

  async function handleRevoke(email: string) {
    try {
      await onRevoke(email);
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
      title: 'Role',
      dataIndex: 'role',
      key: 'role',
      render: (role: Role) => <Tag color={roleColors[role]}>{role}</Tag>,
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: PermissionResponse) => (
        <Popconfirm title="Revoke this permission?" onConfirm={() => handleRevoke(record.email)}>
          <Button danger size="small" icon={<DeleteOutlined />} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <>
      <Space style={{ marginBottom: 12 }}>
        <Button icon={<PlusOutlined />} type="primary" onClick={() => setModalOpen(true)}>
          Grant {scopeLabel} Permission
        </Button>
      </Space>

      <Table<PermissionResponse>
        rowKey={(r) => `${r.user_id}-${JSON.stringify(r.scope)}`}
        columns={columns}
        dataSource={permissions}
        loading={loading}
        size="small"
        pagination={{ pageSize: 10 }}
      />

      <Modal
        title={`Grant ${scopeLabel} Permission`}
        open={modalOpen}
        onOk={handleGrant}
        onCancel={() => setModalOpen(false)}
        confirmLoading={granting}
      >
        <Form form={form} layout="vertical" initialValues={{ role: 'viewer' }}>
          <Form.Item name="email" label="User" rules={[{ required: true }]}>
            <Select
              showSearch
              placeholder="Select a user"
              optionFilterProp="label"
              loading={users.isLoading}
              options={users.data?.data.map((u) => ({
                label: `${u.name} (${u.email})`,
                value: u.email,
              }))}
            />
          </Form.Item>
          <Form.Item name="role" label="Role" rules={[{ required: true }]}>
            <Select options={roleOptions} />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}
