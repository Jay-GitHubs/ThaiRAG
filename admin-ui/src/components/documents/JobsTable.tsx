import { Button, Popconfirm, Space, Table, Tag, Tooltip, Typography, message } from 'antd';
import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleOutlined,
  LoadingOutlined,
  StopOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { useJobsStream, useCancelJob } from '../../hooks/useJobs';
import type { Job, JobKind, JobStatus } from '../../api/types';

const statusConfig: Record<JobStatus, { color: string; icon: React.ReactNode; label: string }> = {
  queued: { color: 'default', icon: <ClockCircleOutlined />, label: 'Queued' },
  running: { color: 'processing', icon: <LoadingOutlined />, label: 'Running' },
  completed: { color: 'success', icon: <CheckCircleOutlined />, label: 'Completed' },
  failed: { color: 'error', icon: <CloseCircleOutlined />, label: 'Failed' },
  cancelled: { color: 'warning', icon: <StopOutlined />, label: 'Cancelled' },
};

const kindLabels: Record<JobKind, string> = {
  document_ingestion: 'Ingestion',
  document_reprocess: 'Reprocess',
  batch_reprocess: 'Batch Reprocess',
};

interface Props {
  workspaceId: string;
}

export function JobsTable({ workspaceId }: Props) {
  const { data, isLoading } = useJobsStream(workspaceId);
  const cancelMut = useCancelJob();

  const handleCancel = async (jobId: string) => {
    try {
      await cancelMut.mutateAsync({ wsId: workspaceId, jobId });
      message.success('Job cancelled');
    } catch {
      message.error('Failed to cancel job');
    }
  };

  const columns = [
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      width: 120,
      render: (status: JobStatus, record: Job) => {
        const cfg = statusConfig[status] || statusConfig.queued;
        return (
          <Tooltip title={record.error}>
            <Tag icon={cfg.icon} color={cfg.color}>
              {cfg.label}
            </Tag>
          </Tooltip>
        );
      },
    },
    {
      title: 'Type',
      dataIndex: 'kind',
      key: 'kind',
      width: 140,
      render: (kind: JobKind) => kindLabels[kind] || kind,
    },
    {
      title: 'Description',
      dataIndex: 'description',
      key: 'description',
      ellipsis: true,
    },
    {
      title: 'Items',
      dataIndex: 'items_processed',
      key: 'items_processed',
      width: 80,
      render: (n: number, record: Job) =>
        record.status === 'completed' ? n : '-',
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 160,
      render: (ts: number) => dayjs.unix(ts).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: 'Duration',
      key: 'duration',
      width: 100,
      render: (_: unknown, record: Job) => {
        const start = record.started_at;
        const end = record.completed_at;
        if (!start) return '-';
        const elapsed = (end || dayjs().unix()) - start;
        if (elapsed < 60) return `${elapsed}s`;
        return `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`;
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 80,
      render: (_: unknown, record: Job) => {
        const canCancel = record.status === 'queued' || record.status === 'running';
        return canCancel ? (
          <Space size="small">
            <Popconfirm
              title="Cancel this job?"
              onConfirm={() => handleCancel(record.id)}
            >
              <Button size="small" danger>
                Cancel
              </Button>
            </Popconfirm>
          </Space>
        ) : null;
      },
    },
  ];

  const jobs = data?.jobs || [];

  if (jobs.length === 0 && !isLoading) {
    return null; // Don't show empty table
  }

  return (
    <div style={{ marginBottom: 24 }}>
      <Typography.Text strong style={{ display: 'block', marginBottom: 8 }}>
        Background Jobs
      </Typography.Text>
      <Table<Job>
        rowKey="id"
        columns={columns}
        dataSource={jobs}
        loading={isLoading}
        pagination={false}
        size="small"
        scroll={{ x: 'max-content' }}
      />
    </div>
  );
}
