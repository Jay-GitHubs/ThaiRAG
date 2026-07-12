import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import rehypeHighlight from 'rehype-highlight';
import rehypeKatex from 'rehype-katex';
import 'katex/dist/katex.min.css';
import { useI18n } from '../i18n/LocaleProvider';
// The code surface (--code-bg) is a dark slate in every theme, so the dark
// syntax palette reads well across all of them.
import 'highlight.js/styles/github-dark.css';
import { Button as AntButton, Image, Input, Popover, Spin, Tag, Tooltip, message as antdMessage } from 'antd';
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
import type { Citation, ConfidenceFactor, ImageRef } from '../api/types';

/** LLMs commonly emit TeX with \\(..\\) / \\[..\\] delimiters, which remark-math
 *  doesn't parse — normalize them to $ / $$ before rendering. Fenced and inline
 *  code segments are passed through untouched so regex-looking code survives. */
/** Elapsed-seconds counter shown next to the pipeline stage while waiting
 *  for the first token. On slow gateways time-to-first-token can exceed 30s;
 *  a visibly advancing count tells the user the request is alive rather than
 *  hung. Mounts with the pre-token placeholder, so 0s = send time. */
function ElapsedTimer() {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const started = Date.now();
    const id = setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 1000);
    return () => clearInterval(id);
  }, []);
  if (seconds < 3) return null; // don't flash a counter on fast answers
  return (
    <span style={{ fontVariantNumeric: 'tabular-nums', opacity: 0.75 }} data-testid="elapsed-timer">
      {seconds}s
    </span>
  );
}

function normalizeMathDelimiters(md: string): string {
  return md
    .split(/(```[\s\S]*?```|`[^`]*`)/g)
    .map((seg, i) =>
      i % 2 === 1
        ? seg
        : seg
            .replace(/\\\[([\s\S]+?)\\\]/g, (_, m) => `$$${m}$$`)
            .replace(/\\\(([\s\S]+?)\\\)/g, (_, m) => `$${m}$`),
    )
    .join('');
}

export interface UiMessage {
  id?: string;
  role: 'user' | 'assistant';
  content: string;
  citations: Citation[];
  images: ImageRef[];
  /** Names of files attached to a user turn (display only). */
  attachments?: string[];
  streaming?: boolean;
  /** The stream failed before completing — the turn was not saved. The page
   *  offers a Retry (re-send) instead of Regenerate for this state. */
  error?: boolean;
  /** Thumbs rating on an assistant turn: 1 up, -1 down, 0/undefined none. */
  feedback?: number;
  /** Friendly label for the current pipeline stage, shown while the answer is
   *  still being prepared (before any tokens arrive). */
  progress?: string;
  /** Token usage for the finished answer (surfaced under the message). */
  usage?: { prompt_tokens: number; completion_tokens: number };
  /** Deterministic answer-grounding confidence, 1–10. */
  confidence?: number;
  /** One-line rationale for `confidence` (shown in the tooltip). */
  confidenceSummary?: string;
  /** Per-factor breakdown behind `confidence` (shown in the tooltip). */
  confidenceFactors?: ConfidenceFactor[];
  /** Wall-clock time from send to first/last token (ms), for the meta line. */
  elapsedMs?: number;
}

/** A fenced code block with a copy button (markdown renderer override). */
function CodeBlock({ children, ...props }: { children?: React.ReactNode }) {
  const { t } = useI18n();
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
          background: 'var(--code-btn-bg)',
          borderRadius: 6,
          padding: '2px 8px',
          fontSize: 11.5,
          cursor: 'pointer',
          color: 'var(--code-text)',
        }}
      >
        {copied ? t('copied') : t('copy')}
      </button>
      <pre ref={preRef} {...props}>
        {children}
      </pre>
    </div>
  );
}

/** Friendly robot mark that stands in for the assistant. The head paints
 *  --on-accent (always legible on the accent square); the eyes/mouth are
 *  cut-outs in the accent color, so the avatar reads correctly in every theme. */
function AssistantMark() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true" style={{ flexShrink: 0 }}>
      <rect width="28" height="28" rx="8" fill="var(--celadon)" />
      {/* antenna */}
      <rect x="13.2" y="4" width="1.6" height="3.2" rx="0.8" fill="var(--on-accent)" />
      <circle cx="14" cy="3.6" r="1.5" fill="var(--on-accent)" />
      {/* head */}
      <rect x="6.5" y="7.5" width="15" height="13" rx="4" fill="var(--on-accent)" />
      {/* eyes + mouth (cut-outs showing the accent square through) */}
      <circle cx="11" cy="13" r="1.7" fill="var(--celadon)" />
      <circle cx="17" cy="13" r="1.7" fill="var(--celadon)" />
      <rect x="10.5" y="16.4" width="7" height="1.7" rx="0.85" fill="var(--celadon)" />
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
  const { t } = useI18n();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.content);
  const [hovered, setHovered] = useState(false);
  const [copied, setCopied] = useState(false);

  const copy = () => {
    navigator.clipboard.writeText(message.content).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

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
        <Tooltip title={copied ? 'Copied' : 'Copy prompt'}>
          {copied ? (
            <CheckOutlined
              data-testid="copy-prompt"
              style={{ fontSize: 13, color: 'var(--celadon)', cursor: 'pointer' }}
            />
          ) : (
            <CopyOutlined
              data-testid="copy-prompt"
              onClick={copy}
              style={{
                fontSize: 13,
                color: hovered ? 'var(--celadon)' : 'var(--text-muted)',
                cursor: 'pointer',
                opacity: hovered ? 1 : 0.6,
                transition: 'color 0.12s, opacity 0.12s',
              }}
            />
          )}
        </Tooltip>
        {editable && (
          <Tooltip title={t('editAndResend')}>
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
            color: 'var(--on-accent)',
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
  const { t } = useI18n();
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
        {t('sources')}
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
                  alt={img.page ? t('sourcePage', { page: img.page }) : t('source')}
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
                  border: '1px solid var(--chip-border)',
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
                    color: 'var(--on-accent)',
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
  const { t } = useI18n();
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
      <Tooltip title={t('goodAnswer')}>
        {rating === 1 ? (
          <LikeFilled
            data-testid="fb-up"
            aria-label={t('removePositiveFeedback')}
            onClick={() => toggle(1)}
            style={iconStyle(true)}
          />
        ) : (
          <LikeOutlined
            data-testid="fb-up"
            aria-label={t('goodAnswer')}
            onClick={() => toggle(1)}
            style={iconStyle(false)}
          />
        )}
      </Tooltip>
      <Tooltip title={t('badAnswer')}>
        {rating === -1 ? (
          <DislikeFilled
            data-testid="fb-down"
            aria-label={t('removeNegativeFeedback')}
            onClick={() => toggle(-1)}
            style={iconStyle(true)}
          />
        ) : (
          <DislikeOutlined
            data-testid="fb-down"
            aria-label={t('badAnswer')}
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
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(message.content).then(() => {
      setCopied(true);
      antdMessage.success(t('answerCopied'));
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
  const conf = message.confidence;
  // Status hues are theme-agnostic mid-tones (read on light + dark themes).
  const confColor =
    conf == null ? 'var(--text-muted)' : conf >= 7 ? '#369e62' : conf >= 4 ? '#d9962a' : '#d6453d';
  // Follow the answer's language for the confidence chrome — the backend
  // localizes the summary/factors the same way (script detection).
  const isThai = /[\u0E00-\u0E7F]/.test(message.confidenceSummary ?? message.content);
  const confL = isThai
    ? {
        title: 'ความเชื่อมั่นของคำตอบ',
        chip: 'ความเชื่อมั่น',
        noAnswer: 'ไม่มีคำตอบ',
        fallback: 'คำตอบนี้อ้างอิงจากแหล่งที่มาที่ค้นคืนได้มากน้อยเพียงใด',
      }
    : {
        title: 'Answer confidence',
        chip: 'Confidence',
        noAnswer: 'No answer',
        fallback: 'How well this answer is grounded in the retrieved sources',
      };
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginTop: 12 }}>
      <Tooltip title={copied ? t('copied') : t('copyAnswer')}>
        {copied ? (
          <CheckOutlined data-testid="copy-answer" style={{ ...icon, color: 'var(--celadon-deep)' }} />
        ) : (
          <CopyOutlined data-testid="copy-answer" onClick={copy} style={icon} />
        )}
      </Tooltip>
      {onFeedback && <FeedbackBar message={message} onFeedback={onFeedback} />}
      {conf != null && (
        // Click (not hover) to open the breakdown: a hover tooltip here popped
        // open whenever the cursor passed over on its way to a citation,
        // interrupting the click. Click-to-open is opt-in and stays out of the way.
        <Popover
          trigger="click"
          placement="top"
          title={confL.title}
          content={
            <div style={{ fontSize: 12, lineHeight: 1.5, maxWidth: 260 }}>
              {message.confidenceSummary && (
                <div style={{ marginBottom: message.confidenceFactors?.length ? 6 : 0 }}>
                  {message.confidenceSummary}
                </div>
              )}
              {message.confidenceFactors?.map((f) => (
                <div key={f.label} style={{ display: 'flex', gap: 6 }}>
                  <span style={{ opacity: 0.7 }}>{f.label}:</span>
                  <span>{f.detail}</span>
                </div>
              ))}
              {!message.confidenceSummary && !message.confidenceFactors?.length && confL.fallback}
            </div>
          }
        >
          <span
            data-testid="confidence"
            role="button"
            tabIndex={0}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 12, color: 'var(--text-muted)', cursor: 'pointer' }}
          >
            <span style={{ width: 7, height: 7, borderRadius: '50%', background: confColor }} />
            {confL.chip} {conf}/10
          </span>
        </Popover>
      )}
      {/* No-answer state: retrieval found nothing relevant, so the turn is a
          refusal rather than a scored answer — show a neutral marker, not a
          (misleading) 1–10 confidence number. */}
      {conf == null && message.confidenceSummary && (
        <Tooltip title={message.confidenceSummary}>
          <span
            data-testid="no-answer"
            style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 12, color: 'var(--text-muted)', cursor: 'help' }}
          >
            <span style={{ width: 7, height: 7, borderRadius: '50%', background: 'var(--text-muted)' }} />
            {confL.noAnswer}
          </span>
        </Tooltip>
      )}
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
  // A generated image turn carries image(s) but no answer text and no citations —
  // render it as the answer itself (large, inline), not under a "Sources" heading.
  const isGeneratedImage =
    !message.streaming &&
    message.citations.length === 0 &&
    message.images.length > 0 &&
    message.content.trim().length === 0;

  return (
    <div
      data-testid="msg-assistant"
      style={{ display: 'flex', gap: 14, marginBottom: 26, alignItems: 'flex-start' }}
    >
      <AssistantMark />
      <div style={{ minWidth: 0, flex: 1, paddingTop: 1 }}>
        {isGeneratedImage && (
          <div data-testid="generated-image" style={{ marginBottom: 6 }}>
            <Image.PreviewGroup>
              {message.images.map((img) => (
                <Image
                  key={img.image_id}
                  src={img.url}
                  alt="generated image"
                  style={{
                    maxWidth: '100%',
                    maxHeight: 420,
                    borderRadius: 10,
                    border: '1px solid var(--line)',
                    display: 'block',
                  }}
                />
              ))}
            </Image.PreviewGroup>
          </div>
        )}
        <div className="md-body">
          <ReactMarkdown
            remarkPlugins={[remarkGfm, remarkMath]}
            rehypePlugins={[rehypeHighlight, [rehypeKatex, { strict: false }]]}
            components={{ pre: CodeBlock }}
          >
            {normalizeMathDelimiters(message.content)}
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
                <ElapsedTimer />
              </div>
            ) : (
              <span className="caret" />
            ))}
        </div>
        {!isGeneratedImage && (
          <Sources
            citations={message.citations}
            images={message.images}
            onSourceClick={onSourceClick}
          />
        )}
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
