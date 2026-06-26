import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Space, Tag, Tooltip, Image, theme } from 'antd';
import { FileTextOutlined } from '@ant-design/icons';
import type { Citation, ImageRef } from '../api/types';

export interface UiMessage {
  id?: string;
  role: 'user' | 'assistant';
  content: string;
  citations: Citation[];
  images: ImageRef[];
  streaming?: boolean;
}

export function MessageBubble({ message }: { message: UiMessage }) {
  const { token } = theme.useToken();
  const isUser = message.role === 'user';

  return (
    <div
      data-testid={isUser ? 'msg-user' : 'msg-assistant'}
      style={{
        display: 'flex',
        justifyContent: isUser ? 'flex-end' : 'flex-start',
        marginBottom: 16,
      }}
    >
      <div
        style={{
          maxWidth: 760,
          background: isUser ? token.colorPrimary : token.colorFillSecondary,
          color: isUser ? token.colorWhite : token.colorText,
          padding: '10px 14px',
          borderRadius: 12,
        }}
      >
        {isUser ? (
          <span style={{ whiteSpace: 'pre-wrap' }}>{message.content}</span>
        ) : (
          <div className="md-body">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {message.content || (message.streaming ? '…' : '')}
            </ReactMarkdown>
          </div>
        )}

        {message.images.length > 0 && (
          <div style={{ marginTop: 10 }}>
            <Image.PreviewGroup>
              <Space wrap>
                {message.images.map((img: ImageRef) => (
                  <Image
                    key={img.image_id}
                    src={img.url}
                    alt={img.page ? `source page ${img.page}` : 'source image'}
                    style={{ maxHeight: 180, borderRadius: 6 }}
                  />
                ))}
              </Space>
            </Image.PreviewGroup>
          </div>
        )}

        {message.citations.length > 0 && (
          <div style={{ marginTop: 10 }}>
            <Space wrap size={[4, 4]}>
              {message.citations.map((c: Citation, i: number) => {
                const label = (
                  <Tag
                    icon={<FileTextOutlined />}
                    style={{ cursor: c.url ? 'pointer' : 'default', margin: 0 }}
                  >
                    {c.title || c.doc_id}
                    {c.page ? ` · p.${c.page}` : ''}
                  </Tag>
                );
                const tip = c.section ? `${c.title} — ${c.section}` : c.title;
                const chip = <Tooltip title={tip}>{label}</Tooltip>;
                return c.url ? (
                  <a key={`${c.doc_id}-${i}`} href={c.url} target="_blank" rel="noreferrer">
                    {chip}
                  </a>
                ) : (
                  <span key={`${c.doc_id}-${i}`}>{chip}</span>
                );
              })}
            </Space>
          </div>
        )}
      </div>
    </div>
  );
}
