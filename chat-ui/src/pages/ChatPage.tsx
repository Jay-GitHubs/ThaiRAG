import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Button,
  Drawer,
  Grid,
  Layout,
  Modal,
  Segmented,
  Skeleton,
  Tag,
  message as antdMessage,
} from 'antd';
import {
  DatabaseOutlined,
  DownOutlined,
  MenuOutlined,
  MenuUnfoldOutlined,
  PictureOutlined,
  ReloadOutlined,
  RobotOutlined,
} from '@ant-design/icons';
import {
  listConversations,
  listWorkspaces,
  createConversation,
  deleteConversation,
  generateImage,
  getChatFeatures,
  listMessages,
  renameConversation,
  setMessageFeedback,
  streamMessage,
} from '../api/conversations';
import { parseAttachmentNames, parseCitations, parseImages } from '../api/types';
import type {
  Attachment,
  ChatFeatures,
  Citation,
  Conversation,
  MessageRow,
  StreamEvent,
  WorkspaceOption,
} from '../api/types';
import { ConversationSidebar } from '../components/ConversationSidebar';
import { MessageBubble, type UiMessage } from '../components/MessageBubble';
import { MessageComposer } from '../components/MessageComposer';
import { ScopeSelector } from '../components/ScopeSelector';
import { SourceDrawer } from '../components/SourceDrawer';

// Friendly labels for the backend's pipeline stage names (the `progress` SSE
// event), so the "preparing answer" state reads in plain language. Unknown
// stages fall back to a humanized version of the raw name.
const STAGE_LABELS: Record<string, string> = {
  query_analyzer: 'Understanding your question',
  self_rag_gate: 'Deciding what to look up',
  pipeline_orchestrator: 'Planning the answer',
  retrieval: 'Searching your documents',
  search: 'Searching your documents',
  context_curator: 'Reading the most relevant parts',
  output_guardrails: 'Reviewing the answer',
};

function stageLabel(stage: string): string {
  return (
    STAGE_LABELS[stage] ??
    `${stage.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase())}…`
  );
}

// Label the platform modifier key for the shortcuts help.
const MOD = /mac|iphone|ipad/i.test(navigator.userAgent) ? '⌘' : 'Ctrl';

export function ChatPage() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [loadingMsgs, setLoadingMsgs] = useState(false);
  const [sending, setSending] = useState(false);
  const [workspaces, setWorkspaces] = useState<WorkspaceOption[]>([]);
  // Scope chosen for the *next* new conversation (null = all workspaces).
  const [newScope, setNewScope] = useState<string | null>(null);
  // Mode chosen for the *next* new conversation: 'rag' (knowledge base) or
  // 'general' (non-RAG plain assistant).
  const [newMode, setNewMode] = useState<'rag' | 'general'>('rag');
  // Server feature flags (general chat on/off, image generation available).
  const [features, setFeatures] = useState<ChatFeatures>({
    general_chat_enabled: true,
    image_generation_enabled: false,
  });
  // In general mode with image-gen available: when true, the next send generates
  // an image instead of a text reply.
  const [imageMode, setImageMode] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const streamStartRef = useRef(0);
  const [atBottom, setAtBottom] = useState(true);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  // Conversation id we're currently streaming into. Lets the activeId effect
  // tell a real conversation *switch* (abort + reload) from the activeId change
  // that happens when we lazily create the conversation we're sending to (which
  // must NOT abort the just-started stream or reload over the optimistic turn).
  const streamingConvRef = useRef<string | null>(null);
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const [drawerOpen, setDrawerOpen] = useState(false);
  // Desktop-only: collapse the sidebar to widen the reading column (persisted).
  const [sidebarCollapsed, setSidebarCollapsed] = useState(
    () => localStorage.getItem('thairag-sidebar-collapsed') === '1',
  );
  const toggleSidebar = useCallback(
    () =>
      setSidebarCollapsed((c) => {
        const next = !c;
        localStorage.setItem('thairag-sidebar-collapsed', next ? '1' : '0');
        return next;
      }),
    [],
  );
  // Citation whose source is open in the in-app source viewer (null = closed).
  const [sourceCitation, setSourceCitation] = useState<Citation | null>(null);

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
    getChatFeatures()
      .then(setFeatures)
      .catch(() => {
        /* defaults: general on, image off — affordances stay hidden */
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
    // When activeId became the conversation we're actively streaming into (a
    // first-message lazy-create), the optimistic turn + live stream are
    // authoritative — don't abort the stream or reload empty over it. Guard on a
    // non-null ref so this doesn't also short-circuit the activeId→null case
    // (e.g. deleting the active conversation), which must still clear the pane.
    if (streamingConvRef.current && streamingConvRef.current === activeId) return;
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
            attachments: parseAttachmentNames(r.attachments),
            feedback: r.feedback,
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

  // Autoscroll only when the user is already near the bottom, so scrolling up to
  // read isn't yanked back down while the answer streams.
  useEffect(() => {
    if (atBottom) bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, atBottom]);

  const onScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    setAtBottom(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
  }, []);

  const scrollToBottom = useCallback(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, []);

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
        case 'progress':
          // Show what the pipeline is doing while the answer is being prepared,
          // so slow retrieval (e.g. an all-workspaces search) doesn't look frozen.
          updateLastAssistant((m) => ({ ...m, progress: stageLabel(evt.stage) }));
          break;
        case 'token':
          updateLastAssistant((m) => ({ ...m, content: m.content + evt.text, progress: undefined }));
          break;
        case 'citation':
          updateLastAssistant((m) => ({ ...m, citations: evt.citations }));
          break;
        case 'image':
          updateLastAssistant((m) => ({ ...m, images: evt.images }));
          break;
        case 'done':
          updateLastAssistant((m) => ({
            ...m,
            id: evt.message_id,
            streaming: false,
            progress: undefined,
            usage: evt.usage,
            confidence: evt.confidence ?? undefined,
            confidenceSummary: evt.confidence_summary ?? undefined,
            confidenceFactors: evt.confidence_factors ?? undefined,
            elapsedMs: streamStartRef.current ? Date.now() - streamStartRef.current : undefined,
          }));
          break;
        case 'error':
          antdMessage.error(evt.message);
          updateLastAssistant((m) => ({ ...m, streaming: false, progress: undefined, error: true }));
          break;
      }
    },
    [updateLastAssistant],
  );

  // The last streaming action, kept so a failed stream can be retried. A failed
  // plain send was never persisted (the backend saves the turn at stream end),
  // so its retry re-sends; failed regenerate/edit re-run themselves (their
  // original rows are still intact — deletion is deferred until persist).
  const lastActionRef = useRef<{
    kind: 'send' | 'regenerate' | 'edit';
    text: string;
    attachments: Attachment[];
  } | null>(null);

  const handleStop = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  // Thumbs feedback on an assistant message. Optimistic; reverts on failure.
  const handleFeedback = useCallback(
    (messageId: string, value: number) => {
      if (!activeId) return;
      let prevValue = 0;
      setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== messageId) return m;
          prevValue = m.feedback ?? 0;
          return { ...m, feedback: value };
        }),
      );
      setMessageFeedback(activeId, messageId, value).catch(() => {
        antdMessage.error('Failed to save feedback');
        setMessages((prev) =>
          prev.map((m) => (m.id === messageId ? { ...m, feedback: prevValue } : m)),
        );
      });
    },
    [activeId],
  );

  // Start a fresh chat: clear the view and let the welcome screen's mode/scope
  // pickers choose. The conversation is created lazily on the first message
  // (handleSend), so toggling General/Knowledge-Base actually applies to it.
  const handleNew = useCallback(() => {
    setActiveId(null);
    setMessages([]);
  }, []);

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

  const handleRename = useCallback(async (id: string, title: string) => {
    try {
      const updated = await renameConversation(id, title);
      setConversations((prev) => prev.map((c) => (c.id === id ? updated : c)));
    } catch {
      antdMessage.error('Failed to rename conversation');
    }
  }, []);

  const handleSend = useCallback(
    async (text: string, attachments: Attachment[] = []) => {
      // Ensure a conversation exists (lazily create one on first message).
      let convId = activeId;
      let isFirstMessage = messages.length === 0;
      if (!convId) {
        try {
          const conv = await createConversation(
            undefined,
            newMode === 'general' ? null : newScope,
            newMode,
          );
          setConversations((prev) => [conv, ...prev]);
          setActiveId(conv.id);
          convId = conv.id;
          isFirstMessage = true;
        } catch {
          antdMessage.error('Failed to start conversation');
          return;
        }
      }

      // ── Image generation (general mode): one-shot, non-streaming ──
      if (imageMode && features.image_generation_enabled) {
        setMessages((prev) => [
          ...prev,
          { role: 'user', content: text, citations: [], images: [] },
          { role: 'assistant', content: '', citations: [], images: [], streaming: true },
        ]);
        setSending(true);
        try {
          const row: MessageRow = await generateImage(convId, text);
          updateLastAssistant((m) => ({
            ...m,
            id: row.id,
            streaming: false,
            content: row.content,
            images: parseImages(row.images),
          }));
        } catch (e) {
          antdMessage.error(e instanceof Error ? e.message : 'Image generation failed');
          updateLastAssistant((m) => ({ ...m, streaming: false }));
        } finally {
          setSending(false);
        }
        return;
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
      lastActionRef.current = { kind: 'send', text, attachments };
      const controller = new AbortController();
      abortRef.current = controller;
      streamingConvRef.current = convId;
      streamStartRef.current = Date.now();

      try {
        await streamMessage(convId, text, handleStreamEvent, controller.signal, attachments);
      } catch (e) {
        // A user-pressed Stop aborts the fetch — keep the partial answer, no toast.
        if (!controller.signal.aborted) {
          antdMessage.error(e instanceof Error ? e.message : 'Streaming failed');
          updateLastAssistant((m) => ({ ...m, streaming: false, progress: undefined, error: true }));
        } else {
          updateLastAssistant((m) => ({ ...m, streaming: false }));
        }
      } finally {
        abortRef.current = null;
        streamingConvRef.current = null;
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
    [
      activeId,
      messages.length,
      newScope,
      newMode,
      imageMode,
      features.image_generation_enabled,
      handleStreamEvent,
      updateLastAssistant,
    ],
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
    lastActionRef.current = { kind: 'regenerate', text: '', attachments: [] };
    const controller = new AbortController();
    abortRef.current = controller;
    streamingConvRef.current = activeId;
    streamStartRef.current = Date.now();
    try {
      await streamMessage(activeId, '', handleStreamEvent, controller.signal, undefined, true);
    } catch (e) {
      if (!controller.signal.aborted) {
        antdMessage.error(e instanceof Error ? e.message : 'Regeneration failed');
        updateLastAssistant((m) => ({ ...m, streaming: false, progress: undefined, error: true }));
      } else {
        updateLastAssistant((m) => ({ ...m, streaming: false }));
      }
    } finally {
      abortRef.current = null;
      streamingConvRef.current = null;
      setSending(false);
    }
  }, [activeId, sending, handleStreamEvent, updateLastAssistant]);

  const handleEdit = useCallback(
    async (text: string) => {
      const edited = text.trim();
      if (!activeId || sending || !edited) return;
      // Replace the last user turn: drop it (and its answer) from view, then
      // stream a fresh answer for the edited prompt. The backend deletes the old
      // user+assistant rows and persists the edited pair in their place.
      setMessages((prev) => {
        let i = prev.length - 1;
        while (i >= 0 && prev[i].role !== 'user') i--;
        if (i < 0) return prev;
        const next = prev.slice(0, i);
        next.push({ role: 'user', content: edited, citations: [], images: [], attachments: [] });
        next.push({ role: 'assistant', content: '', citations: [], images: [], streaming: true });
        return next;
      });
      setSending(true);
      lastActionRef.current = { kind: 'edit', text: edited, attachments: [] };
      const controller = new AbortController();
      abortRef.current = controller;
      streamingConvRef.current = activeId;
      streamStartRef.current = Date.now();
      try {
        await streamMessage(activeId, edited, handleStreamEvent, controller.signal, undefined, false, true);
      } catch (e) {
        if (!controller.signal.aborted) {
          antdMessage.error(e instanceof Error ? e.message : 'Edit failed');
          updateLastAssistant((m) => ({ ...m, streaming: false, progress: undefined, error: true }));
        } else {
          updateLastAssistant((m) => ({ ...m, streaming: false }));
        }
      } finally {
        abortRef.current = null;
        streamingConvRef.current = null;
        setSending(false);
      }
    },
    [activeId, sending, handleStreamEvent, updateLastAssistant],
  );

  // Retry a failed stream. Dispatches on what failed: regenerate/edit re-run
  // themselves (their original rows survive a failed attempt), while a plain
  // send re-sends after dropping the optimistic pair (it was never persisted).
  const handleRetry = useCallback(() => {
    const last = lastActionRef.current;
    if (!last || sending) return;
    if (last.kind === 'regenerate') {
      handleRegenerate();
      return;
    }
    if (last.kind === 'edit') {
      handleEdit(last.text);
      return;
    }
    setMessages((prev) => {
      const next = prev.slice();
      if (next.length > 0 && next[next.length - 1].role === 'assistant') next.pop();
      if (next.length > 0 && next[next.length - 1].role === 'user') next.pop();
      return next;
    });
    handleSend(last.text, last.attachments);
  }, [sending, handleRegenerate, handleEdit, handleSend]);

  // Index of the last user turn — the only one offered an edit affordance (and
  // only while idle, since editing re-runs the conversation tail).
  const lastUserIdx = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'user') return i;
    }
    return -1;
  }, [messages]);

  // Keyboard shortcuts. Mod = ⌘ (mac) / Ctrl. We stay out of the way while the
  // user is typing in a field, except for the global new-chat and stop bindings.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const el = e.target as HTMLElement | null;
      const typing =
        el?.tagName === 'INPUT' || el?.tagName === 'TEXTAREA' || el?.isContentEditable === true;

      if (mod && e.shiftKey && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        void handleNew();
      } else if (e.key === 'Escape' && sending) {
        handleStop();
      } else if (!typing && !mod && e.key === '/') {
        e.preventDefault();
        document.querySelector<HTMLTextAreaElement>('[data-testid="composer-input"]')?.focus();
      } else if (!typing && e.key === '?') {
        e.preventDefault();
        setShowShortcuts(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [handleNew, handleStop, sending]);

  // Effective mode: the active conversation's mode once created, else the choice
  // for the next new chat. General mode = non-RAG (no corpus retrieval).
  const chatMode: 'rag' | 'general' = activeConversation?.mode ?? newMode;
  const isGeneral = chatMode === 'general';

  // Image mode only makes sense in general chat with a configured model; reset it
  // whenever we leave that context so a stray toggle can't reach the send path.
  useEffect(() => {
    if (!isGeneral || !features.image_generation_enabled) setImageMode(false);
  }, [isGeneral, features.image_generation_enabled]);

  // If the admin disables general chat, force the next-chat selection back to RAG
  // (the picker is hidden, but the prior selection could still be 'general').
  useEffect(() => {
    if (!features.general_chat_enabled) setNewMode('rag');
  }, [features.general_chat_enabled]);

  const suggestions = isGeneral
    ? ['Write a Python function to parse CSV', 'อธิบายเรื่อง machine learning แบบสั้น ๆ']
    : ['สรุปขั้นตอนการขอสินเชื่อ', 'What documents do I need to apply?'];

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
      onRename={handleRename}
      onCollapse={isMobile ? undefined : toggleSidebar}
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
        <Layout.Sider
          width={272}
          collapsible
          collapsed={sidebarCollapsed}
          collapsedWidth={0}
          trigger={null}
          style={{ background: 'var(--ink)' }}
        >
          {sidebar}
        </Layout.Sider>
      )}
      <Layout.Content
        style={{
          display: 'flex',
          flexDirection: 'column',
          height: '100%',
          background: 'var(--canvas)',
          position: 'relative',
        }}
      >
        {(isMobile || sidebarCollapsed) && (
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
              aria-label={isMobile ? 'Open menu' : 'Show sidebar'}
              data-testid={isMobile ? 'mobile-menu' : 'sidebar-expand'}
              icon={isMobile ? <MenuOutlined /> : <MenuUnfoldOutlined />}
              onClick={() => (isMobile ? setDrawerOpen(true) : toggleSidebar())}
            />
            <span style={{ fontFamily: 'var(--font-display)', fontWeight: 600 }}>ThaiRAG</span>
          </div>
        )}
        {messages.length > 0 && (
          <div
            data-testid="mode-bar"
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
            {isGeneral ? (
              <>
                <RobotOutlined />
                <span>General chat</span>
                <Tag color="default" style={{ margin: 0 }}>
                  not using your documents
                </Tag>
              </>
            ) : (
              <>
                <DatabaseOutlined />
                <span>Searching</span>
                <Tag
                  color={activeConversation?.workspace_scope ? 'green' : 'default'}
                  style={{ margin: 0 }}
                >
                  {activeScopeName ?? 'All my workspaces'}
                </Tag>
              </>
            )}
          </div>
        )}
        <div
          ref={scrollRef}
          onScroll={onScroll}
          className="thin-scroll"
          style={{ flex: 1, overflowY: 'auto', padding: '28px 20px' }}
        >
          {loadingMsgs ? (
            <div style={{ maxWidth: 820, margin: '0 auto', paddingTop: 8 }} data-testid="msg-skeleton">
              {[0, 1, 2].map((i) => (
                <div
                  key={i}
                  style={{
                    display: 'flex',
                    justifyContent: i % 2 === 0 ? 'flex-end' : 'flex-start',
                    marginBottom: 26,
                  }}
                >
                  <Skeleton.Button
                    active
                    block={i % 2 === 1}
                    style={{ width: i % 2 === 0 ? 240 : '100%', height: i % 2 === 0 ? 44 : 80 }}
                  />
                </div>
              ))}
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
                {isGeneral ? 'How can I help?' : 'What do you want to find?'}
              </h1>
              <p style={{ color: 'var(--text-muted)', fontSize: 16, marginTop: 12 }}>
                {isGeneral
                  ? 'General assistant — answers from the model’s own knowledge, not your documents.'
                  : 'ถามจากคลังเอกสารของคุณ แล้วได้คำตอบพร้อมหน้าต้นทาง'}
              </p>

              {/* Mode picker for the next new chat: Knowledge Base (RAG) vs General.
                  Hidden entirely when the admin has disabled general chat — there's
                  only one mode then, so a dead toggle would just confuse. */}
              {features.general_chat_enabled && (
                <div style={{ marginTop: 22 }}>
                  <Segmented
                    data-testid="mode-segmented"
                    value={newMode}
                    onChange={(v) => setNewMode(v as 'rag' | 'general')}
                    options={[
                      { label: 'Knowledge base', value: 'rag', icon: <DatabaseOutlined /> },
                      { label: 'General', value: 'general', icon: <RobotOutlined /> },
                    ]}
                  />
                </div>
              )}

              {/* Image generation is a general-mode affordance, shown only when a
                  text-to-image model is actually configured on the backend. */}
              {isGeneral && features.image_generation_enabled && (
                <div style={{ marginTop: 16 }}>
                  <Segmented
                    data-testid="image-mode-segmented"
                    value={imageMode ? 'image' : 'text'}
                    onChange={(v) => setImageMode(v === 'image')}
                    options={[
                      { label: 'Text', value: 'text', icon: <RobotOutlined /> },
                      { label: 'Image', value: 'image', icon: <PictureOutlined /> },
                    ]}
                  />
                </div>
              )}

              {/* Scope picker only matters for RAG — general chat never searches the corpus. */}
              {!isGeneral && workspaces.length > 0 && (
                <div style={{ marginTop: 16 }}>
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
                <MessageBubble
                  key={m.id ?? i}
                  message={m}
                  onFeedback={handleFeedback}
                  onSourceClick={setSourceCitation}
                  editable={!sending && i === lastUserIdx}
                  onEdit={handleEdit}
                />
              ))}
              {!sending &&
                messages[messages.length - 1]?.role === 'assistant' &&
                (messages[messages.length - 1]?.error ? (
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'center',
                      alignItems: 'center',
                      gap: 10,
                      marginTop: 4,
                    }}
                  >
                    <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                      The answer was interrupted
                    </span>
                    <Button
                      size="small"
                      type="primary"
                      icon={<ReloadOutlined />}
                      onClick={handleRetry}
                      data-testid="retry-answer"
                    >
                      Retry
                    </Button>
                  </div>
                ) : (
                  <div style={{ display: 'flex', justifyContent: 'center', marginTop: 4 }}>
                    <Button size="small" icon={<ReloadOutlined />} onClick={handleRegenerate}>
                      Regenerate
                    </Button>
                  </div>
                ))}
              <div ref={bottomRef} />
            </div>
          )}
        </div>
        {!atBottom && messages.length > 0 && (
          <Button
            shape="circle"
            icon={<DownOutlined />}
            onClick={scrollToBottom}
            data-testid="scroll-to-bottom"
            aria-label="Scroll to latest"
            style={{
              position: 'absolute',
              bottom: 88,
              left: '50%',
              transform: 'translateX(-50%)',
              boxShadow: '0 2px 10px var(--shadow-md)',
              zIndex: 5,
            }}
          />
        )}
        <div style={{ borderTop: '1px solid var(--line)', background: 'var(--canvas)' }}>
          <div style={{ maxWidth: 820, margin: '0 auto', width: '100%' }}>
            <MessageComposer disabled={sending} onSend={handleSend} onStop={handleStop} />
          </div>
        </div>
      </Layout.Content>
      <SourceDrawer citation={sourceCitation} onClose={() => setSourceCitation(null)} />
      <Modal
        open={showShortcuts}
        onCancel={() => setShowShortcuts(false)}
        footer={null}
        title="Keyboard shortcuts"
        width={400}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12, paddingTop: 4 }}>
          {[
            { keys: `${MOD} + Shift + O`, label: 'New chat' },
            { keys: '/', label: 'Focus the message box' },
            { keys: 'Enter', label: 'Send · Shift + Enter for a new line' },
            { keys: 'Esc', label: 'Stop a streaming answer' },
            { keys: '?', label: 'Show this help' },
          ].map((s) => (
            <div
              key={s.label}
              style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 16 }}
            >
              <span style={{ color: 'var(--text-muted)', fontSize: 14 }}>{s.label}</span>
              <kbd
                style={{
                  fontFamily: 'var(--font-mono)',
                  fontSize: 12,
                  background: 'var(--celadon-tint)',
                  color: 'var(--celadon-deep)',
                  border: '1px solid var(--line)',
                  borderRadius: 6,
                  padding: '2px 8px',
                  whiteSpace: 'nowrap',
                }}
              >
                {s.keys}
              </kbd>
            </div>
          ))}
        </div>
      </Modal>
    </Layout>
  );
}
