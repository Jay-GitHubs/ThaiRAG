import { useState } from 'react';
import { Modal, Upload, Input, message, Button, Spin } from 'antd';
import { InboxOutlined } from '@ant-design/icons';
import { useUploadDocument, useDocument } from '../../hooks/useDocuments';
import { ProcessingTimeline } from './ProcessingTimeline';
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
  const upload = useUploadDocument();
  const { data: trackedDoc } = useDocument(workspaceId, trackingDocId ?? undefined, !!trackingDocId);

  function reset() {
    setFileList([]);
    setTitle('');
    setTrackingDocId(null);
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
        <Button key="upload" type="primary" loading={upload.isPending} onClick={handleUpload}>
          Upload
        </Button>,
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
            onChange={({ fileList }) => setFileList(fileList)}
            accept=".txt,.md,.html,.pdf,.csv,.json,.docx,.xlsx"
          >
            <p className="ant-upload-drag-icon">
              <InboxOutlined />
            </p>
            <p className="ant-upload-text">Click or drag file to this area</p>
            <p className="ant-upload-hint">Max 10MB. Supported: txt, md, html, pdf, csv, json, docx, xlsx</p>
          </Upload.Dragger>
        </>
      )}
    </Modal>
  );
}
