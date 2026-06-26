import { useState } from 'react';
import { Button, Input } from 'antd';
import { ArrowUpOutlined } from '@ant-design/icons';

export function MessageComposer({
  disabled,
  onSend,
}: {
  disabled: boolean;
  onSend: (text: string) => void;
}) {
  const [value, setValue] = useState('');

  const submit = () => {
    const text = value.trim();
    if (!text || disabled) return;
    onSend(text);
    setValue('');
  };

  return (
    <div style={{ padding: '14px 16px 18px' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-end',
          gap: 8,
          background: 'var(--surface)',
          border: '1px solid var(--line)',
          borderRadius: 16,
          padding: '8px 8px 8px 14px',
          boxShadow: '0 1px 2px rgba(20,34,59,0.04)',
        }}
      >
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
          disabled={!value.trim()}
        />
      </div>
      <div
        style={{
          textAlign: 'center',
          marginTop: 8,
          fontSize: 11.5,
          color: 'var(--text-muted)',
        }}
      >
        Enter to send · Shift + Enter for a new line
      </div>
    </div>
  );
}
