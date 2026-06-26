import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Image, Tag, Tooltip } from 'antd';
import { FileTextOutlined } from '@ant-design/icons';
import type { Citation, ImageRef } from '../api/types';

export interface UiMessage {
  id?: string;
  role: 'user' | 'assistant';
  content: string;
  citations: Citation[];
  images: ImageRef[];
  /** Names of files attached to a user turn (display only). */
  attachments?: string[];
  streaming?: boolean;
}

/** Small celadon document mark that stands in for the assistant. */
function AssistantMark() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true" style={{ flexShrink: 0 }}>
      <rect width="28" height="28" rx="8" fill="var(--celadon)" />
      <rect x="8" y="9" width="12" height="1.8" rx="0.9" fill="rgba(255,255,255,0.85)" />
      <rect x="8" y="13" width="12" height="1.8" rx="0.9" fill="rgba(255,255,255,0.55)" />
      <rect x="8" y="17" width="8" height="1.8" rx="0.9" fill="rgba(255,255,255,0.85)" />
    </svg>
  );
}

function UserMessage({ message }: { message: UiMessage }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'flex-end',
        marginBottom: 22,
      }}
    >
      <div
        data-testid="msg-user"
        style={{
          maxWidth: 620,
          background: 'var(--celadon)',
          color: '#fff',
          padding: '11px 15px',
          borderRadius: '14px 14px 4px 14px',
          fontSize: 15.5,
          lineHeight: 1.6,
          whiteSpace: 'pre-wrap',
        }}
      >
        {message.content}
      </div>
      {message.attachments && message.attachments.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 6, justifyContent: 'flex-end' }}>
          {message.attachments.map((name, i) => (
            <Tag key={`${name}-${i}`} icon={<FileTextOutlined />} style={{ margin: 0 }}>
              {name}
            </Tag>
          ))}
        </div>
      )}
    </div>
  );
}

function Sources({ citations, images }: { citations: Citation[]; images: ImageRef[] }) {
  if (citations.length === 0 && images.length === 0) return null;
  return (
    <div
      style={{
        marginTop: 16,
        paddingTop: 14,
        borderTop: '1px solid var(--line)',
      }}
    >
      <div className="eyebrow" style={{ marginBottom: 10 }}>
        Sources
      </div>

      {images.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, marginBottom: citations.length ? 12 : 0 }}>
          <Image.PreviewGroup>
            {images.map((img) => (
              <div
                key={img.image_id}
                style={{
                  border: '1px solid var(--line)',
                  borderRadius: 8,
                  padding: 4,
                  background: 'var(--surface)',
                }}
              >
                <Image
                  src={img.url}
                  alt={img.page ? `source page ${img.page}` : 'source image'}
                  style={{ maxHeight: 150, borderRadius: 4, display: 'block' }}
                />
                {img.page && (
                  <div
                    style={{
                      fontFamily: 'var(--font-mono)',
                      fontSize: 10.5,
                      color: 'var(--text-muted)',
                      textAlign: 'center',
                      paddingTop: 4,
                    }}
                  >
                    p.{img.page}
                  </div>
                )}
              </div>
            ))}
          </Image.PreviewGroup>
        </div>
      )}

      {citations.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          {citations.map((c, i) => {
            const inner = (
              <span
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 8,
                  background: 'var(--celadon-tint)',
                  border: '1px solid #cfe3dd',
                  borderRadius: 8,
                  padding: '5px 10px 5px 6px',
                  maxWidth: 320,
                }}
              >
                <span
                  style={{
                    fontFamily: 'var(--font-mono)',
                    fontSize: 11,
                    fontWeight: 500,
                    color: '#fff',
                    background: 'var(--celadon-deep)',
                    borderRadius: 5,
                    padding: '1px 6px',
                  }}
                >
                  {i + 1}
                </span>
                <span
                  style={{
                    fontSize: 13,
                    color: 'var(--celadon-deep)',
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                  }}
                >
                  {c.title || c.doc_id}
                  {c.page ? ` · p.${c.page}` : ''}
                </span>
              </span>
            );
            const tip = c.section ? `${c.title} — ${c.section}` : c.title || c.doc_id;
            const chip = <Tooltip title={tip}>{inner}</Tooltip>;
            return c.url ? (
              <a
                key={`${c.doc_id}-${i}`}
                href={c.url}
                target="_blank"
                rel="noreferrer"
                style={{ textDecoration: 'none' }}
              >
                {chip}
              </a>
            ) : (
              <span key={`${c.doc_id}-${i}`}>{chip}</span>
            );
          })}
        </div>
      )}
    </div>
  );
}

function AssistantMessage({ message }: { message: UiMessage }) {
  return (
    <div
      data-testid="msg-assistant"
      style={{ display: 'flex', gap: 14, marginBottom: 26, alignItems: 'flex-start' }}
    >
      <AssistantMark />
      <div style={{ minWidth: 0, flex: 1, paddingTop: 1 }}>
        <div className="md-body">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
          {message.streaming && <span className="caret" />}
        </div>
        <Sources citations={message.citations} images={message.images} />
      </div>
    </div>
  );
}

export function MessageBubble({ message }: { message: UiMessage }) {
  return message.role === 'user' ? (
    <UserMessage message={message} />
  ) : (
    <AssistantMessage message={message} />
  );
}
