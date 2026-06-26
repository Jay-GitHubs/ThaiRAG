import { useState } from 'react';
import { Button, Input, Space } from 'antd';
import { SendOutlined } from '@ant-design/icons';

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
    <div style={{ display: 'flex', gap: 8, padding: 16, borderTop: '1px solid #f0f0f0' }}>
      <Input.TextArea
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Ask anything about your documents…"
        autoSize={{ minRows: 1, maxRows: 6 }}
        onPressEnter={(e) => {
          if (!e.shiftKey) {
            e.preventDefault();
            submit();
          }
        }}
        disabled={disabled}
      />
      <Space direction="vertical" style={{ justifyContent: 'flex-end' }}>
        <Button
          type="primary"
          icon={<SendOutlined />}
          onClick={submit}
          loading={disabled}
          disabled={!value.trim()}
        >
          Send
        </Button>
      </Space>
    </div>
  );
}
