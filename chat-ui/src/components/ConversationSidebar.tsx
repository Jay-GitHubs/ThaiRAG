import { Button, List, Typography, Popconfirm, theme } from 'antd';
import { PlusOutlined, DeleteOutlined, LogoutOutlined } from '@ant-design/icons';
import type { Conversation } from '../api/types';
import { useAuth } from '../auth/AuthContext';

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
  const { token } = theme.useToken();
  const { user, logout } = useAuth();

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ padding: 12 }}>
        <Button type="primary" icon={<PlusOutlined />} block onClick={onNew}>
          New chat
        </Button>
      </div>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        <List
          dataSource={conversations}
          locale={{ emptyText: 'No conversations yet' }}
          renderItem={(c) => (
            <List.Item
              onClick={() => onSelect(c.id)}
              style={{
                cursor: 'pointer',
                padding: '8px 12px',
                background: c.id === activeId ? token.colorFillSecondary : undefined,
              }}
              actions={[
                <Popconfirm
                  key="del"
                  title="Delete this conversation?"
                  onConfirm={(e) => {
                    e?.stopPropagation();
                    onDelete(c.id);
                  }}
                  onCancel={(e) => e?.stopPropagation()}
                >
                  <DeleteOutlined onClick={(e) => e.stopPropagation()} />
                </Popconfirm>,
              ]}
            >
              <Typography.Text ellipsis style={{ maxWidth: 180 }}>
                {c.title || 'Untitled'}
              </Typography.Text>
            </List.Item>
          )}
        />
      </div>
      <div
        style={{
          padding: 12,
          borderTop: `1px solid ${token.colorBorderSecondary}`,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}
      >
        <Typography.Text type="secondary" ellipsis style={{ maxWidth: 160 }}>
          {user?.name ?? user?.email}
        </Typography.Text>
        <Button type="text" icon={<LogoutOutlined />} onClick={logout} title="Sign out" />
      </div>
    </div>
  );
}
