import { useState } from 'react';
import { Button, Popconfirm, Space, Table, Tag, message } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { IdentityProvider, IdpType } from '../../api/types';
import {
  useCreateIdp,
  useDeleteIdp,
  useIdentityProviders,
  useTestIdpConnection,
  useUpdateIdp,
} from '../../hooks/useSettings';
import { IdpFormModal } from './IdpFormModal';

const typeColors: Record<IdpType, string> = {
  oidc: 'green',
  oauth2: 'purple',
  saml: 'orange',
  ldap: 'cyan',
};

export function IdpTab() {
  const { data, isLoading } = useIdentityProviders();
  const createMut = useCreateIdp();
  const updateMut = useUpdateIdp();
  const deleteMut = useDeleteIdp();
  const testMut = useTestIdpConnection();

  const [modalOpen, setModalOpen] = useState(false);
  const [editingIdp, setEditingIdp] = useState<IdentityProvider | null>(null);

  const openCreate = () => {
    setEditingIdp(null);
    setModalOpen(true);
  };

  const openEdit = (idp: IdentityProvider) => {
    setEditingIdp(idp);
    setModalOpen(true);
  };

  const handleSubmit = async (values: {
    name: string;
    provider_type: IdpType;
    enabled: boolean;
    config: Record<string, unknown>;
  }) => {
    if (editingIdp) {
      await updateMut.mutateAsync({ id: editingIdp.id, data: values });
      message.success('Provider updated');
    } else {
      await createMut.mutateAsync(values);
      message.success('Provider created');
    }
    setModalOpen(false);
  };

  const handleTest = async (id: string) => {
    const result = await testMut.mutateAsync(id);
    if (result.success) {
      message.success(result.message);
    } else {
      message.warning(result.message);
    }
  };

  const columns = [
    { title: 'Name', dataIndex: 'name', key: 'name' },
    {
      title: 'Type',
      dataIndex: 'provider_type',
      key: 'provider_type',
      render: (t: IdpType) => <Tag color={typeColors[t]}>{t.toUpperCase()}</Tag>,
    },
    {
      title: 'Enabled',
      dataIndex: 'enabled',
      key: 'enabled',
      render: (v: boolean) =>
        v ? <Tag color="success">Enabled</Tag> : <Tag color="default">Disabled</Tag>,
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
      render: (_: unknown, record: IdentityProvider) => (
        <Space>
          <Button size="small" onClick={() => openEdit(record)}>
            Edit
          </Button>
          <Button size="small" onClick={() => handleTest(record.id)} loading={testMut.isPending}>
            Test
          </Button>
          <Popconfirm
            title="Delete this provider?"
            onConfirm={() => deleteMut.mutate(record.id)}
          >
            <Button size="small" danger>
              Delete
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <>
      <div style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
          Add Provider
        </Button>
      </div>
      <Table<IdentityProvider>
        rowKey="id"
        columns={columns}
        dataSource={data?.data}
        loading={isLoading}
        pagination={false}
      />
      <IdpFormModal
        open={modalOpen}
        editingIdp={editingIdp}
        onCancel={() => setModalOpen(false)}
        onSubmit={handleSubmit}
        loading={createMut.isPending || updateMut.isPending}
      />
    </>
  );
}
