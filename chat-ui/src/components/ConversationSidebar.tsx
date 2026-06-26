import { Button, Popconfirm, Tooltip } from 'antd';
import { PlusOutlined, DeleteOutlined, LogoutOutlined } from '@ant-design/icons';
import { useState } from 'react';
import type { Conversation } from '../api/types';
import { useAuth } from '../auth/AuthContext';
import { BrandMark } from './BrandMark';

export function ConversationSidebar({
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
}: {
  conversations: Conversation[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
}) {
  const { user, logout } = useAuth();
  const [hovered, setHovered] = useState<string | null>(null);

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--ink)',
        color: 'var(--ink-bright)',
      }}
    >
      <div style={{ padding: '18px 16px 12px' }}>
        <BrandMark tone="light" size={24} />
      </div>

      <div style={{ padding: '4px 12px 12px' }}>
        <Button type="primary" icon={<PlusOutlined />} block onClick={onNew}>
          New chat
        </Button>
      </div>

      <div className="eyebrow" style={{ padding: '4px 18px', color: 'rgba(255,255,255,0.4)' }}>
        Conversations
      </div>

      <div className="thin-scroll" style={{ flex: 1, overflowY: 'auto', padding: '4px 8px' }}>
        {conversations.length === 0 ? (
          <div style={{ color: 'var(--ink-dim)', fontSize: 13, padding: '8px 10px' }}>
            No conversations yet. Start one above.
          </div>
        ) : (
          conversations.map((c) => {
            const active = c.id === activeId;
            return (
              <div
                key={c.id}
                onClick={() => onSelect(c.id)}
                onMouseEnter={() => setHovered(c.id)}
                onMouseLeave={() => setHovered(null)}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 8,
                  cursor: 'pointer',
                  padding: '9px 10px',
                  borderRadius: 8,
                  marginBottom: 2,
                  background: active ? 'var(--ink-soft)' : hovered === c.id ? 'rgba(255,255,255,0.05)' : 'transparent',
                  borderLeft: `2px solid ${active ? 'var(--celadon)' : 'transparent'}`,
                  color: active ? 'var(--ink-bright)' : 'var(--ink-dim)',
                  transition: 'background 0.12s',
                }}
              >
                <span
                  style={{
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    fontSize: 14,
                  }}
                >
                  {c.title || 'Untitled'}
                </span>
                {(hovered === c.id || active) && (
                  <Popconfirm
                    title="Delete this conversation?"
                    okText="Delete"
                    okButtonProps={{ danger: true }}
                    onConfirm={(e) => {
                      e?.stopPropagation();
                      onDelete(c.id);
                    }}
                    onCancel={(e) => e?.stopPropagation()}
                  >
                    <DeleteOutlined
                      onClick={(e) => e.stopPropagation()}
                      style={{ color: 'rgba(255,255,255,0.45)', fontSize: 13 }}
                    />
                  </Popconfirm>
                )}
              </div>
            );
          })
        )}
      </div>

      <div
        style={{
          padding: '12px 14px',
          borderTop: '1px solid var(--ink-line)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div
            style={{
              fontSize: 13,
              fontWeight: 500,
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {user?.name ?? user?.email ?? 'Signed in'}
          </div>
          {user?.email && (
            <div style={{ fontSize: 11, color: 'var(--ink-dim)' }}>{user.email}</div>
          )}
        </div>
        <Tooltip title="Sign out">
          <Button
            type="text"
            icon={<LogoutOutlined style={{ color: 'rgba(255,255,255,0.6)' }} />}
            onClick={logout}
          />
        </Tooltip>
      </div>
    </div>
  );
}
