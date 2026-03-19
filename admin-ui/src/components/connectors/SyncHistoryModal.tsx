import { Modal, Table, Tag } from 'antd';
import dayjs from 'dayjs';
import { useSyncRuns } from '../../hooks/useConnectors';
import type { SyncRunResponse } from '../../api/types';

interface Props {
  connectorId: string | undefined;
  connectorName: string;
  open: boolean;
  onClose: () => void;
}

const statusColor: Record<string, string> = {
  completed: 'success',
  failed: 'error',
  running: 'processing',
};

export function SyncHistoryModal({
  connectorId,
  connectorName,
  open,
  onClose,
}: Props) {
  const { data, isLoading } = useSyncRuns(open ? connectorId : undefined);

  const columns = [
    {
      title: 'Started',
      dataIndex: 'started_at',
      key: 'started_at',
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (v: string) => (
        <Tag color={statusColor[v] || 'default'}>{v}</Tag>
      ),
    },
    {
      title: 'Duration',
      dataIndex: 'duration_secs',
      key: 'duration_secs',
      render: (v: number | null) => (v != null ? `${v.toFixed(1)}s` : '-'),
    },
    {
      title: 'Created',
      dataIndex: 'items_created',
      key: 'items_created',
    },
    {
      title: 'Updated',
      dataIndex: 'items_updated',
      key: 'items_updated',
    },
    {
      title: 'Skipped',
      dataIndex: 'items_skipped',
      key: 'items_skipped',
    },
    {
      title: 'Failed',
      dataIndex: 'items_failed',
      key: 'items_failed',
      render: (v: number) =>
        v > 0 ? <Tag color="error">{v}</Tag> : v,
    },
    {
      title: 'Error',
      dataIndex: 'error_message',
      key: 'error_message',
      ellipsis: true,
      render: (v: string | null) => v || '-',
    },
  ];

  return (
    <Modal
      title={`Sync History — ${connectorName}`}
      open={open}
      onCancel={onClose}
      footer={null}
      width={900}
    >
      <Table<SyncRunResponse>
        rowKey="id"
        columns={columns}
        dataSource={data?.data}
        loading={isLoading}
        pagination={{ pageSize: 10 }}
        size="small"
      />
    </Modal>
  );
}
