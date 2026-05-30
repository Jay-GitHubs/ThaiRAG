import { useCallback, useEffect, useState } from 'react';
import { Modal, Spin, List, Tag, Typography, Empty, Alert, Button, theme } from 'antd';
import axios from 'axios';
import { getDocumentChunks } from '../../api/documents';
import type { ChunkInfo, ChunksResponse, Document } from '../../api/types';

interface Props {
  workspaceId: string;
  doc: Document;
  open: boolean;
  onClose: () => void;
}

export function ChunksModal({ workspaceId, doc, open, onClose }: Props) {
  const [loading, setLoading] = useState(true);
  const [data, setData] = useState<ChunksResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { token: themeToken } = theme.useToken();

  const load = useCallback(() => {
    setLoading(true);
    setError(null);
    getDocumentChunks(workspaceId, doc.id)
      .then((res) => {
        setData(res);
        setError(null);
      })
      .catch((err) => {
        setData(null);
        if (axios.isAxiosError(err) && (err.code === 'ECONNABORTED' || !err.response)) {
          setError(
            'Timed out loading chunks. The server may be busy reprocessing or not responding — try again shortly.',
          );
        } else {
          setError('Failed to load chunks. Please try again.');
        }
      })
      .finally(() => setLoading(false));
  }, [workspaceId, doc.id]);

  useEffect(() => {
    if (open) load();
  }, [open, load]);

  return (
    <Modal
      title={`Chunks: ${doc.title} (${data?.total ?? 0} chunks)`}
      open={open}
      onCancel={onClose}
      footer={null}
      width={800}
    >
      {loading ? (
        <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
      ) : error ? (
        <Alert
          type="error"
          showIcon
          message="Could not load chunks"
          description={error}
          action={
            <Button size="small" onClick={load}>
              Retry
            </Button>
          }
        />
      ) : data && data.chunks.length > 0 ? (
        <List
          dataSource={data.chunks}
          style={{ maxHeight: 500, overflow: 'auto' }}
          renderItem={(chunk: ChunkInfo) => (
            <List.Item>
              <List.Item.Meta
                title={
                  <span>
                    <Tag color="blue">#{chunk.index}</Tag>
                    {chunk.page !== null && <Tag color="orange">Page {chunk.page}</Tag>}
                    <Typography.Text type="secondary" style={{ fontSize: 11, marginLeft: 8 }}>
                      {chunk.chunk_id.substring(0, 8)}...
                    </Typography.Text>
                  </span>
                }
                description={
                  <div
                    style={{
                      fontFamily: 'monospace',
                      fontSize: 12,
                      lineHeight: 1.5,
                      whiteSpace: 'pre-wrap',
                      wordBreak: 'break-word',
                      maxHeight: 150,
                      overflow: 'auto',
                      background: themeToken.colorBgLayout,
                      color: themeToken.colorText,
                      padding: 8,
                      borderRadius: 4,
                    }}
                  >
                    {chunk.text || '(empty)'}
                  </div>
                }
              />
            </List.Item>
          )}
        />
      ) : (
        <Empty description={<Typography.Text type="secondary">No chunks found</Typography.Text>} />
      )}
    </Modal>
  );
}
