import { useState } from 'react';
import { Table, Button, Tag, Popconfirm, Space, message, Tooltip, Popover } from 'antd';
import {
  UploadOutlined, PlusOutlined, DeleteOutlined, LoadingOutlined,
  SyncOutlined, EyeOutlined, DownloadOutlined, BlockOutlined, ReloadOutlined,
  FileMarkdownOutlined, CheckCircleTwoTone, MinusCircleOutlined, CloseCircleTwoTone,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import {
  useDocuments,
  useDeleteDocument,
  useReprocessDocument,
  useReprocessAllDocuments,
} from '../../hooks/useDocuments';
import { UploadModal } from './UploadModal';
import { IngestModal } from './IngestModal';
import { PreviewModal } from './PreviewModal';
import { ChunksModal } from './ChunksModal';
import { downloadDocument, getDocumentContent } from '../../api/documents';
import type { Document, DocStatus, ProcessingProvenance } from '../../api/types';

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

// Human-readable labels for the structured `empty_extraction[<reason>]` codes
// emitted by the pipeline. Keep in sync with thairag-document::pipeline::empty_reason.
const EMPTY_REASON_LABELS: Record<string, string> = {
  no_text_vision_unavailable: 'Vision OCR Required',
  no_text_vision_failed: 'Vision OCR Failed',
  vision_budget_exceeded: 'Vision Budget Exceeded',
  no_text_no_fallback: 'No Text Extracted',
  no_chunks_after_plugins: 'No Chunks Produced',
};

/// Parse `empty_extraction[<reason>]: <hint>` into its reason code, or null
/// if the message is not a structured empty-extraction failure.
function parseEmptyExtractionReason(msg: string | null | undefined): string | null {
  if (!msg) return null;
  const match = /^empty_extraction\[([a-z_]+)\]/.exec(msg);
  return match ? match[1] : null;
}

// Human-readable labels for the agent identifiers in ProcessingProvenance.
const AGENT_LABELS: Record<string, string> = {
  analyzer: 'Analyzer',
  chunker: 'Chunker',
  enricher: 'Enricher',
  converter: 'Converter',
  quality: 'Quality',
};

function agentStatusIcon(status: string) {
  switch (status) {
    case 'ran':
      return <CheckCircleTwoTone twoToneColor="#52c41a" />;
    case 'failed':
      return <CloseCircleTwoTone twoToneColor="#ff4d4f" />;
    case 'skipped':
    default:
      return <MinusCircleOutlined style={{ color: '#bfbfbf' }} />;
  }
}

/// Detailed per-document processing breakdown shown in the Pipeline popover.
function ProvenanceDetail({ prov }: { prov: ProcessingProvenance }) {
  return (
    <div style={{ maxWidth: 360, fontSize: 13 }}>
      <div style={{ marginBottom: 8 }}>
        <strong>Path:</strong> {prov.path}
      </div>
      <div style={{ marginBottom: 8 }}>
        <strong>Agents</strong>
        <div style={{ marginTop: 4 }}>
          {prov.agents.length === 0 && <div style={{ color: '#999' }}>None (no AI agents)</div>}
          {prov.agents.map((a) => (
            <div key={a.agent} style={{ display: 'flex', alignItems: 'center', gap: 6, lineHeight: '20px' }}>
              {agentStatusIcon(a.status)}
              <span style={{ minWidth: 72, display: 'inline-block' }}>{AGENT_LABELS[a.agent] ?? a.agent}</span>
              <span style={{ color: '#555', fontFamily: 'monospace' }}>{a.model ?? '—'}</span>
              {a.note && <span style={{ color: '#999' }}>({a.note})</span>}
            </div>
          ))}
        </div>
      </div>
      <div>
        <strong>Fallback:</strong>{' '}
        {prov.mechanical_fallback ? <Tag color="warning">mechanical</Tag> : 'none'}
        <span style={{ marginLeft: 12 }}>
          <strong>Chunks:</strong> {prov.chunk_count}
        </span>
      </div>
    </div>
  );
}

/// Compact Pipeline cell: a clickable tag showing the path, with the full
/// breakdown (agents + models + fallback) in a popover.
function ProvenanceCell({ doc }: { doc: Document }) {
  const prov = doc.processing_provenance;
  if (!prov) return <span style={{ color: '#bbb' }}>—</span>;
  const usedAi = prov.agents.some((a) => a.status === 'ran' && a.model);
  const color = prov.mechanical_fallback ? 'warning' : usedAi ? 'geekblue' : 'default';
  return (
    <Popover content={<ProvenanceDetail prov={prov} />} title="Processing details" trigger="hover">
      <Tag color={color} style={{ cursor: 'help' }}>{prov.path}</Tag>
    </Popover>
  );
}

/// Conversion-fidelity badge: how faithfully the converted text (vector-DB
/// content) matches the original. Verified = nothing dropped/fabricated; Review
/// = numbers dropped/fabricated or low coverage; Unverifiable = no text layer
/// in the original (e.g. scanned).
function FidelityCell({ doc }: { doc: Document }) {
  const f = doc.processing_provenance?.fidelity;
  if (!f) return <span style={{ color: '#bbb' }}>—</span>;
  const meta: Record<string, { color: string; label: string }> = {
    verified: { color: 'success', label: 'Verified' },
    review: { color: 'warning', label: 'Review' },
    unverifiable: { color: 'default', label: 'Unverifiable' },
  };
  const m = meta[f.status] ?? { color: 'default', label: f.status };
  const detail = (
    <div style={{ maxWidth: 300, fontSize: 13, lineHeight: '20px' }}>
      {f.status !== 'unverifiable' && (
        <>
          <div>
            Score: <strong>{Math.round(f.score * 100)}%</strong>
          </div>
          <div>
            Numbers matched: {f.numbers_matched}/{f.numbers_total}
          </div>
          {f.numbers_fabricated > 0 && (
            <div style={{ color: '#cf1322' }}>
              Fabricated numbers: {f.numbers_fabricated}
            </div>
          )}
          <div>Char coverage: {Math.round(f.char_coverage * 100)}%</div>
        </>
      )}
      {f.status === 'unverifiable' && (
        <div style={{ color: '#999' }}>
          The original has no extractable text layer (e.g. a scanned PDF), so the
          conversion cannot be verified against it.
        </div>
      )}
    </div>
  );
  return (
    <Popover content={detail} title="Conversion fidelity" trigger="hover">
      <Tag color={m.color} style={{ cursor: 'help' }}>
        {m.label}
      </Tag>
    </Popover>
  );
}

export function DocumentTable({ workspaceId }: Props) {
  const { data, isLoading } = useDocuments(workspaceId);
  const deleteDoc = useDeleteDocument();
  const reprocessDoc = useReprocessDocument();
  const reprocessAll = useReprocessAllDocuments();
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

  async function handleDownloadMarkdown(doc: Document) {
    try {
      const { converted_text } = await getDocumentContent(workspaceId, doc.id);
      const blob = new Blob([converted_text ?? ''], { type: 'text/markdown;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      // Swap the original extension (e.g. .pdf) for .md.
      a.download = `${doc.title.replace(/\.[^/.]+$/, '')}.md`;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      message.error('Failed to download markdown');
    }
  }

  async function handleReprocess(doc: Document) {
    try {
      await reprocessDoc.mutateAsync({ wsId: workspaceId, docId: doc.id });
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
      case 'failed': {
        const reason = parseEmptyExtractionReason(record.error_message);
        const label = reason ? (EMPTY_REASON_LABELS[reason] ?? reason) : 'Failed';
        return (
          <Tooltip
            title={record.error_message || 'Processing failed'}
            styles={{ root: { maxWidth: 480 } }}
          >
            <Tag color={reason ? 'warning' : 'error'}>
              {label}
            </Tag>
          </Tooltip>
        );
      }
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
      title: 'Pipeline',
      key: 'pipeline',
      render: (_: unknown, record: Document) =>
        record.status === 'processing' ? <LoadingOutlined /> : <ProvenanceCell doc={record} />,
    },
    {
      title: 'Fidelity',
      key: 'fidelity',
      render: (_: unknown, record: Document) =>
        record.status === 'processing' ? <LoadingOutlined /> : <FidelityCell doc={record} />,
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
          <Tooltip title="Download markdown">
            <Button size="small" icon={<FileMarkdownOutlined />} onClick={() => handleDownloadMarkdown(record)} disabled={record.status === 'processing'} />
          </Tooltip>
          <Tooltip title="Reprocess">
            <Popconfirm title="Reprocess this document?" onConfirm={() => handleReprocess(record)}>
              <Button size="small" icon={<ReloadOutlined />} disabled={record.status === 'processing'} />
            </Popconfirm>
          </Tooltip>
          <Popconfirm title={record.status === 'processing' ? "This document is still processing. Delete anyway?" : "Delete this document?"} onConfirm={() => handleDelete(record.id)}>
            <Button danger size="small" icon={<DeleteOutlined />} />
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
        <Popconfirm
          title="Re-embed all documents?"
          description="This will reprocess all documents with the current embedding model. Use after switching embedding models."
          onConfirm={async () => {
            try {
              const result = await reprocessAll.mutateAsync({ wsId: workspaceId });
              message.success(result.message);
            } catch {
              message.error('Failed to start reprocessing');
            }
          }}
        >
          <Tooltip title="Re-embed all documents (use after changing embedding model)">
            <Button icon={<ReloadOutlined />}>Re-embed All</Button>
          </Tooltip>
        </Popconfirm>
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
        scroll={{ x: 'max-content' }}
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
