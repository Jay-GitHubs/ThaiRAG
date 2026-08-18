import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../i18n/LocaleProvider';
import { Button, Input, Popover, Tag, Tooltip, message as antdMessage } from 'antd';
import {
  ArrowUpOutlined,
  BookOutlined,
  PaperClipOutlined,
  FileTextOutlined,
  BorderOutlined,
} from '@ant-design/icons';
import { listPromptTemplates } from '../api/prompts';
import type { PromptTemplate } from '../api/prompts';
import type { Attachment } from '../api/types';

const MAX_FILES = 5;
const MAX_BYTES = 10 * 1024 * 1024; // 10 MB per file — friendly client-side guard
const THUMB_MAX_DIM = 320; // thumbnail bound (px) — small enough to persist
const THUMB_INLINE_BYTES = 100 * 1024; // images this small skip re-encoding

/** A staged file: the wire payload plus its size, for the chip's size label. */
type PendingFile = { att: Attachment; size: number };

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Downscale an image data-URL to a small thumbnail (JPEG, bounded at
 *  THUMB_MAX_DIM). Resolves undefined when the image can't be decoded —
 *  the attachment then falls back to a plain file chip. */
function makeThumbnail(dataUrl: string): Promise<string | undefined> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onerror = () => resolve(undefined);
    img.onload = () => {
      try {
        const scale = Math.min(1, THUMB_MAX_DIM / Math.max(img.width, img.height, 1));
        const canvas = document.createElement('canvas');
        canvas.width = Math.max(1, Math.round(img.width * scale));
        canvas.height = Math.max(1, Math.round(img.height * scale));
        const ctx = canvas.getContext('2d');
        if (!ctx) return resolve(undefined);
        // JPEG has no alpha — flatten transparency onto white first.
        ctx.fillStyle = '#fff';
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
        resolve(canvas.toDataURL('image/jpeg', 0.85));
      } catch {
        resolve(undefined);
      }
    };
    img.src = dataUrl;
  });
}

async function readAsAttachment(file: File): Promise<PendingFile> {
  const dataUrl = await new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onerror = () => reject(new Error('read failed'));
    r.onload = () => resolve(r.result as string);
    r.readAsDataURL(file);
  });
  const mime = file.type || 'application/octet-stream';
  let preview: string | undefined;
  if (mime.startsWith('image/')) {
    // Small images ride along as-is; larger ones get a bounded thumbnail.
    preview = file.size <= THUMB_INLINE_BYTES ? dataUrl : await makeThumbnail(dataUrl);
  }
  return {
    att: {
      name: file.name || 'pasted-image',
      mime_type: mime,
      data: dataUrl.split(',')[1] ?? '',
      ...(preview ? { preview } : {}),
    },
    size: file.size,
  };
}

export function MessageComposer({
  disabled,
  onSend,
  onStop,
}: {
  disabled: boolean;
  onSend: (text: string, attachments: Attachment[]) => void;
  onStop?: () => void;
}) {
  const { t } = useI18n();
  const [templates, setTemplates] = useState<PromptTemplate[]>([]);
  const [templatesOpen, setTemplatesOpen] = useState(false);
  // Load once; an empty list (none published, or no /api/km access) simply
  // keeps the picker hidden.
  useEffect(() => {
    listPromptTemplates().then(setTemplates);
  }, []);
  const [value, setValue] = useState('');
  const [files, setFiles] = useState<PendingFile[]>([]);
  const [dragging, setDragging] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const submit = () => {
    const text = value.trim();
    if ((!text && files.length === 0) || disabled) return;
    onSend(text, files.map((f) => f.att));
    setValue('');
    setFiles([]);
  };

  // Shared entry point for picker, drag-drop, and paste: validate count/size,
  // then stage the readable files.
  const addFiles = async (picked: File[]) => {
    if (picked.length === 0) return;
    if (files.length + picked.length > MAX_FILES) {
      antdMessage.warning(t('tooManyFiles', { max: MAX_FILES }));
      return;
    }
    const tooBig = picked.find((f) => f.size > MAX_BYTES);
    if (tooBig) {
      antdMessage.warning(
        t('fileTooLarge', { name: tooBig.name, size: humanSize(tooBig.size), max: '10 MB' }),
      );
      return;
    }
    try {
      const next = await Promise.all(picked.map(readAsAttachment));
      setFiles((prev) => [...prev, ...next]);
    } catch {
      antdMessage.error(t('fileReadError'));
    }
  };

  const onPick = async (list: FileList | null) => {
    if (!list) return;
    await addFiles(Array.from(list));
    if (inputRef.current) inputRef.current.value = '';
  };

  const onPaste = (e: React.ClipboardEvent) => {
    const pasted = Array.from(e.clipboardData.files ?? []);
    if (pasted.length > 0) {
      e.preventDefault();
      void addFiles(pasted);
    }
  };

  // Whole-window drop target (like claude.ai): dragging a file anywhere over
  // the app shows a drop overlay and attaches on release. Without this, a drop
  // outside the composer makes the browser navigate to the file. The handlers
  // read through refs so the window listeners register once.
  const addFilesRef = useRef(addFiles);
  addFilesRef.current = addFiles;
  useEffect(() => {
    // Nested dragenter/dragleave pairs fire for every child element — only
    // depth 0 means the pointer actually left the window.
    let depth = 0;
    const hasFiles = (e: DragEvent) => (e.dataTransfer?.types ?? []).includes('Files');
    const onEnter = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      depth += 1;
      setDragging(true);
    };
    const onOver = (e: DragEvent) => {
      // preventDefault is what permits a drop (and stops browser navigation).
      if (hasFiles(e)) e.preventDefault();
    };
    const onLeave = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      depth = Math.max(0, depth - 1);
      if (depth === 0) setDragging(false);
    };
    const onDrop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      setDragging(false);
      const dropped = Array.from(e.dataTransfer?.files ?? []);
      if (dropped.length > 0) void addFilesRef.current(dropped);
    };
    window.addEventListener('dragenter', onEnter);
    window.addEventListener('dragover', onOver);
    window.addEventListener('dragleave', onLeave);
    window.addEventListener('drop', onDrop);
    return () => {
      window.removeEventListener('dragenter', onEnter);
      window.removeEventListener('dragover', onOver);
      window.removeEventListener('dragleave', onLeave);
      window.removeEventListener('drop', onDrop);
    };
  }, []);

  return (
    <div style={{ padding: '14px 16px 18px' }} data-testid="composer">
      {dragging && (
        <div
          data-testid="drop-overlay"
          style={{
            position: 'fixed',
            inset: 0,
            zIndex: 1000,
            background: 'var(--overlay, rgba(0,0,0,0.35))',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            pointerEvents: 'none',
          }}
        >
          <div
            style={{
              background: 'var(--surface)',
              border: '2px dashed var(--celadon)',
              borderRadius: 16,
              padding: '28px 40px',
              fontSize: 16,
              fontWeight: 500,
              color: 'var(--text)',
              boxShadow: '0 8px 32px var(--shadow-sm)',
            }}
          >
            {t('dropFilesToAttach')}
          </div>
        </div>
      )}
      {files.length > 0 && (
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            gap: 8,
            marginBottom: 8,
            alignItems: 'flex-end',
          }}
        >
          {files.map((f, i) =>
            f.att.preview ? (
              <div
                key={`${f.att.name}-${i}`}
                data-testid="staged-image"
                style={{ position: 'relative', lineHeight: 0 }}
              >
                <img
                  src={f.att.preview}
                  alt={f.att.name}
                  title={`${f.att.name} · ${humanSize(f.size)}`}
                  style={{
                    height: 64,
                    maxWidth: 120,
                    objectFit: 'cover',
                    borderRadius: 10,
                    border: '1px solid var(--line)',
                    display: 'block',
                  }}
                />
                <button
                  type="button"
                  aria-label={t('removeAttachment')}
                  onClick={() => setFiles((prev) => prev.filter((_, j) => j !== i))}
                  style={{
                    position: 'absolute',
                    top: -6,
                    right: -6,
                    width: 18,
                    height: 18,
                    borderRadius: '50%',
                    border: '1px solid var(--line)',
                    background: 'var(--surface)',
                    color: 'var(--text-muted)',
                    fontSize: 11,
                    lineHeight: '15px',
                    padding: 0,
                    cursor: 'pointer',
                  }}
                >
                  ✕
                </button>
              </div>
            ) : (
              <Tag
                key={`${f.att.name}-${i}`}
                icon={<FileTextOutlined />}
                closable
                onClose={() => setFiles((prev) => prev.filter((_, j) => j !== i))}
                style={{ margin: 0 }}
              >
                {f.att.name}{' '}
                <span style={{ color: 'var(--text-muted)' }}>· {humanSize(f.size)}</span>
              </Tag>
            ),
          )}
        </div>
      )}
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-end',
          gap: 8,
          background: 'var(--surface)',
          border: `1px solid ${dragging ? 'var(--celadon)' : 'var(--line)'}`,
          borderRadius: 16,
          padding: '8px 8px 8px 8px',
          boxShadow: dragging
            ? '0 0 0 3px var(--celadon-tint)'
            : '0 1px 2px var(--shadow-sm)',
          transition: 'border-color 0.12s, box-shadow 0.12s',
        }}
      >
        <input
          ref={inputRef}
          type="file"
          multiple
          style={{ display: 'none' }}
          onChange={(e) => onPick(e.target.files)}
        />
        <Tooltip title={t('attachFiles')}>
          <Button
            type="text"
            aria-label={t('attach')}
            icon={<PaperClipOutlined />}
            onClick={() => inputRef.current?.click()}
            disabled={disabled}
          />
        </Tooltip>
        {templates.length > 0 && (
          <Popover
            trigger="click"
            open={templatesOpen}
            onOpenChange={setTemplatesOpen}
            placement="topLeft"
            title={t('promptTemplates')}
            content={
              <div style={{ maxHeight: 280, overflowY: 'auto', maxWidth: 320 }}>
                {templates.map((tpl) => (
                  <div
                    key={tpl.id}
                    data-testid="prompt-template-option"
                    role="button"
                    tabIndex={0}
                    onClick={() => {
                      setValue(tpl.content);
                      setTemplatesOpen(false);
                    }}
                    style={{ padding: '7px 8px', borderRadius: 8, cursor: 'pointer' }}
                    onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--surface)')}
                    onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
                  >
                    <div style={{ fontSize: 13.5, fontWeight: 500 }}>{tpl.name}</div>
                    {tpl.description && (
                      <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                        {tpl.description}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            }
          >
            <Tooltip title={t('promptTemplates')}>
              <Button
                type="text"
                aria-label={t('promptTemplates')}
                data-testid="prompt-templates"
                icon={<BookOutlined />}
                disabled={disabled}
              />
            </Tooltip>
          </Popover>
        )}
        <Input.TextArea
          data-testid="composer-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onPaste={onPaste}
          placeholder={t('composerPlaceholder')}
          variant="borderless"
          autoSize={{ minRows: 1, maxRows: 7 }}
          onPressEnter={(e) => {
            if (!e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          disabled={disabled}
          style={{ padding: '5px 0', fontSize: 15.5, resize: 'none' }}
        />
        {disabled && onStop ? (
          <Tooltip title={t('stop')}>
            <Button
              type="primary"
              shape="circle"
              aria-label={t('stop')}
              icon={<BorderOutlined />}
              onClick={onStop}
            />
          </Tooltip>
        ) : (
          <Button
            type="primary"
            shape="circle"
            aria-label={t('send')}
            icon={<ArrowUpOutlined />}
            onClick={submit}
            loading={disabled}
            disabled={!value.trim() && files.length === 0}
          />
        )}
      </div>
      <div style={{ textAlign: 'center', marginTop: 8, fontSize: 11.5, color: 'var(--text-muted)' }}>
        {dragging ? t('dropFilesToAttach') : t('composerHint')}
      </div>
    </div>
  );
}
