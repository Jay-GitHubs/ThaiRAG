import { useEffect, useState } from 'react';
import { Modal, Spin, Tag, Space, Typography, Empty, theme } from 'antd';
import { FileImageOutlined, TableOutlined } from '@ant-design/icons';
import { getDocumentContent } from '../../api/documents';
import type { Document, DocumentContentResponse } from '../../api/types';

interface Props {
  workspaceId: string;
  doc: Document;
  open: boolean;
  onClose: () => void;
}

export function PreviewModal({ workspaceId, doc, open, onClose }: Props) {
  const [loading, setLoading] = useState(true);
  const [content, setContent] = useState<DocumentContentResponse | null>(null);
  const { token: themeToken } = theme.useToken();

  useEffect(() => {
    if (open) {
      setLoading(true);
      getDocumentContent(workspaceId, doc.id)
        .then(setContent)
        .catch(() => setContent(null))
        .finally(() => setLoading(false));
    }
  }, [open, workspaceId, doc.id]);

  return (
    <Modal
      title={`Preview: ${doc.title}`}
      open={open}
      onCancel={onClose}
      footer={null}
      width={800}
    >
      {loading ? (
        <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
      ) : content?.converted_text ? (
        <>
          <Space style={{ marginBottom: 12 }}>
            <Tag icon={<FileImageOutlined />} color="blue">
              {content.image_count} image{content.image_count !== 1 ? 's' : ''}
            </Tag>
            <Tag icon={<TableOutlined />} color="green">
              {content.table_count} table{content.table_count !== 1 ? 's' : ''}
            </Tag>
            <Tag>{doc.mime_type}</Tag>
          </Space>
          <div
            style={{
              maxHeight: 500,
              overflow: 'auto',
              background: themeToken.colorBgLayout,
              color: themeToken.colorText,
              padding: 16,
              borderRadius: 8,
              border: `1px solid ${themeToken.colorBorderSecondary}`,
              fontFamily: 'monospace',
              fontSize: 13,
              lineHeight: 1.6,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          >
            {content.converted_text}
          </div>
        </>
      ) : (
        <Empty description={<Typography.Text type="secondary">No converted content available</Typography.Text>} />
      )}
    </Modal>
  );
}
