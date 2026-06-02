import { useCallback, useEffect, useState } from 'react';
import { Modal, Spin, List, Tag, Typography, Empty, Alert, Button, Image, theme } from 'antd';
import axios from 'axios';
import {
  fetchDocumentImageBlob,
  getDocumentChunks,
  listDocumentImages,
} from '../../api/documents';
import type {
  ChunkInfo,
  ChunksResponse,
  Document,
  DocumentImageInfo,
} from '../../api/types';

interface Props {
  workspaceId: string;
  doc: Document;
  open: boolean;
  onClose: () => void;
}

/**
 * Renders one extracted image. The image endpoint is JWT-gated, so we fetch the
 * bytes through the authed client and show an object URL (revoked on unmount).
 */
function AuthImage({
  workspaceId,
  docId,
  info,
}: {
  workspaceId: string;
  docId: string;
  info: DocumentImageInfo;
}) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const { token: themeToken } = theme.useToken();

  useEffect(() => {
    let active = true;
    let url: string | null = null;
    fetchDocumentImageBlob(workspaceId, docId, info.image_id)
      .then((blob) => {
        if (!active) return;
        url = URL.createObjectURL(blob);
        setSrc(url);
      })
      .catch(() => {
        if (active) setFailed(true);
      });
    return () => {
      active = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [workspaceId, docId, info.image_id]);

  const caption = [
    info.page_num !== null ? `Page ${info.page_num}` : null,
    info.width && info.height ? `${info.width}×${info.height}` : null,
    info.source,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <div style={{ width: 160 }}>
      <div
        style={{
          width: 160,
          height: 120,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: themeToken.colorBgLayout,
          borderRadius: 4,
          overflow: 'hidden',
        }}
      >
        {failed ? (
          <Typography.Text type="secondary" style={{ fontSize: 11 }}>
            Failed to load
          </Typography.Text>
        ) : src ? (
          <Image
            src={src}
            alt={caption}
            style={{ maxWidth: 160, maxHeight: 120, objectFit: 'contain' }}
          />
        ) : (
          <Spin size="small" />
        )}
      </div>
      <Typography.Text type="secondary" style={{ fontSize: 11 }} ellipsis title={caption}>
        {caption}
      </Typography.Text>
    </div>
  );
}

export function ChunksModal({ workspaceId, doc, open, onClose }: Props) {
  const [loading, setLoading] = useState(true);
  const [data, setData] = useState<ChunksResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [images, setImages] = useState<DocumentImageInfo[]>([]);
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
    // Images are an optional enrichment — a failure here must not block chunks.
    listDocumentImages(workspaceId, doc.id)
      .then(setImages)
      .catch(() => setImages([]));
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
      {!loading && images.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <Typography.Text strong style={{ display: 'block', marginBottom: 8 }}>
            Extracted Images ({images.length})
          </Typography.Text>
          <Image.PreviewGroup>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 12 }}>
              {images.map((info) => (
                <AuthImage
                  key={info.image_id}
                  workspaceId={workspaceId}
                  docId={doc.id}
                  info={info}
                />
              ))}
            </div>
          </Image.PreviewGroup>
        </div>
      )}
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
