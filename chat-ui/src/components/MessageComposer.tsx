import { useRef, useState } from 'react';
import { Button, Input, Tag, Tooltip, message as antdMessage } from 'antd';
import { ArrowUpOutlined, PaperClipOutlined, FileTextOutlined } from '@ant-design/icons';
import type { Attachment } from '../api/types';

const MAX_FILES = 5;
const MAX_BYTES = 10 * 1024 * 1024; // 10 MB per file — friendly client-side guard

function readAsAttachment(file: File): Promise<Attachment> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onerror = () => reject(new Error('read failed'));
    r.onload = () => {
      const result = r.result as string;
      resolve({
        name: file.name,
        mime_type: file.type || 'application/octet-stream',
        data: result.split(',')[1] ?? '',
      });
    };
    r.readAsDataURL(file);
  });
}

export function MessageComposer({
  disabled,
  onSend,
}: {
  disabled: boolean;
  onSend: (text: string, attachments: Attachment[]) => void;
}) {
  const [value, setValue] = useState('');
  const [files, setFiles] = useState<Attachment[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  const submit = () => {
    const text = value.trim();
    if ((!text && files.length === 0) || disabled) return;
    onSend(text, files);
    setValue('');
    setFiles([]);
  };

  const onPick = async (list: FileList | null) => {
    if (!list) return;
    const picked = Array.from(list);
    if (files.length + picked.length > MAX_FILES) {
      antdMessage.warning(`Up to ${MAX_FILES} files per message.`);
      return;
    }
    const tooBig = picked.find((f) => f.size > MAX_BYTES);
    if (tooBig) {
      antdMessage.warning(`"${tooBig.name}" is over 10 MB.`);
      return;
    }
    try {
      const next = await Promise.all(picked.map(readAsAttachment));
      setFiles((prev) => [...prev, ...next]);
    } catch {
      antdMessage.error('Could not read that file.');
    }
    if (inputRef.current) inputRef.current.value = '';
  };

  return (
    <div style={{ padding: '14px 16px 18px' }}>
      {files.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginBottom: 8 }}>
          {files.map((f, i) => (
            <Tag
              key={`${f.name}-${i}`}
              icon={<FileTextOutlined />}
              closable
              onClose={() => setFiles((prev) => prev.filter((_, j) => j !== i))}
              style={{ margin: 0 }}
            >
              {f.name}
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
          border: '1px solid var(--line)',
          borderRadius: 16,
          padding: '8px 8px 8px 8px',
          boxShadow: '0 1px 2px rgba(20,34,59,0.04)',
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
          value={value}
          onChange={(e) => setValue(e.target.value)}
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
        <Button
          type="primary"
          shape="circle"
          aria-label="Send"
          icon={<ArrowUpOutlined />}
          onClick={submit}
          loading={disabled}
          disabled={!value.trim() && files.length === 0}
        />
      </div>
      <div style={{ textAlign: 'center', marginTop: 8, fontSize: 11.5, color: 'var(--text-muted)' }}>
        Enter to send · Shift + Enter for a new line
      </div>
    </div>
  );
}
