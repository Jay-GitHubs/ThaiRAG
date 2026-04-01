import { useState, useEffect, useCallback } from 'react';
import {
  Button,
  Checkbox,
  Form,
  Input,
  Modal,
  Popconfirm,
  Space,
  Table,
  Tag,
  Tour,
  Typography,
  message,
} from 'antd';
import { PlusOutlined, EditOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getRolesSteps } from '../tours/steps/roles';
import type { CustomRole, RolePermission } from '../api/roles';
import { listRoles, createRole, updateRole, deleteRole } from '../api/roles';

const RESOURCES = ['documents', 'workspaces', 'users', 'settings', 'analytics'] as const;
const ACTIONS = ['read', 'write', 'delete', 'manage', 'export'] as const;

type Resource = (typeof RESOURCES)[number];
type Action = (typeof ACTIONS)[number];

/** Flat map: resource -> set of actions */
type PermMatrix = Record<Resource, Set<Action>>;

function roleToMatrix(permissions: RolePermission[]): PermMatrix {
  const matrix = {} as PermMatrix;
  for (const r of RESOURCES) {
    matrix[r] = new Set<Action>();
  }
  for (const perm of permissions) {
    if (RESOURCES.includes(perm.resource as Resource)) {
      for (const action of perm.actions) {
        if (ACTIONS.includes(action as Action)) {
          matrix[perm.resource as Resource].add(action as Action);
        }
      }
    }
  }
  return matrix;
}

function matrixToPermissions(matrix: PermMatrix): RolePermission[] {
  return RESOURCES.map((r) => ({ resource: r, actions: Array.from(matrix[r]) })).filter(
    (p) => p.actions.length > 0,
  );
}

function PermissionMatrixEditor({
  value,
  onChange,
  disabled,
}: {
  value?: PermMatrix;
  onChange?: (m: PermMatrix) => void;
  disabled?: boolean;
}) {
  const { t } = useI18n();
  const matrix = value ?? ({} as PermMatrix);

  const toggle = (resource: Resource, action: Action, checked: boolean) => {
    const next: PermMatrix = {} as PermMatrix;
    for (const r of RESOURCES) {
      next[r] = new Set(matrix[r] ?? []);
    }
    if (checked) {
      next[resource].add(action);
    } else {
      next[resource].delete(action);
    }
    onChange?.(next);
  };

  return (
    <table style={{ width: '100%', borderCollapse: 'collapse' }}>
      <thead>
        <tr>
          <th style={{ textAlign: 'left', padding: '4px 8px' }}>{t('roles.resource')}</th>
          {ACTIONS.map((a) => (
            <th key={a} style={{ textAlign: 'center', padding: '4px 8px', textTransform: 'capitalize' }}>
              {a}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {RESOURCES.map((r, i) => (
          <tr key={r} style={{ background: i % 2 === 0 ? 'transparent' : 'rgba(0,0,0,0.03)' }}>
            <td style={{ padding: '4px 8px', textTransform: 'capitalize', fontWeight: 500 }}>{r}</td>
            {ACTIONS.map((a) => (
              <td key={a} style={{ textAlign: 'center', padding: '4px 8px' }}>
                <Checkbox
                  checked={matrix[r]?.has(a) ?? false}
                  onChange={(e) => toggle(r, a, e.target.checked)}
                  disabled={disabled}
                />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function usePermMatrix(initial?: RolePermission[]) {
  const [matrix, setMatrix] = useState<PermMatrix>(() =>
    roleToMatrix(initial ?? []),
  );
  return { matrix, setMatrix };
}

export function RolesPage() {
  const { t } = useI18n();
  const tour = useTour('roles');
  const [roles, setRoles] = useState<CustomRole[]>([]);
  const [loading, setLoading] = useState(false);

  // Create modal
  const [createOpen, setCreateOpen] = useState(false);
  const [createLoading, setCreateLoading] = useState(false);
  const [createForm] = Form.useForm();
  const createMatrix = usePermMatrix();

  // Edit modal
  const [editOpen, setEditOpen] = useState(false);
  const [editLoading, setEditLoading] = useState(false);
  const [editForm] = Form.useForm();
  const editMatrix = usePermMatrix();
  const [editingRole, setEditingRole] = useState<CustomRole | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listRoles();
      setRoles(data);
    } catch {
      message.error(t('roles.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  const handleCreate = async (values: { name: string; description: string }) => {
    setCreateLoading(true);
    try {
      await createRole({
        name: values.name,
        description: values.description,
        permissions: matrixToPermissions(createMatrix.matrix),
      });
      message.success(t('roles.created'));
      setCreateOpen(false);
      createForm.resetFields();
      createMatrix.setMatrix(roleToMatrix([]));
      load();
    } catch {
      message.error(t('roles.createFailed'));
    } finally {
      setCreateLoading(false);
    }
  };

  const openEdit = (role: CustomRole) => {
    setEditingRole(role);
    editForm.setFieldsValue({ name: role.name, description: role.description });
    editMatrix.setMatrix(roleToMatrix(role.permissions));
    setEditOpen(true);
  };

  const handleEdit = async (values: { name: string; description: string }) => {
    if (!editingRole) return;
    setEditLoading(true);
    try {
      await updateRole(editingRole.id, {
        name: values.name,
        description: values.description,
        permissions: matrixToPermissions(editMatrix.matrix),
      });
      message.success(t('roles.updated'));
      setEditOpen(false);
      load();
    } catch {
      message.error(t('roles.updateFailed'));
    } finally {
      setEditLoading(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteRole(id);
      message.success(t('roles.deleted'));
      load();
    } catch {
      message.error(t('roles.deleteFailed'));
    }
  };

  const columns = [
    {
      title: t('column.name'),
      dataIndex: 'name',
      key: 'name',
      sorter: (a: CustomRole, b: CustomRole) => a.name.localeCompare(b.name),
    },
    {
      title: t('roles.description'),
      dataIndex: 'description',
      key: 'description',
      ellipsis: true,
    },
    {
      title: t('roles.system'),
      dataIndex: 'is_system',
      key: 'is_system',
      render: (v: boolean) =>
        v ? <Tag color="blue">{t('roles.systemRole')}</Tag> : <Tag>{t('roles.customRole')}</Tag>,
    },
    {
      title: t('column.created'),
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: t('column.actions'),
      key: 'actions',
      render: (_: unknown, record: CustomRole) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => openEdit(record)}>
            {t('action.edit')}
          </Button>
          <Popconfirm
            title={t('roles.deleteRole')}
            description={t('message.cannotUndo')}
            onConfirm={() => handleDelete(record.id)}
            disabled={record.is_system}
          >
            <Button size="small" danger disabled={record.is_system}>
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
        <Typography.Title level={4} style={{ margin: 0 }}>{t('roles.title')}</Typography.Title>
        <TourGuideButton tourId="roles" />
      </div>

      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)} data-tour="roles-create">
          {t('roles.create')}
        </Button>
      </Space>

      <Table<CustomRole>
        rowKey="id"
        columns={columns}
        dataSource={roles}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true }}
        size="middle"
        scroll={{ x: 'max-content' }}
        data-tour="roles-table"
      />

      {/* Create Role Modal */}
      <Modal
        title={t('roles.create')}
        open={createOpen}
        onCancel={() => {
          setCreateOpen(false);
          createForm.resetFields();
          createMatrix.setMatrix(roleToMatrix([]));
        }}
        onOk={() => createForm.submit()}
        confirmLoading={createLoading}
        okText={t('action.create')}
        width={600}
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item
            name="name"
            label={t('column.name')}
            rules={[{ required: true, message: t('roles.nameRequired') }]}
          >
            <Input placeholder={t('roles.namePlaceholder')} />
          </Form.Item>
          <Form.Item name="description" label={t('roles.description')}>
            <Input.TextArea rows={2} placeholder={t('roles.descriptionPlaceholder')} />
          </Form.Item>
          <Form.Item label={t('roles.permissions')}>
            <PermissionMatrixEditor
              value={createMatrix.matrix}
              onChange={createMatrix.setMatrix}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* Edit Role Modal */}
      <Modal
        title={editingRole ? `${t('action.edit')} — ${editingRole.name}` : ''}
        open={editOpen}
        onCancel={() => setEditOpen(false)}
        onOk={() => editForm.submit()}
        confirmLoading={editLoading}
        okText={t('action.save')}
        width={600}
      >
        <Form form={editForm} layout="vertical" onFinish={handleEdit}>
          <Form.Item
            name="name"
            label={t('column.name')}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item name="description" label={t('roles.description')}>
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item label={t('roles.permissions')}>
            <PermissionMatrixEditor
              value={editMatrix.matrix}
              onChange={editMatrix.setMatrix}
              disabled={editingRole?.is_system}
            />
          </Form.Item>
        </Form>
      </Modal>
      <Tour
        open={tour.isActive}
        steps={getRolesSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
