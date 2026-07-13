import { Button, Input, Popconfirm, Tooltip , Spin } from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  DownloadOutlined,
  EditOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  PushpinOutlined,
  PushpinFilled,
  SearchOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import { message as antdMessage } from 'antd';
import { useMemo, useState } from 'react';
import type { Conversation } from '../api/types';
import { useAuth } from '../auth/AuthContext';
import { BrandMark } from './BrandMark';
import { ThemePicker } from './ThemePicker';
import { useI18n } from '../i18n/LocaleProvider';
import type { MessageKey } from '../i18n/LocaleProvider';
import { LocaleSwitcher } from '../i18n/LocaleSwitcher';
import { SettingsModal } from './SettingsModal';
import { exportConversationMarkdown } from '../utils/exportConversation';

// Bucket a conversation by how recently it was updated, for sidebar grouping.
// Buckets are catalog keys so the group headers follow the UI locale.
const GROUP_ORDER: MessageKey[] = [
  'groupPinned',
  'groupToday',
  'groupYesterday',
  'groupPrev7',
  'groupPrev30',
  'groupOlder',
];
function bucketKey(iso: string): MessageKey {
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return 'groupOlder';
  const diff = Math.round((startOf(new Date()) - startOf(d)) / 86_400_000);
  if (diff <= 0) return 'groupToday';
  if (diff === 1) return 'groupYesterday';
  if (diff <= 7) return 'groupPrev7';
  if (diff <= 30) return 'groupPrev30';
  return 'groupOlder';
}

export function ConversationSidebar({
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
  onRename,
  onTogglePin,
  onCollapse,
  busyIds,
}: {
  conversations: Conversation[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onTogglePin: (id: string, pinned: boolean) => void;
  /** Desktop only: collapse the rail. Omitted on mobile (the Drawer closes instead). */
  onCollapse?: () => void;
  /** Conversations with an answer still generating (possibly in the
   *  background) — rendered with a busy dot so users can find them. */
  busyIds?: Set<string>;
}) {
  const { user, logout } = useAuth();
  const { t } = useI18n();
  const [hovered, setHovered] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [query, setQuery] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Filter by title, then group by recency (preserving the backend's
  // updated_at-desc order within each group).
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = q
      ? conversations.filter((c) => (c.title || '').toLowerCase().includes(q))
      : conversations;
    const map = new Map<MessageKey, Conversation[]>();
    for (const c of filtered) {
      const k: MessageKey = c.pinned ? 'groupPinned' : bucketKey(c.updated_at || c.created_at);
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
        {busyIds?.has(c.id) && (
          <Spin size="small" data-testid="conv-busy" style={{ flexShrink: 0 }} />
        )}
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
            {c.title || t('untitled')}
          </span>
        )}
        {(hovered === c.id || active) && editingId !== c.id && (
          <span style={{ display: 'flex', gap: 10, flexShrink: 0 }}>
            <Tooltip title={c.pinned ? t('unpin') : t('pin')}>
              {c.pinned ? (
                <PushpinFilled
                  data-testid="pin-toggle"
                  onClick={(e) => {
                    e.stopPropagation();
                    onTogglePin(c.id, false);
                  }}
                  style={{ color: 'var(--celadon)', fontSize: 13 }}
                />
              ) : (
                <PushpinOutlined
                  data-testid="pin-toggle"
                  onClick={(e) => {
                    e.stopPropagation();
                    onTogglePin(c.id, true);
                  }}
                  style={{ color: 'var(--ink-icon)', fontSize: 13 }}
                />
              )}
            </Tooltip>
            <Tooltip title={t('exportConversation')}>
              <DownloadOutlined
                data-testid="export-conversation"
                onClick={(e) => {
                  e.stopPropagation();
                  exportConversationMarkdown(c).catch(() => antdMessage.error(t('errExport')));
                }}
                style={{ color: 'var(--ink-icon)', fontSize: 13 }}
              />
            </Tooltip>
            <Tooltip title={t('rename')}>
              <EditOutlined
                onClick={(e) => {
                  e.stopPropagation();
                  startEdit(c.id, c.title || '');
                }}
                style={{ color: 'var(--ink-icon)', fontSize: 13 }}
              />
            </Tooltip>
            <Popconfirm
              title={t('deleteConfirm')}
              okText={t('delete')}
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
          <Tooltip title={t('collapseSidebar')}>
            <Button
              type="text"
              aria-label={t('collapseSidebar')}
              data-testid="sidebar-collapse"
              icon={<MenuFoldOutlined style={{ color: 'var(--ink-icon)' }} />}
              onClick={onCollapse}
            />
          </Tooltip>
        )}
      </div>

      <div style={{ padding: '4px 12px 12px' }}>
        <Tooltip title={t('newChatTooltip')}>
          <Button type="primary" icon={<PlusOutlined />} block onClick={onNew}>
            {t('newChat')}
          </Button>
        </Tooltip>
      </div>

      <div style={{ padding: '0 12px 10px' }}>
        <Input
          size="small"
          allowClear
          prefix={<SearchOutlined style={{ color: 'var(--ink-dim)' }} />}
          placeholder={t('searchConversations')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          data-testid="conversation-search"
        />
      </div>

      <div className="thin-scroll" style={{ flex: 1, overflowY: 'auto', padding: '4px 8px' }}>
        {conversations.length === 0 ? (
          <div style={{ color: 'var(--ink-dim)', fontSize: 13, padding: '8px 10px' }}>
            {t('noConversations')}
          </div>
        ) : groups.length === 0 ? (
          <div style={{ color: 'var(--ink-dim)', fontSize: 13, padding: '8px 10px' }}>
            {t('noSearchMatches')}
          </div>
        ) : (
          groups.map((g) => (
            <div key={g.label}>
              <div
                className="eyebrow"
                style={{ padding: '10px 10px 4px', color: 'var(--ink-dim)' }}
              >
                {t(g.label)}
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
            {user?.name ?? user?.email ?? ''}
          </div>
          {user?.email && (
            <div style={{ fontSize: 11, color: 'var(--ink-dim)' }}>{user.email}</div>
          )}
        </div>
        <div style={{ display: 'flex', flexShrink: 0 }}>
          <Tooltip title={t('settings')}>
            <Button
              type="text"
              aria-label={t('settings')}
              data-testid="settings-button"
              icon={<SettingOutlined style={{ color: 'var(--ink-icon)' }} />}
              onClick={() => setSettingsOpen(true)}
            />
          </Tooltip>
          <LocaleSwitcher />
          <ThemePicker />
          <Tooltip title={t('signOut')}>
            <Button
              type="text"
              aria-label={t('signOut')}
              icon={<LogoutOutlined style={{ color: 'var(--ink-icon)' }} />}
              onClick={logout}
            />
          </Tooltip>
        </div>
      </div>
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
