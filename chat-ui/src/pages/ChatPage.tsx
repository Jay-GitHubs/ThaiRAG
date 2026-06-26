import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, Drawer, Grid, Layout, Spin, Tag, message as antdMessage } from 'antd';
import { DatabaseOutlined, MenuOutlined, ReloadOutlined } from '@ant-design/icons';
import {
  listConversations,
  listWorkspaces,
  createConversation,
  deleteConversation,
  listMessages,
  renameConversation,
  streamMessage,
} from '../api/conversations';
import { parseCitations, parseImages } from '../api/types';
import type { Attachment, Conversation, StreamEvent, WorkspaceOption } from '../api/types';
import { ConversationSidebar } from '../components/ConversationSidebar';
import { MessageBubble, type UiMessage } from '../components/MessageBubble';
import { MessageComposer } from '../components/MessageComposer';
import { ScopeSelector } from '../components/ScopeSelector';

export function ChatPage() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [loadingMsgs, setLoadingMsgs] = useState(false);
  const [sending, setSending] = useState(false);
  const [workspaces, setWorkspaces] = useState<WorkspaceOption[]>([]);
  // Scope chosen for the *next* new conversation (null = all workspaces).
  const [newScope, setNewScope] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const abortRef = useRef<AbortController | null>(null);
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const [drawerOpen, setDrawerOpen] = useState(false);

  // Initial conversation list + the user's workspaces (for the scope picker).
  useEffect(() => {
    listConversations()
      .then((list) => {
        setConversations(list);
        if (list.length > 0) setActiveId(list[0].id);
      })
      .catch(() => antdMessage.error('Failed to load conversations'));
    listWorkspaces()
      .then(setWorkspaces)
      .catch(() => {
        /* no picker if this fails; chat still works across all workspaces */
      });
  }, []);

  const wsName = useMemo(() => {
    const m = new Map(workspaces.map((w) => [w.id, w.name]));
    return (id?: string | null) => (id ? (m.get(id) ?? 'Workspace') : null);
  }, [workspaces]);

  const activeConversation = conversations.find((c) => c.id === activeId) ?? null;

  // Load messages when the active conversation changes. Switching aborts any
  // in-flight stream (so its tokens can't bleed into the new conversation) and
  // ignores a stale load that resolves after another switch.
  useEffect(() => {
    abortRef.current?.abort();
    if (!activeId) {
      setMessages([]);
      return;
    }
    let cancelled = false;
    setLoadingMsgs(true);
    listMessages(activeId)
      .then((rows) => {
        if (cancelled) return;
        setMessages(
          rows.map((r) => ({
            id: r.id,
            role: r.role === 'user' ? 'user' : 'assistant',
            content: r.content,
            citations: parseCitations(r.citations),
            images: parseImages(r.images),
          })),
        );
      })
      .catch(() => {
        if (!cancelled) antdMessage.error('Failed to load messages');
      })
      .finally(() => {
        if (!cancelled) setLoadingMsgs(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeId]);

  // Abort a running stream if the page unmounts.
  useEffect(() => () => abortRef.current?.abort(), []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const updateLastAssistant = useCallback((fn: (m: UiMessage) => UiMessage) => {
    setMessages((prev) => {
      if (prev.length === 0) return prev;
      const next = prev.slice();
      const last = next[next.length - 1];
      if (last.role !== 'assistant') return prev;
      next[next.length - 1] = fn(last);
      return next;
    });
  }, []);

  // Shared SSE event handler for both send and regenerate.
  const handleStreamEvent = useCallback(
    (evt: StreamEvent) => {
      switch (evt.type) {
        case 'token':
          updateLastAssistant((m) => ({ ...m, content: m.content + evt.text }));
          break;
        case 'citation':
          updateLastAssistant((m) => ({ ...m, citations: evt.citations }));
          break;
        case 'image':
          updateLastAssistant((m) => ({ ...m, images: evt.images }));
          break;
        case 'done':
          updateLastAssistant((m) => ({ ...m, id: evt.message_id, streaming: false }));
          break;
        case 'error':
          antdMessage.error(evt.message);
          updateLastAssistant((m) => ({ ...m, streaming: false }));
          break;
      }
    },
    [updateLastAssistant],
  );

  const handleStop = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  const handleNew = useCallback(async () => {
    try {
      const conv = await createConversation(undefined, newScope);
      setConversations((prev) => [conv, ...prev]);
      setActiveId(conv.id);
      setMessages([]);
    } catch {
      antdMessage.error('Failed to create conversation');
    }
  }, [newScope]);

  const handleDelete = useCallback(
    async (id: string) => {
      try {
        await deleteConversation(id);
        setConversations((prev) => prev.filter((c) => c.id !== id));
        if (id === activeId) setActiveId(null);
      } catch {
        antdMessage.error('Failed to delete conversation');
      }
    },
    [activeId],
  );

  const handleSend = useCallback(
    async (text: string, attachments: Attachment[] = []) => {
      // Ensure a conversation exists (lazily create one on first message).
      let convId = activeId;
      let isFirstMessage = messages.length === 0;
      if (!convId) {
        try {
          const conv = await createConversation(undefined, newScope);
          setConversations((prev) => [conv, ...prev]);
          setActiveId(conv.id);
          convId = conv.id;
          isFirstMessage = true;
        } catch {
          antdMessage.error('Failed to start conversation');
          return;
        }
      }

      setMessages((prev) => [
        ...prev,
        {
          role: 'user',
          content: text,
          citations: [],
          images: [],
          attachments: attachments.map((a) => a.name),
        },
        { role: 'assistant', content: '', citations: [], images: [], streaming: true },
      ]);
      setSending(true);
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await streamMessage(convId, text, handleStreamEvent, controller.signal, attachments);
      } catch (e) {
        // A user-pressed Stop aborts the fetch — keep the partial answer, no toast.
        if (!controller.signal.aborted) {
          antdMessage.error(e instanceof Error ? e.message : 'Streaming failed');
        }
        updateLastAssistant((m) => ({ ...m, streaming: false }));
      } finally {
        abortRef.current = null;
        setSending(false);
      }

      // Auto-title a fresh conversation from its first message.
      if (isFirstMessage && convId) {
        const title = text.length > 60 ? `${text.slice(0, 60)}…` : text;
        renameConversation(convId, title)
          .then((updated) =>
            setConversations((prev) => {
              const rest = prev.filter((c) => c.id !== updated.id);
              return [updated, ...rest];
            }),
          )
          .catch(() => {
            /* non-fatal: title stays default */
          });
      }
    },
    [activeId, messages.length, newScope, handleStreamEvent, updateLastAssistant],
  );

  const handleRegenerate = useCallback(async () => {
    if (!activeId || sending) return;
    // Drop the last answer from view and stream a fresh one (the backend deletes
    // the old assistant row and re-answers the same last user message).
    setMessages((prev) => {
      const next = prev.slice();
      if (next.length > 0 && next[next.length - 1].role === 'assistant') next.pop();
      next.push({ role: 'assistant', content: '', citations: [], images: [], streaming: true });
      return next;
    });
    setSending(true);
    const controller = new AbortController();
    abortRef.current = controller;
    try {
      await streamMessage(activeId, '', handleStreamEvent, controller.signal, undefined, true);
    } catch (e) {
      if (!controller.signal.aborted) {
        antdMessage.error(e instanceof Error ? e.message : 'Regeneration failed');
      }
      updateLastAssistant((m) => ({ ...m, streaming: false }));
    } finally {
      abortRef.current = null;
      setSending(false);
    }
  }, [activeId, sending, handleStreamEvent, updateLastAssistant]);

  const suggestions = [
    'สรุปขั้นตอนการขอสินเชื่อ',
    'What documents do I need to apply?',
  ];

  // Scope shown for the active conversation: its pin once created, else the
  // picker selection for the next new chat.
  const activeScopeName = activeConversation
    ? wsName(activeConversation.workspace_scope)
    : wsName(newScope);

  const sidebar = (
    <ConversationSidebar
      conversations={conversations}
      activeId={activeId}
      onSelect={(id) => {
        setActiveId(id);
        setDrawerOpen(false);
      }}
      onNew={() => {
        void handleNew();
        setDrawerOpen(false);
      }}
      onDelete={handleDelete}
    />
  );

  return (
    <Layout style={{ height: '100%' }}>
      {isMobile ? (
        <Drawer
          open={drawerOpen}
          onClose={() => setDrawerOpen(false)}
          placement="left"
          width={272}
          closable={false}
          styles={{ body: { padding: 0, background: 'var(--ink)' } }}
        >
          {sidebar}
        </Drawer>
      ) : (
        <Layout.Sider width={272} style={{ background: 'var(--ink)' }}>
          {sidebar}
        </Layout.Sider>
      )}
      <Layout.Content
        style={{
          display: 'flex',
          flexDirection: 'column',
          height: '100%',
          background: 'var(--canvas)',
        }}
      >
        {isMobile && (
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              padding: '8px 12px',
              borderBottom: '1px solid var(--line)',
            }}
          >
            <Button
              type="text"
              aria-label="Menu"
              icon={<MenuOutlined />}
              onClick={() => setDrawerOpen(true)}
            />
            <span style={{ fontFamily: 'var(--font-display)', fontWeight: 600 }}>ThaiRAG</span>
          </div>
        )}
        {messages.length > 0 && (
          <div
            style={{
              borderBottom: '1px solid var(--line)',
              padding: '9px 20px',
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              fontSize: 13,
              color: 'var(--text-muted)',
            }}
          >
            <DatabaseOutlined />
            <span>Searching</span>
            <Tag
              color={activeConversation?.workspace_scope ? 'green' : 'default'}
              style={{ margin: 0 }}
            >
              {activeScopeName ?? 'All my workspaces'}
            </Tag>
          </div>
        )}
        <div className="thin-scroll" style={{ flex: 1, overflowY: 'auto', padding: '28px 20px' }}>
          {loadingMsgs ? (
            <div style={{ textAlign: 'center', marginTop: 100 }}>
              <Spin />
            </div>
          ) : messages.length === 0 ? (
            <div style={{ maxWidth: 640, margin: '14vh auto 0', textAlign: 'center' }}>
              <h1
                style={{
                  fontFamily: 'var(--font-display)',
                  fontWeight: 600,
                  fontSize: 30,
                  margin: 0,
                  color: 'var(--text)',
                }}
              >
                What do you want to find?
              </h1>
              <p style={{ color: 'var(--text-muted)', fontSize: 16, marginTop: 12 }}>
                ถามจากคลังเอกสารของคุณ แล้วได้คำตอบพร้อมหน้าต้นทาง
              </p>
              {workspaces.length > 0 && (
                <div style={{ marginTop: 22 }}>
                  <div className="eyebrow" style={{ marginBottom: 8 }}>
                    Search in
                  </div>
                  <ScopeSelector workspaces={workspaces} value={newScope} onChange={setNewScope} />
                </div>
              )}
              <div
                style={{
                  display: 'flex',
                  gap: 10,
                  justifyContent: 'center',
                  flexWrap: 'wrap',
                  marginTop: 26,
                }}
              >
                {suggestions.map((s) => (
                  <button
                    key={s}
                    onClick={() => handleSend(s)}
                    disabled={sending}
                    style={{
                      fontFamily: 'var(--font-body)',
                      fontSize: 14,
                      color: 'var(--text)',
                      background: 'var(--surface)',
                      border: '1px solid var(--line)',
                      borderRadius: 10,
                      padding: '10px 14px',
                      cursor: sending ? 'default' : 'pointer',
                    }}
                  >
                    {s}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div style={{ maxWidth: 820, margin: '0 auto' }}>
              {messages.map((m, i) => (
                <MessageBubble key={m.id ?? i} message={m} />
              ))}
              {!sending && messages[messages.length - 1]?.role === 'assistant' && (
                <div style={{ display: 'flex', justifyContent: 'center', marginTop: 4 }}>
                  <Button size="small" icon={<ReloadOutlined />} onClick={handleRegenerate}>
                    Regenerate
                  </Button>
                </div>
              )}
              <div ref={bottomRef} />
            </div>
          )}
        </div>
        <div style={{ borderTop: '1px solid var(--line)', background: 'var(--canvas)' }}>
          <div style={{ maxWidth: 820, margin: '0 auto', width: '100%' }}>
            <MessageComposer disabled={sending} onSend={handleSend} onStop={handleStop} />
          </div>
        </div>
      </Layout.Content>
    </Layout>
  );
}
