import { useState, useEffect } from 'react';
import { Modal, Upload, Input, message, Button, Spin, Radio, InputNumber, Space, Tooltip } from 'antd';
import { InboxOutlined } from '@ant-design/icons';
import { useUploadDocument, useDocument } from '../../hooks/useDocuments';
import { ProcessingTimeline } from './ProcessingTimeline';
import { DocumentPreviewPanel } from './DocumentPreviewPanel';
import { previewDocument } from '../../api/documents';
import { getDocumentConfig } from '../../api/settings';
import type { DocumentPreview, DocumentHandling } from '../../api/types';
import type { UploadFile } from 'antd/es/upload';

interface Props {
  workspaceId: string;
  open: boolean;
  onClose: () => void;
}

export function UploadModal({ workspaceId, open, onClose }: Props) {
  const [fileList, setFileList] = useState<UploadFile[]>([]);
  const [title, setTitle] = useState('');
  // Once a file is uploaded we keep the modal open and switch into a live
  // tracker that polls this document and renders its per-stage progress.
  const [trackingDocId, setTrackingDocId] = useState<string | null>(null);
  // Optional pre-ingest analysis: what the pipeline WOULD do (dry-run, no cost).
  const [preview, setPreview] = useState<DocumentPreview | null>(null);
  const [previewing, setPreviewing] = useState(false);
  // Per-document handling override (admin's choice for this ingest).
  const [handlingMode, setHandlingMode] = useState<DocumentHandling['handling_mode']>('auto');
  const [covThreshold, setCovThreshold] = useState<number | null>(null);
  const [minChars, setMinChars] = useState<number | null>(null);
  // When the admin policy requires it, ingest is gated behind a preview.
  const [alwaysPreview, setAlwaysPreview] = useState(false);
  const upload = useUploadDocument();

  useEffect(() => {
    if (!open) return;
    getDocumentConfig()
      .then((c) => setAlwaysPreview(c.always_preview))
      .catch(() => setAlwaysPreview(false));
  }, [open]);
  const { data: trackedDoc } = useDocument(workspaceId, trackingDocId ?? undefined, !!trackingDocId);

  function reset() {
    setFileList([]);
    setTitle('');
    setTrackingDocId(null);
    setPreview(null);
    setHandlingMode('auto');
    setCovThreshold(null);
    setMinChars(null);
  }

  async function handlePreview() {
    const file = fileList[0]?.originFileObj;
    if (!file) {
      message.warning('Please select a file');
      return;
    }
    setPreviewing(true);
    try {
      setPreview(await previewDocument(workspaceId, file));
    } catch {
      message.error('Preview failed');
    } finally {
      setPreviewing(false);
    }
  }

  function handleClose() {
    reset();
    onClose();
  }

  async function handleUpload() {
    const file = fileList[0]?.originFileObj;
    if (!file) {
      message.warning('Please select a file');
      return;
    }
    try {
      const res = await upload.mutateAsync({
        wsId: workspaceId,
        file,
        title: title || undefined,
        handling: {
          handling_mode: handlingMode,
          image_coverage_threshold: covThreshold ?? undefined,
          min_chars_per_page: minChars ?? undefined,
        },
      });
      // Stay open and track the document live regardless of inline/background.
      setTrackingDocId(res.doc_id);
    } catch {
      message.error('Upload failed');
    }
  }

  const tracking = !!trackingDocId;

  const footer = tracking
    ? [
        <Button key="close" type="primary" onClick={handleClose}>
          Done
        </Button>,
      ]
    : [
        <Button key="cancel" onClick={handleClose}>
          Cancel
        </Button>,
        <Button
          key="preview"
          loading={previewing}
          disabled={fileList.length === 0}
          onClick={handlePreview}
        >
          Preview analysis
        </Button>,
        <Tooltip
          key="upload"
          title={
            alwaysPreview && !preview
              ? 'Admin policy: run "Preview analysis" before ingesting'
              : ''
          }
        >
          <Button
            type="primary"
            loading={upload.isPending}
            disabled={alwaysPreview && !preview}
            onClick={handleUpload}
          >
            {preview ? 'Ingest anyway' : 'Upload'}
          </Button>
        </Tooltip>,
      ];

  return (
    <Modal
      title={tracking ? 'Processing Document' : 'Upload Document'}
      open={open}
      onCancel={handleClose}
      maskClosable={!tracking}
      footer={footer}
    >
      {tracking ? (
        trackedDoc ? (
          <ProcessingTimeline doc={trackedDoc} />
        ) : (
          <div style={{ textAlign: 'center', padding: 32 }}>
            <Spin />
          </div>
        )
      ) : (
        <>
          <Input
            placeholder="Title (optional, defaults to filename)"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            style={{ marginBottom: 16 }}
          />
          <Upload.Dragger
            maxCount={1}
            fileList={fileList}
            beforeUpload={() => false}
            onChange={({ fileList }) => {
              setFileList(fileList);
              setPreview(null); // a new file invalidates any prior analysis
            }}
            accept=".txt,.md,.html,.pdf,.csv,.json,.docx,.xlsx,.odt"
          >
            <p className="ant-upload-drag-icon">
              <InboxOutlined />
            </p>
            <p className="ant-upload-text">Click or drag file to this area</p>
            <p className="ant-upload-hint">
              Max 10MB. Supported: txt, md, html, pdf, csv, json, docx, xlsx, odt
            </p>
          </Upload.Dragger>
          {preview && <DocumentPreviewPanel preview={preview} />}

          {/* Per-document handling override (admin's choice for this ingest). */}
          <div style={{ marginTop: 16 }}>
            <div style={{ fontSize: 12, color: '#888', marginBottom: 6 }}>Handling</div>
            <Radio.Group
              value={handlingMode}
              onChange={(e) => setHandlingMode(e.target.value)}
              optionType="button"
              size="small"
            >
              <Tooltip title="Adaptive routing (recommended)">
                <Radio.Button value="auto">Auto</Radio.Button>
              </Tooltip>
              <Tooltip title="OCR every page via the vision model — max fidelity, slowest">
                <Radio.Button value="high_quality">High quality</Radio.Button>
              </Tooltip>
              <Tooltip title="Deterministic OCR tier only — no vision LLM (no hallucination)">
                <Radio.Button value="force_ocr">OCR only</Radio.Button>
              </Tooltip>
              <Tooltip title="No models — text layer only (fast, zero cost/risk)">
                <Radio.Button value="text_only">Text only</Radio.Button>
              </Tooltip>
            </Radio.Group>
            <Space size="large" style={{ marginTop: 10, display: 'flex', flexWrap: 'wrap' }}>
              <span style={{ fontSize: 12 }}>
                <Tooltip title="Override the image-coverage threshold for this document (blank = default)">
                  <span style={{ color: '#888', marginRight: 6 }}>Image-coverage ≥</span>
                </Tooltip>
                <InputNumber
                  size="small"
                  min={0}
                  max={1}
                  step={0.05}
                  value={covThreshold}
                  placeholder={preview ? String(preview.thresholds.image_coverage_threshold) : '0.5'}
                  onChange={(v) => setCovThreshold(v ?? null)}
                  style={{ width: 90 }}
                />
              </span>
              <span style={{ fontSize: 12 }}>
                <Tooltip title="Override the min-chars/page threshold for this document (blank = default)">
                  <span style={{ color: '#888', marginRight: 6 }}>Min chars/page</span>
                </Tooltip>
                <InputNumber
                  size="small"
                  min={0}
                  max={100000}
                  value={minChars}
                  placeholder={preview ? String(preview.thresholds.min_chars_per_page) : '50'}
                  onChange={(v) => setMinChars(v ?? null)}
                  style={{ width: 90 }}
                />
              </span>
            </Space>
          </div>
        </>
      )}
    </Modal>
  );
}
