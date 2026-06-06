import { useEffect, useState } from 'react';
import { Modal, Spin, Tag, Space, Typography, Empty, theme } from 'antd';
import { FileImageOutlined, TableOutlined } from '@ant-design/icons';
import DOMPurify from 'dompurify';
import { getDocumentContent } from '../../api/documents';
import type { Document, DocumentContentResponse } from '../../api/types';

// Reconstructed tables are stored as HTML inside the converted text. Render
// them, but strip everything else (incl. any HTML in the document's own text)
// via a strict allowlist — guaranteed-safe regardless of the table's origin.
function sanitizeDocHtml(text: string): string {
  return DOMPurify.sanitize(text, {
    ALLOWED_TAGS: ['table', 'thead', 'tbody', 'tr', 'td', 'th'],
    ALLOWED_ATTR: ['colspan', 'rowspan'],
  });
}

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
          <style>{`.doc-preview table{border-collapse:collapse;margin:.5rem 0;white-space:normal}
.doc-preview td,.doc-preview th{border:1px solid ${themeToken.colorBorderSecondary};padding:.3rem .5rem;vertical-align:top}`}</style>
          <div
            className="doc-preview"
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
            // Sanitised: only table tags survive; prose shows as pre-wrapped text.
            dangerouslySetInnerHTML={{ __html: sanitizeDocHtml(content.converted_text) }}
          />
        </>
      ) : (
        <Empty description={<Typography.Text type="secondary">No converted content available</Typography.Text>} />
      )}
    </Modal>
  );
}
