import { Button, Input, Popconfirm, Tooltip } from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  EditOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  SearchOutlined,
} from '@ant-design/icons';
import { useMemo, useState } from 'react';
import type { Conversation } from '../api/types';
import { useAuth } from '../auth/AuthContext';
import { BrandMark } from './BrandMark';
import { ThemePicker } from './ThemePicker';

// Bucket a conversation by how recently it was updated, for sidebar grouping.
const GROUP_ORDER = ['Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older'];
function bucketLabel(iso: string): string {
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return 'Older';
  const diff = Math.round((startOf(new Date()) - startOf(d)) / 86_400_000);
  if (diff <= 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  if (diff <= 7) return 'Previous 7 days';
  if (diff <= 30) return 'Previous 30 days';
  return 'Older';
}

export function ConversationSidebar({
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
  onRename,
  onCollapse,
}: {
  conversations: Conversation[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename: (id: string, title: string) => void;
  /** Desktop only: collapse the rail. Omitted on mobile (the Drawer closes instead). */
  onCollapse?: () => void;
}) {
  const { user, logout } = useAuth();
  const [hovered, setHovered] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [query, setQuery] = useState('');

  // Filter by title, then group by recency (preserving the backend's
  // updated_at-desc order within each group).
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = q
      ? conversations.filter((c) => (c.title || '').toLowerCase().includes(q))
      : conversations;
    const map = new Map<string, Conversation[]>();
    for (const c of filtered) {
      const k = bucketLabel(c.updated_at || c.created_at);
      if (!map.has(k)) map.set(k, []);
      map.get(k)!.push(c);
    }
    return GROUP_ORDER.filter((l) => map.has(l)).map((l) => ({ label: l, items: map.get(l)! }));
  }, [conversations, query]);

  const startEdit = (id: string, title: string) => {
    setEditingId(id);
    setEditValue(title);
  };
  const commitEdit = (id: string) => {
    const t = editValue.trim();
    if (t) onRename(id, t);
    setEditingId(null);
  };

  const renderRow = (c: Conversation) => {
    const active = c.id === activeId;
    return (
      <div
        key={c.id}
        data-testid="conversation-row"
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
          background: active
            ? 'var(--ink-soft)'
            : hovered === c.id
              ? 'var(--ink-hover)'
              : 'transparent',
          borderLeft: `2px solid ${active ? 'var(--celadon)' : 'transparent'}`,
          color: active ? 'var(--ink-bright)' : 'var(--ink-dim)',
          transition: 'background 0.12s',
        }}
      >
        {editingId === c.id ? (
          <Input
            size="small"
            autoFocus
            value={editValue}
            onChange={(e) => setEditValue(e.target.value)}
            onClick={(e) => e.stopPropagation()}
            onPressEnter={() => commitEdit(c.id)}
            onBlur={() => commitEdit(c.id)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') setEditingId(null);
            }}
            style={{ flex: 1 }}
          />
        ) : (
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
        )}
        {(hovered === c.id || active) && editingId !== c.id && (
          <span style={{ display: 'flex', gap: 10, flexShrink: 0 }}>
            <Tooltip title="Rename">
              <EditOutlined
                onClick={(e) => {
                  e.stopPropagation();
                  startEdit(c.id, c.title || '');
                }}
                style={{ color: 'var(--ink-icon)', fontSize: 13 }}
              />
            </Tooltip>
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
                style={{ color: 'var(--ink-icon)', fontSize: 13 }}
              />
            </Popconfirm>
          </span>
        )}
      </div>
    );
  };

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
      <div
        style={{
          padding: '18px 16px 12px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}
      >
        <BrandMark tone="light" size={24} />
        {onCollapse && (
          <Tooltip title="Collapse sidebar">
            <Button
              type="text"
              aria-label="Collapse sidebar"
              data-testid="sidebar-collapse"
              icon={<MenuFoldOutlined style={{ color: 'var(--ink-icon)' }} />}
              onClick={onCollapse}
            />
          </Tooltip>
        )}
      </div>

      <div style={{ padding: '4px 12px 12px' }}>
        <Tooltip title="New chat — press ? for all shortcuts">
          <Button type="primary" icon={<PlusOutlined />} block onClick={onNew}>
            New chat
          </Button>
        </Tooltip>
      </div>

      <div style={{ padding: '0 12px 10px' }}>
        <Input
          size="small"
          allowClear
          prefix={<SearchOutlined style={{ color: 'var(--ink-dim)' }} />}
          placeholder="Search conversations"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          data-testid="conversation-search"
        />
      </div>

      <div className="thin-scroll" style={{ flex: 1, overflowY: 'auto', padding: '4px 8px' }}>
        {conversations.length === 0 ? (
          <div style={{ color: 'var(--ink-dim)', fontSize: 13, padding: '8px 10px' }}>
            No conversations yet. Start one above.
          </div>
        ) : groups.length === 0 ? (
          <div style={{ color: 'var(--ink-dim)', fontSize: 13, padding: '8px 10px' }}>
            No conversations match your search.
          </div>
        ) : (
          groups.map((g) => (
            <div key={g.label}>
              <div
                className="eyebrow"
                style={{ padding: '10px 10px 4px', color: 'var(--ink-dim)' }}
              >
                {g.label}
              </div>
              {g.items.map(renderRow)}
            </div>
          ))
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
        <div style={{ display: 'flex', flexShrink: 0 }}>
          <ThemePicker />
          <Tooltip title="Sign out">
            <Button
              type="text"
              aria-label="Sign out"
              icon={<LogoutOutlined style={{ color: 'var(--ink-icon)' }} />}
              onClick={logout}
            />
          </Tooltip>
        </div>
      </div>
    </div>
  );
}
