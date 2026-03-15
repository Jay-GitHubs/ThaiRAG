import { useState } from 'react';
import { Table, Button, Tag, Popconfirm, Space, message, Tooltip } from 'antd';
import {
  UploadOutlined, PlusOutlined, DeleteOutlined, LoadingOutlined,
  SyncOutlined, EyeOutlined, DownloadOutlined, BlockOutlined, ReloadOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { useDocuments, useDeleteDocument } from '../../hooks/useDocuments';
import { UploadModal } from './UploadModal';
import { IngestModal } from './IngestModal';
import { PreviewModal } from './PreviewModal';
import { ChunksModal } from './ChunksModal';
import { downloadDocument, reprocessDocument } from '../../api/documents';
import type { Document, DocStatus } from '../../api/types';

interface Props {
  workspaceId: string;
}

const STEP_LABELS: Record<string, string> = {
  analyzing: 'Analyzing',
  converting: 'Converting',
  checking_quality: 'Quality Check',
  chunking: 'Chunking',
  indexing: 'Indexing',
};

export function DocumentTable({ workspaceId }: Props) {
  const { data, isLoading } = useDocuments(workspaceId);
  const deleteDoc = useDeleteDocument();
  const [uploadOpen, setUploadOpen] = useState(false);
  const [ingestOpen, setIngestOpen] = useState(false);
  const [previewDoc, setPreviewDoc] = useState<Document | null>(null);
  const [chunksDoc, setChunksDoc] = useState<Document | null>(null);

  // Auto-refresh when any document is processing
  const hasProcessing = data?.data?.some((d) => d.status === 'processing');

  // useDocuments already handles refetch — we just need to trigger polling
  // We'll pass refetchInterval option via the hook (see below)

  async function handleDelete(docId: string) {
    try {
      await deleteDoc.mutateAsync({ wsId: workspaceId, docId });
      message.success('Document deleted');
    } catch {
      message.error('Failed to delete document');
    }
  }

  async function handleDownload(doc: Document) {
    try {
      const blob = await downloadDocument(workspaceId, doc.id);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = doc.title;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      message.error('Failed to download document');
    }
  }

  async function handleReprocess(doc: Document) {
    try {
      await reprocessDocument(workspaceId, doc.id);
      message.success('Reprocessing started');
    } catch {
      message.error('Failed to reprocess document');
    }
  }

  function renderStatus(status: DocStatus, record: Document) {
    switch (status) {
      case 'processing':
        return (
          <Tooltip title={record.processing_step ? `Step: ${STEP_LABELS[record.processing_step] || record.processing_step}` : 'Processing...'}>
            <Tag icon={<SyncOutlined spin />} color="processing">
              {record.processing_step ? STEP_LABELS[record.processing_step] || record.processing_step : 'Processing'}
            </Tag>
          </Tooltip>
        );
      case 'ready':
        return <Tag color="success">Ready</Tag>;
      case 'failed':
        return (
          <Tooltip title={record.error_message || 'Processing failed'}>
            <Tag color="error">Failed</Tag>
          </Tooltip>
        );
      default:
        return <Tag>{status}</Tag>;
    }
  }

  const columns = [
    { title: 'Title', dataIndex: 'title', key: 'title' },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (status: DocStatus, record: Document) => renderStatus(status, record),
    },
    {
      title: 'Chunks',
      dataIndex: 'chunk_count',
      key: 'chunk_count',
      render: (v: number, record: Document) =>
        record.status === 'processing' ? <LoadingOutlined /> : v,
    },
    {
      title: 'MIME Type',
      dataIndex: 'mime_type',
      key: 'mime_type',
      render: (v: string) => <Tag>{v}</Tag>,
    },
    {
      title: 'Size',
      dataIndex: 'size_bytes',
      key: 'size_bytes',
      render: (v: number) => formatBytes(v),
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
      render: (_: unknown, record: Document) => (
        <Space size="small">
          <Tooltip title="Preview content">
            <Button size="small" icon={<EyeOutlined />} onClick={() => setPreviewDoc(record)} disabled={record.status === 'processing'} />
          </Tooltip>
          <Tooltip title="View chunks">
            <Button size="small" icon={<BlockOutlined />} onClick={() => setChunksDoc(record)} disabled={record.status === 'processing'} />
          </Tooltip>
          <Tooltip title="Download original">
            <Button size="small" icon={<DownloadOutlined />} onClick={() => handleDownload(record)} disabled={record.status === 'processing'} />
          </Tooltip>
          <Tooltip title="Reprocess">
            <Popconfirm title="Reprocess this document?" onConfirm={() => handleReprocess(record)}>
              <Button size="small" icon={<ReloadOutlined />} disabled={record.status === 'processing'} />
            </Popconfirm>
          </Tooltip>
          <Popconfirm title="Delete this document?" onConfirm={() => handleDelete(record.id)}>
            <Button danger size="small" icon={<DeleteOutlined />} disabled={record.status === 'processing'} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <>
      <Space style={{ marginBottom: 16 }}>
        <Button icon={<UploadOutlined />} onClick={() => setUploadOpen(true)}>
          Upload File
        </Button>
        <Button icon={<PlusOutlined />} onClick={() => setIngestOpen(true)}>
          Ingest Text
        </Button>
        {hasProcessing && (
          <Tag icon={<SyncOutlined spin />} color="processing">
            Documents processing...
          </Tag>
        )}
      </Space>

      <Table<Document>
        rowKey="id"
        columns={columns}
        dataSource={data?.data}
        loading={isLoading}
        pagination={{ pageSize: 20 }}
      />

      <UploadModal
        workspaceId={workspaceId}
        open={uploadOpen}
        onClose={() => setUploadOpen(false)}
      />
      <IngestModal
        workspaceId={workspaceId}
        open={ingestOpen}
        onClose={() => setIngestOpen(false)}
      />
      {previewDoc && (
        <PreviewModal
          workspaceId={workspaceId}
          doc={previewDoc}
          open={!!previewDoc}
          onClose={() => setPreviewDoc(null)}
        />
      )}
      {chunksDoc && (
        <ChunksModal
          workspaceId={workspaceId}
          doc={chunksDoc}
          open={!!chunksDoc}
          onClose={() => setChunksDoc(null)}
        />
      )}
    </>
  );
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}
