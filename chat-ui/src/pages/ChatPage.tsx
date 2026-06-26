import { useCallback, useEffect, useRef, useState } from 'react';
import { Layout, Spin, message as antdMessage } from 'antd';
import {
  listConversations,
  createConversation,
  deleteConversation,
  listMessages,
  renameConversation,
  streamMessage,
} from '../api/conversations';
import { parseCitations, parseImages } from '../api/types';
import type { Conversation } from '../api/types';
import { ConversationSidebar } from '../components/ConversationSidebar';
import { MessageBubble, type UiMessage } from '../components/MessageBubble';
import { MessageComposer } from '../components/MessageComposer';

export function ChatPage() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [loadingMsgs, setLoadingMsgs] = useState(false);
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  // Initial conversation list.
  useEffect(() => {
    listConversations()
      .then((list) => {
        setConversations(list);
        if (list.length > 0) setActiveId(list[0].id);
      })
      .catch(() => antdMessage.error('Failed to load conversations'));
  }, []);

  // Load messages when the active conversation changes.
  useEffect(() => {
    if (!activeId) {
      setMessages([]);
      return;
    }
    setLoadingMsgs(true);
    listMessages(activeId)
      .then((rows) => {
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
      .catch(() => antdMessage.error('Failed to load messages'))
      .finally(() => setLoadingMsgs(false));
  }, [activeId]);

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

  const handleNew = useCallback(async () => {
    try {
      const conv = await createConversation();
      setConversations((prev) => [conv, ...prev]);
      setActiveId(conv.id);
      setMessages([]);
    } catch {
      antdMessage.error('Failed to create conversation');
    }
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

  const handleSend = useCallback(
    async (text: string) => {
      // Ensure a conversation exists (lazily create one on first message).
      let convId = activeId;
      let isFirstMessage = messages.length === 0;
      if (!convId) {
        try {
          const conv = await createConversation();
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
        { role: 'user', content: text, citations: [], images: [] },
        { role: 'assistant', content: '', citations: [], images: [], streaming: true },
      ]);
      setSending(true);

      try {
        await streamMessage(convId, text, (evt) => {
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
        });
      } catch (e) {
        antdMessage.error(e instanceof Error ? e.message : 'Streaming failed');
        updateLastAssistant((m) => ({ ...m, streaming: false }));
      } finally {
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
    [activeId, messages.length, updateLastAssistant],
  );

  const suggestions = [
    'สรุปขั้นตอนการขอสินเชื่อ',
    'What documents do I need to apply?',
  ];

  return (
    <Layout style={{ height: '100%' }}>
      <Layout.Sider
        width={272}
        style={{ background: 'var(--ink)' }}
        breakpoint="md"
        collapsedWidth={0}
      >
        <ConversationSidebar
          conversations={conversations}
          activeId={activeId}
          onSelect={setActiveId}
          onNew={handleNew}
          onDelete={handleDelete}
        />
      </Layout.Sider>
      <Layout.Content
        style={{
          display: 'flex',
          flexDirection: 'column',
          height: '100%',
          background: 'var(--canvas)',
        }}
      >
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
              <div ref={bottomRef} />
            </div>
          )}
        </div>
        <div style={{ borderTop: '1px solid var(--line)', background: 'var(--canvas)' }}>
          <div style={{ maxWidth: 820, margin: '0 auto', width: '100%' }}>
            <MessageComposer disabled={sending} onSend={handleSend} />
          </div>
        </div>
      </Layout.Content>
    </Layout>
  );
}
