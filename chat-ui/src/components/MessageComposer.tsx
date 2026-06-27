import { useRef, useState } from 'react';
import { Button, Input, Tag, Tooltip, message as antdMessage } from 'antd';
import {
  ArrowUpOutlined,
  PaperClipOutlined,
  FileTextOutlined,
  BorderOutlined,
} from '@ant-design/icons';
import type { Attachment } from '../api/types';

const MAX_FILES = 5;
const MAX_BYTES = 10 * 1024 * 1024; // 10 MB per file — friendly client-side guard

/** A staged file: the wire payload plus its size, for the chip's size label. */
type PendingFile = { att: Attachment; size: number };

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function readAsAttachment(file: File): Promise<PendingFile> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onerror = () => reject(new Error('read failed'));
    r.onload = () => {
      const result = r.result as string;
      resolve({
        att: {
          name: file.name || 'pasted-image',
          mime_type: file.type || 'application/octet-stream',
          data: result.split(',')[1] ?? '',
        },
        size: file.size,
      });
    };
    r.readAsDataURL(file);
  });
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
      antdMessage.warning(`Up to ${MAX_FILES} files per message.`);
      return;
    }
    const tooBig = picked.find((f) => f.size > MAX_BYTES);
    if (tooBig) {
      antdMessage.warning(`"${tooBig.name}" is ${humanSize(tooBig.size)} — over the 10 MB limit.`);
      return;
    }
    try {
      const next = await Promise.all(picked.map(readAsAttachment));
      setFiles((prev) => [...prev, ...next]);
    } catch {
      antdMessage.error('Could not read that file.');
    }
  };

  const onPick = async (list: FileList | null) => {
    if (!list) return;
    await addFiles(Array.from(list));
    if (inputRef.current) inputRef.current.value = '';
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragging(false);
    if (disabled) return;
    const dropped = Array.from(e.dataTransfer.files ?? []);
    if (dropped.length > 0) void addFiles(dropped);
  };

  const onPaste = (e: React.ClipboardEvent) => {
    const pasted = Array.from(e.clipboardData.files ?? []);
    if (pasted.length > 0) {
      e.preventDefault();
      void addFiles(pasted);
    }
  };

  return (
    <div
      style={{ padding: '14px 16px 18px' }}
      data-testid="composer"
      onDragOver={(e) => {
        e.preventDefault();
        if (!disabled) setDragging(true);
      }}
      onDragLeave={(e) => {
        // Only clear when the cursor actually leaves the composer, not on child enters.
        if (e.currentTarget === e.target) setDragging(false);
      }}
      onDrop={onDrop}
    >
      {files.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginBottom: 8 }}>
          {files.map((f, i) => (
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
          ))}
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
        <Tooltip title="Attach files">
          <Button
            type="text"
            aria-label="Attach"
            icon={<PaperClipOutlined />}
            onClick={() => inputRef.current?.click()}
            disabled={disabled}
          />
        </Tooltip>
        <Input.TextArea
          data-testid="composer-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onPaste={onPaste}
          placeholder="Ask anything about your documents…"
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
          <Tooltip title="Stop">
            <Button
              type="primary"
              shape="circle"
              aria-label="Stop"
              icon={<BorderOutlined />}
              onClick={onStop}
            />
          </Tooltip>
        ) : (
          <Button
            type="primary"
            shape="circle"
            aria-label="Send"
            icon={<ArrowUpOutlined />}
            onClick={submit}
            loading={disabled}
            disabled={!value.trim() && files.length === 0}
          />
        )}
      </div>
      <div style={{ textAlign: 'center', marginTop: 8, fontSize: 11.5, color: 'var(--text-muted)' }}>
        {dragging ? 'Drop files to attach' : 'Enter to send · Shift + Enter for a new line · drop or paste files'}
      </div>
    </div>
  );
}
