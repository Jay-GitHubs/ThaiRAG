import { useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github.css';
import { Button as AntButton, Image, Input, Spin, Tag, Tooltip, message as antdMessage } from 'antd';
import {
  CheckOutlined,
  CopyOutlined,
  DislikeFilled,
  DislikeOutlined,
  EditOutlined,
  FileTextOutlined,
  LikeFilled,
  LikeOutlined,
} from '@ant-design/icons';
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
  /** Thumbs rating on an assistant turn: 1 up, -1 down, 0/undefined none. */
  feedback?: number;
  /** Friendly label for the current pipeline stage, shown while the answer is
   *  still being prepared (before any tokens arrive). */
  progress?: string;
  /** Token usage for the finished answer (surfaced under the message). */
  usage?: { prompt_tokens: number; completion_tokens: number };
  /** Wall-clock time from send to first/last token (ms), for the meta line. */
  elapsedMs?: number;
}

/** A fenced code block with a copy button (markdown renderer override). */
function CodeBlock({ children, ...props }: { children?: React.ReactNode }) {
  const preRef = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  const copy = () => {
    const text = preRef.current?.innerText ?? '';
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };
  return (
    <div style={{ position: 'relative' }}>
      <button
        type="button"
        data-testid="copy-code"
        onClick={copy}
        style={{
          position: 'absolute',
          top: 6,
          right: 6,
          border: 'none',
          background: 'rgba(0,0,0,0.06)',
          borderRadius: 6,
          padding: '2px 8px',
          fontSize: 11.5,
          cursor: 'pointer',
          color: 'var(--text-muted)',
        }}
      >
        {copied ? 'Copied' : 'Copy'}
      </button>
      <pre ref={preRef} {...props}>
        {children}
      </pre>
    </div>
  );
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

function UserMessage({
  message,
  editable,
  onEdit,
}: {
  message: UiMessage;
  editable?: boolean;
  onEdit?: (text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.content);
  const [hovered, setHovered] = useState(false);

  const begin = () => {
    setDraft(message.content);
    setEditing(true);
  };
  const save = () => {
    const t = draft.trim();
    setEditing(false);
    if (t && t !== message.content) onEdit?.(t);
  };

  if (editing) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', marginBottom: 22 }}>
        <div style={{ width: '100%', maxWidth: 620 }}>
          <Input.TextArea
            data-testid="edit-input"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            autoSize={{ minRows: 1, maxRows: 8 }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                save();
              }
              if (e.key === 'Escape') setEditing(false);
            }}
            style={{ fontSize: 15.5 }}
          />
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 8 }}>
            <AntButton size="small" onClick={() => setEditing(false)}>
              Cancel
            </AntButton>
            <AntButton
              size="small"
              type="primary"
              data-testid="edit-save"
              onClick={save}
              disabled={!draft.trim()}
            >
              Send
            </AntButton>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'flex-end',
        marginBottom: 22,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        {editable && (
          <Tooltip title="Edit & resend">
            <EditOutlined
              data-testid="edit-message"
              onClick={begin}
              style={{
                fontSize: 13,
                color: hovered ? 'var(--celadon)' : 'var(--text-muted)',
                cursor: 'pointer',
                opacity: hovered ? 1 : 0.6,
                transition: 'color 0.12s, opacity 0.12s',
              }}
            />
          </Tooltip>
        )}
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

function Sources({
  citations,
  images,
  onSourceClick,
}: {
  citations: Citation[];
  images: ImageRef[];
  onSourceClick?: (c: Citation) => void;
}) {
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
                data-testid="source-image"
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
                    background: 'var(--celadon)',
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
            // Open the in-app source viewer if available; otherwise fall back to
            // the new-tab citation link (e.g. when no doc_id to fetch).
            if (onSourceClick && c.doc_id) {
              return (
                <button
                  key={`${c.doc_id}-${i}`}
                  type="button"
                  data-testid="source-chip"
                  onClick={() => onSourceClick(c)}
                  style={{ border: 'none', background: 'none', padding: 0, cursor: 'pointer' }}
                >
                  {chip}
                </button>
              );
            }
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

/** Thumbs up/down on a finished assistant answer. Clicking the active rating
 *  again clears it. Hidden while streaming or before the turn has an id. */
function FeedbackBar({
  message,
  onFeedback,
}: {
  message: UiMessage;
  onFeedback: (messageId: string, value: number) => void;
}) {
  if (!message.id || message.streaming) return null;
  const rating = message.feedback ?? 0;
  const toggle = (value: number) => onFeedback(message.id!, rating === value ? 0 : value);
  const iconStyle = (active: boolean) => ({
    cursor: 'pointer',
    fontSize: 14,
    color: active ? 'var(--celadon-deep)' : 'var(--text-muted)',
  });
  return (
    <div style={{ display: 'flex', gap: 14 }}>
      <Tooltip title="Good answer">
        {rating === 1 ? (
          <LikeFilled
            data-testid="fb-up"
            aria-label="Remove positive feedback"
            onClick={() => toggle(1)}
            style={iconStyle(true)}
          />
        ) : (
          <LikeOutlined
            data-testid="fb-up"
            aria-label="Good answer"
            onClick={() => toggle(1)}
            style={iconStyle(false)}
          />
        )}
      </Tooltip>
      <Tooltip title="Bad answer">
        {rating === -1 ? (
          <DislikeFilled
            data-testid="fb-down"
            aria-label="Remove negative feedback"
            onClick={() => toggle(-1)}
            style={iconStyle(true)}
          />
        ) : (
          <DislikeOutlined
            data-testid="fb-down"
            aria-label="Bad answer"
            onClick={() => toggle(-1)}
            style={iconStyle(false)}
          />
        )}
      </Tooltip>
    </div>
  );
}

/** Action row under a finished answer: copy, feedback thumbs, and a usage meta. */
function AnswerActions({
  message,
  onFeedback,
}: {
  message: UiMessage;
  onFeedback?: (messageId: string, value: number) => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(message.content).then(() => {
      setCopied(true);
      antdMessage.success('Answer copied');
      setTimeout(() => setCopied(false), 1500);
    });
  };
  const meta = [
    message.elapsedMs != null ? `${(message.elapsedMs / 1000).toFixed(1)}s` : null,
    message.usage
      ? `${(message.usage.prompt_tokens + message.usage.completion_tokens).toLocaleString()} tokens`
      : null,
  ]
    .filter(Boolean)
    .join(' · ');
  const icon = { cursor: 'pointer', fontSize: 14, color: 'var(--text-muted)' };
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginTop: 12 }}>
      <Tooltip title={copied ? 'Copied' : 'Copy answer'}>
        {copied ? (
          <CheckOutlined data-testid="copy-answer" style={{ ...icon, color: 'var(--celadon-deep)' }} />
        ) : (
          <CopyOutlined data-testid="copy-answer" onClick={copy} style={icon} />
        )}
      </Tooltip>
      {onFeedback && <FeedbackBar message={message} onFeedback={onFeedback} />}
      {meta && <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>{meta}</span>}
    </div>
  );
}

function AssistantMessage({
  message,
  onFeedback,
  onSourceClick,
}: {
  message: UiMessage;
  onFeedback?: (messageId: string, value: number) => void;
  onSourceClick?: (c: Citation) => void;
}) {
  return (
    <div
      data-testid="msg-assistant"
      style={{ display: 'flex', gap: 14, marginBottom: 26, alignItems: 'flex-start' }}
    >
      <AssistantMark />
      <div style={{ minWidth: 0, flex: 1, paddingTop: 1 }}>
        <div className="md-body">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeHighlight]}
            components={{ pre: CodeBlock }}
          >
            {message.content}
          </ReactMarkdown>
          {message.streaming &&
            (message.content.length === 0 ? (
              <div
                data-testid="msg-progress"
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 9,
                  color: 'var(--text-muted)',
                  fontSize: 14,
                }}
              >
                <Spin size="small" />
                <span>{message.progress ?? 'Working…'}</span>
              </div>
            ) : (
              <span className="caret" />
            ))}
        </div>
        <Sources
          citations={message.citations}
          images={message.images}
          onSourceClick={onSourceClick}
        />
        {!message.streaming && message.content.length > 0 && (
          <AnswerActions message={message} onFeedback={onFeedback} />
        )}
      </div>
    </div>
  );
}

export function MessageBubble({
  message,
  onFeedback,
  onSourceClick,
  editable,
  onEdit,
}: {
  message: UiMessage;
  onFeedback?: (messageId: string, value: number) => void;
  onSourceClick?: (c: Citation) => void;
  editable?: boolean;
  onEdit?: (text: string) => void;
}) {
  return message.role === 'user' ? (
    <UserMessage message={message} editable={editable} onEdit={onEdit} />
  ) : (
    <AssistantMessage message={message} onFeedback={onFeedback} onSourceClick={onSourceClick} />
  );
}
