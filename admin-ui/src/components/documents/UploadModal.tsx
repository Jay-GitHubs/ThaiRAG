import { useState } from 'react';
import { Modal, Upload, Input, message } from 'antd';
import { InboxOutlined } from '@ant-design/icons';
import { useUploadDocument } from '../../hooks/useDocuments';
import type { UploadFile } from 'antd/es/upload';

interface Props {
  workspaceId: string;
  open: boolean;
  onClose: () => void;
}

export function UploadModal({ workspaceId, open, onClose }: Props) {
  const [fileList, setFileList] = useState<UploadFile[]>([]);
  const [title, setTitle] = useState('');
  const upload = useUploadDocument();

  async function handleOk() {
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
      message.success(`Uploaded: ${res.chunks} chunks indexed`);
      setFileList([]);
      setTitle('');
      onClose();
    } catch {
      message.error('Upload failed');
    }
  }

  return (
    <Modal
      title="Upload Document"
      open={open}
      onOk={handleOk}
      onCancel={onClose}
      confirmLoading={upload.isPending}
    >
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
    </Modal>
  );
}
