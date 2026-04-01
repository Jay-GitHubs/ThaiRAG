import { useState, useEffect, useCallback } from 'react';
import {
  Button,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Progress,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
  Tour,
} from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getTenantsSteps } from '../tours/steps/tenants';
import type { Tenant, TenantQuota, TenantUsage } from '../api/tenants';
import {
  listTenants,
  createTenant,
  updateTenant,
  deleteTenant,
  getTenantQuota,
  setTenantQuota,
  getTenantUsage,
} from '../api/tenants';

const PLAN_COLORS: Record<string, string> = {
  free: 'default',
  standard: 'blue',
  premium: 'gold',
};

const PLAN_OPTIONS = [
  { label: 'Free', value: 'free' },
  { label: 'Standard', value: 'standard' },
  { label: 'Premium', value: 'premium' },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function usagePercent(current: number, max: number): number {
  if (max === 0) return 0;
  return Math.min(100, Math.round((current / max) * 100));
}

export function TenantsPage() {
  const { t } = useI18n();
  const tour = useTour('tenants');
  const [tenants, setTenants] = useState<Tenant[]>([]);
  const [loading, setLoading] = useState(false);

  // Create modal
  const [createOpen, setCreateOpen] = useState(false);
  const [createLoading, setCreateLoading] = useState(false);
  const [createForm] = Form.useForm();

  // Quota modal
  const [quotaOpen, setQuotaOpen] = useState(false);
  const [quotaLoading, setQuotaLoading] = useState(false);
  const [quotaForm] = Form.useForm();
  const [selectedTenant, setSelectedTenant] = useState<Tenant | null>(null);
  const [quota, setQuota] = useState<TenantQuota | null>(null);
  const [usage, setUsage] = useState<TenantUsage | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listTenants();
      setTenants(data);
    } catch {
      message.error(t('tenants.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  const handleCreate = async (values: { name: string; plan: string }) => {
    setCreateLoading(true);
    try {
      await createTenant(values.name, values.plan);
      message.success(t('tenants.created'));
      setCreateOpen(false);
      createForm.resetFields();
      load();
    } catch {
      message.error(t('tenants.createFailed'));
    } finally {
      setCreateLoading(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteTenant(id);
      message.success(t('tenants.deleted'));
      load();
    } catch {
      message.error(t('tenants.deleteFailed'));
    }
  };

  const openQuotaModal = async (tenant: Tenant) => {
    setSelectedTenant(tenant);
    setQuotaOpen(true);
    setQuota(null);
    setUsage(null);
    try {
      const [q, u] = await Promise.all([getTenantQuota(tenant.id), getTenantUsage(tenant.id)]);
      setQuota(q);
      setUsage(u);
      quotaForm.setFieldsValue({
        max_documents: q.max_documents,
        max_storage_bytes: q.max_storage_bytes,
        max_queries_per_day: q.max_queries_per_day,
        max_users: q.max_users,
        max_workspaces: q.max_workspaces,
        name: tenant.name,
        plan: tenant.plan,
      });
    } catch {
      message.error(t('tenants.loadFailed'));
    }
  };

  const handleSaveQuota = async (values: {
    name: string;
    plan: string;
    max_documents: number;
    max_storage_bytes: number;
    max_queries_per_day: number;
    max_users: number;
    max_workspaces: number;
  }) => {
    if (!selectedTenant) return;
    setQuotaLoading(true);
    try {
      await Promise.all([
        updateTenant(selectedTenant.id, values.name, values.plan),
        setTenantQuota(selectedTenant.id, {
          max_documents: values.max_documents,
          max_storage_bytes: values.max_storage_bytes,
          max_queries_per_day: values.max_queries_per_day,
          max_users: values.max_users,
          max_workspaces: values.max_workspaces,
        }),
      ]);
      message.success(t('tenants.saved'));
      setQuotaOpen(false);
      load();
    } catch {
      message.error(t('tenants.saveFailed'));
    } finally {
      setQuotaLoading(false);
    }
  };

  const columns = [
    {
      title: t('column.name'),
      dataIndex: 'name',
      key: 'name',
      sorter: (a: Tenant, b: Tenant) => a.name.localeCompare(b.name),
    },
    {
      title: t('tenants.plan'),
      dataIndex: 'plan',
      key: 'plan',
      render: (plan: string) => (
        <Tag color={PLAN_COLORS[plan] ?? 'default'}>{plan.toUpperCase()}</Tag>
      ),
      filters: PLAN_OPTIONS.map((p) => ({ text: p.label, value: p.value })),
      onFilter: (value: unknown, record: Tenant) => record.plan === value,
    },
    {
      title: t('column.status'),
      dataIndex: 'is_active',
      key: 'is_active',
      render: (active: boolean) => (
        <Tag color={active ? 'green' : 'red'}>
          {active ? t('status.active') : t('status.disabled')}
        </Tag>
      ),
    },
    {
      title: t('column.created'),
      dataIndex: 'created_at',
      key: 'created_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
      sorter: (a: Tenant, b: Tenant) => dayjs(a.created_at).unix() - dayjs(b.created_at).unix(),
    },
    {
      title: t('column.actions'),
      key: 'actions',
      render: (_: unknown, record: Tenant) => (
        <Space>
          <Button size="small" onClick={() => openQuotaModal(record)}>
            {t('tenants.manageQuota')}
          </Button>
          <Popconfirm
            title={t('tenants.deleteTenant')}
            description={t('message.cannotUndo')}
            onConfirm={() => handleDelete(record.id)}
          >
            <Button size="small" danger>
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
        <Typography.Title level={4} style={{ margin: 0 }}>{t('tenants.title')}</Typography.Title>
        <TourGuideButton tourId="tenants" />
      </div>

      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)} data-tour="tenants-create">
          {t('tenants.create')}
        </Button>
      </Space>

      <Table<Tenant>
        rowKey="id"
        columns={columns}
        dataSource={tenants}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true }}
        size="middle"
        scroll={{ x: 'max-content' }}
        data-tour="tenants-table"
      />

      {/* Create Tenant Modal */}
      <Modal
        title={t('tenants.create')}
        open={createOpen}
        onCancel={() => {
          setCreateOpen(false);
          createForm.resetFields();
        }}
        onOk={() => createForm.submit()}
        confirmLoading={createLoading}
        okText={t('action.create')}
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item
            name="name"
            label={t('column.name')}
            rules={[{ required: true, message: t('tenants.nameRequired') }]}
          >
            <Input placeholder={t('tenants.namePlaceholder')} />
          </Form.Item>
          <Form.Item
            name="plan"
            label={t('tenants.plan')}
            initialValue="free"
            rules={[{ required: true }]}
          >
            <Select options={PLAN_OPTIONS} />
          </Form.Item>
        </Form>
      </Modal>

      {/* Quota & Edit Modal */}
      <Modal
        data-tour="tenants-detail"
        title={selectedTenant ? `${t('tenants.manageQuota')} — ${selectedTenant.name}` : ''}
        open={quotaOpen}
        onCancel={() => setQuotaOpen(false)}
        onOk={() => quotaForm.submit()}
        confirmLoading={quotaLoading}
        okText={t('action.save')}
        width={560}
      >
        <Form form={quotaForm} layout="vertical" onFinish={handleSaveQuota}>
          <Form.Item
            name="name"
            label={t('column.name')}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item name="plan" label={t('tenants.plan')} rules={[{ required: true }]}>
            <Select options={PLAN_OPTIONS} />
          </Form.Item>

          <Typography.Text strong style={{ display: 'block', marginBottom: 8, marginTop: 8 }}>
            {t('tenants.quotaLimits')}
          </Typography.Text>

          <Form.Item name="max_documents" label={t('tenants.maxDocuments')}>
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          {usage && quota && (
            <Progress
              percent={usagePercent(usage.current_documents, quota.max_documents)}
              format={() =>
                `${usage.current_documents} / ${quota.max_documents} ${t('tenants.docs')}`
              }
              style={{ marginBottom: 12 }}
            />
          )}

          <Form.Item name="max_storage_bytes" label={t('tenants.maxStorage')}>
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          {usage && quota && (
            <Progress
              percent={usagePercent(usage.current_storage_bytes, quota.max_storage_bytes)}
              format={() =>
                `${formatBytes(usage.current_storage_bytes)} / ${formatBytes(quota.max_storage_bytes)}`
              }
              style={{ marginBottom: 12 }}
            />
          )}

          <Form.Item name="max_queries_per_day" label={t('tenants.maxQueriesPerDay')}>
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          {usage && quota && (
            <Progress
              percent={usagePercent(usage.queries_today, quota.max_queries_per_day)}
              format={() =>
                `${usage.queries_today} / ${quota.max_queries_per_day} ${t('tenants.today')}`
              }
              style={{ marginBottom: 12 }}
            />
          )}

          <Form.Item name="max_users" label={t('tenants.maxUsers')}>
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          {usage && quota && (
            <Progress
              percent={usagePercent(usage.current_users, quota.max_users)}
              format={() => `${usage.current_users} / ${quota.max_users} ${t('tenants.users')}`}
              style={{ marginBottom: 12 }}
            />
          )}

          <Form.Item name="max_workspaces" label={t('tenants.maxWorkspaces')}>
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          {usage && quota && (
            <Progress
              percent={usagePercent(usage.current_workspaces, quota.max_workspaces)}
              format={() =>
                `${usage.current_workspaces} / ${quota.max_workspaces} ${t('tenants.workspaces')}`
              }
              style={{ marginBottom: 12 }}
            />
          )}
        </Form>
      </Modal>
      <Tour
        open={tour.isActive}
        steps={getTenantsSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
