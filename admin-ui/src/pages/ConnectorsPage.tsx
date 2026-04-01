import { useState } from 'react';
import {
  Button,
  Popconfirm,
  Space,
  Table,
  Tag,
  Tooltip,
  Tour,
  Typography,
  message,
} from 'antd';
import {
  ApiOutlined,
  DeleteOutlined,
  EditOutlined,
  HistoryOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  SyncOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';

import type { Connector } from '../api/types';
import {
  useConnectors,
  useConnectorTemplates,
  useCreateConnector,
  useCreateFromTemplate,
  useDeleteConnector,
  usePauseConnector,
  useResumeConnector,
  useTestConnection,
  useTriggerSync,
  useUpdateConnector,
} from '../hooks/useConnectors';
import { ConnectorFormModal } from '../components/connectors/ConnectorFormModal';
import { TemplatePickerModal } from '../components/connectors/TemplatePickerModal';
import { SyncHistoryModal } from '../components/connectors/SyncHistoryModal';
import { cronToHuman } from '../components/connectors/CronPicker';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getConnectorsSteps } from '../tours/steps/connectors';

const statusColor: Record<string, string> = {
  active: 'success',
  paused: 'warning',
  error: 'error',
};

const syncStatusColor: Record<string, string> = {
  completed: 'success',
  failed: 'error',
  running: 'processing',
};

export function ConnectorsPage() {
  const { data, isLoading } = useConnectors();
  const { data: templates } = useConnectorTemplates();
  const createMut = useCreateConnector();
  const createFromTplMut = useCreateFromTemplate();
  const updateMut = useUpdateConnector();
  const deleteMut = useDeleteConnector();
  const syncMut = useTriggerSync();
  const pauseMut = usePauseConnector();
  const resumeMut = useResumeConnector();
  const testMut = useTestConnection();

  const { t } = useI18n();
  const tour = useTour('connectors');

  const [formOpen, setFormOpen] = useState(false);
  const [templateOpen, setTemplateOpen] = useState(false);
  const [editingConnector, setEditingConnector] = useState<Connector | null>(null);
  const [historyConnector, setHistoryConnector] = useState<Connector | null>(null);

  const handleCreate = async (values: Record<string, unknown>) => {
    try {
      await createMut.mutateAsync(values as never);
      message.success('Connector created');
      setFormOpen(false);
    } catch {
      message.error('Failed to create connector');
    }
  };

  const handleUpdate = async (values: Record<string, unknown>) => {
    if (!editingConnector) return;
    try {
      await updateMut.mutateAsync({ id: editingConnector.id, data: values as never });
      message.success('Connector updated');
      setFormOpen(false);
      setEditingConnector(null);
    } catch {
      message.error('Failed to update connector');
    }
  };

  const handleCreateFromTemplate = async (values: {
    template_id: string;
    workspace_id: string;
    name?: string;
    env: Record<string, string>;
    sync_mode: string;
    schedule_cron?: string;
  }) => {
    try {
      await createFromTplMut.mutateAsync(values);
      message.success('Connector created from template');
      setTemplateOpen(false);
    } catch {
      message.error('Failed to create connector');
    }
  };

  const handleSync = async (id: string) => {
    try {
      const run = await syncMut.mutateAsync(id);
      if (run.status === 'completed') {
        message.success(
          `Sync completed: ${run.items_created} created, ${run.items_updated} updated`,
        );
      } else {
        message.warning(`Sync finished with status: ${run.status}`);
      }
    } catch {
      message.error('Sync failed');
    }
  };

  const handleTest = async (id: string) => {
    try {
      const result = await testMut.mutateAsync(id);
      message.success(`Connection OK — ${result.resources.length} resources found`);
    } catch {
      message.error('Connection test failed');
    }
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: Connector) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{name}</Typography.Text>
          {record.description && (
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {record.description}
            </Typography.Text>
          )}
        </Space>
      ),
    },
    {
      title: 'Transport',
      dataIndex: 'transport',
      key: 'transport',
      render: (v: string) => <Tag>{v.toUpperCase()}</Tag>,
    },
    {
      title: 'Sync Mode',
      dataIndex: 'sync_mode',
      key: 'sync_mode',
      render: (v: string, record: Connector) => (
        <Space direction="vertical" size={0}>
          <Tag>{v === 'scheduled' ? 'Scheduled' : 'On Demand'}</Tag>
          {record.schedule_cron && (
            <Tooltip title={record.schedule_cron}>
              <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                {cronToHuman(record.schedule_cron)}
              </Typography.Text>
            </Tooltip>
          )}
        </Space>
      ),
    },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (v: string) => <Tag color={statusColor[v] || 'default'}>{v}</Tag>,
    },
    {
      title: 'Last Sync',
      key: 'last_sync',
      render: (_: unknown, record: Connector) => {
        if (!record.last_sync_at)
          return <Typography.Text type="secondary">Never</Typography.Text>;
        return (
          <Space direction="vertical" size={0}>
            <Tag color={syncStatusColor[record.last_sync_status ?? ''] || 'default'}>
              {record.last_sync_status}
            </Tag>
            <Typography.Text type="secondary" style={{ fontSize: 11 }}>
              {dayjs(record.last_sync_at).format('MM-DD HH:mm')}
            </Typography.Text>
          </Space>
        );
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 260,
      render: (_: unknown, record: Connector) => (
        <Space size="small" wrap>
          <Tooltip title="Trigger Sync">
            <Button
              size="small"
              icon={<SyncOutlined />}
              onClick={() => handleSync(record.id)}
              loading={syncMut.isPending}
            />
          </Tooltip>
          <Tooltip title="Test Connection">
            <Button
              size="small"
              icon={<ThunderboltOutlined />}
              onClick={() => handleTest(record.id)}
              loading={testMut.isPending}
            />
          </Tooltip>
          <Tooltip title="Sync History">
            <Button
              size="small"
              icon={<HistoryOutlined />}
              onClick={() => setHistoryConnector(record)}
            />
          </Tooltip>
          {record.status === 'active' ? (
            <Tooltip title="Pause">
              <Button
                size="small"
                icon={<PauseCircleOutlined />}
                onClick={() => pauseMut.mutate(record.id)}
              />
            </Tooltip>
          ) : (
            <Tooltip title="Resume">
              <Button
                size="small"
                icon={<PlayCircleOutlined />}
                onClick={() => resumeMut.mutate(record.id)}
              />
            </Tooltip>
          )}
          <Tooltip title="Edit">
            <Button
              size="small"
              icon={<EditOutlined />}
              onClick={() => {
                setEditingConnector(record);
                setFormOpen(true);
              }}
            />
          </Tooltip>
          <Popconfirm
            title="Delete this connector?"
            onConfirm={() => deleteMut.mutate(record.id)}
          >
            <Tooltip title="Delete">
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Typography.Title level={4}>
          <ApiOutlined /> MCP Connectors
        </Typography.Title>
        <TourGuideButton tourId="connectors" />
      </div>

      <Space style={{ marginBottom: 16 }} data-tour="connectors-add">
        <Button
          type="primary"
          icon={<PlusOutlined />}
          onClick={() => {
            setEditingConnector(null);
            setFormOpen(true);
          }}
        >
          Create Connector
        </Button>
        <Button icon={<PlusOutlined />} onClick={() => setTemplateOpen(true)}>
          From Template
        </Button>
      </Space>

      <Table<Connector>
        data-tour="connectors-list"
        rowKey="id"
        columns={columns}
        dataSource={data?.data}
        loading={isLoading}
        pagination={{ pageSize: 20 }}
      />

      <ConnectorFormModal
        open={formOpen}
        editingConnector={editingConnector}
        onCancel={() => {
          setFormOpen(false);
          setEditingConnector(null);
        }}
        onSubmit={editingConnector ? handleUpdate : handleCreate}
        loading={createMut.isPending || updateMut.isPending}
      />

      <TemplatePickerModal
        open={templateOpen}
        templates={templates ?? []}
        onCancel={() => setTemplateOpen(false)}
        onSubmit={handleCreateFromTemplate}
        loading={createFromTplMut.isPending}
      />

      <SyncHistoryModal
        connectorId={historyConnector?.id}
        connectorName={historyConnector?.name ?? ''}
        open={!!historyConnector}
        onClose={() => setHistoryConnector(null)}
      />
      <Tour
        open={tour.isActive}
        steps={getConnectorsSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
