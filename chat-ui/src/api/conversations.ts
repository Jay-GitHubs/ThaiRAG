import client, { getToken } from './client';
import type {
  Attachment,
  ChatFeatures,
  Conversation,
  DocumentSource,
  MessageRow,
  StreamEvent,
  WorkspaceOption,
} from './types';

export async function listConversations(): Promise<Conversation[]> {
  const res = await client.get<Conversation[]>('/api/chat/conversations');
  return res.data;
}

export async function listWorkspaces(): Promise<WorkspaceOption[]> {
  const res = await client.get<WorkspaceOption[]>('/api/chat/workspaces');
  return res.data;
}

/** Feature flags (general chat on/off, image generation available). */
export async function getChatFeatures(): Promise<ChatFeatures> {
  const res = await client.get<ChatFeatures>('/api/chat/features');
  return res.data;
}

/** Generate an image from a prompt and persist it as a turn (general mode). */
export async function generateImage(
  conversationId: string,
  prompt: string,
): Promise<MessageRow> {
  const res = await client.post<MessageRow>(
    `/api/chat/conversations/${conversationId}/images`,
    { prompt },
  );
  return res.data;
}

export async function createConversation(
  title?: string,
  workspaceScope?: string | null,
  mode?: 'rag' | 'general',
): Promise<Conversation> {
  const res = await client.post<Conversation>('/api/chat/conversations', {
    title: title ?? '',
    ...(workspaceScope ? { workspace_scope: workspaceScope } : {}),
    ...(mode ? { mode } : {}),
  });
  return res.data;
}

export async function renameConversation(id: string, title: string): Promise<Conversation> {
  const res = await client.patch<Conversation>(`/api/chat/conversations/${id}`, { title });
  return res.data;
}

export async function setConversationPinned(id: string, pinned: boolean): Promise<Conversation> {
  const res = await client.patch<Conversation>(`/api/chat/conversations/${id}`, { pinned });
  return res.data;
}

export async function summarizeConversation(id: string): Promise<string> {
  const res = await client.post<{ summary: string }>(`/api/chat/conversations/${id}/summarize`, {});
  return res.data.summary;
}

export async function deleteConversation(id: string): Promise<void> {
  await client.delete(`/api/chat/conversations/${id}`);
}

export async function listMessages(id: string): Promise<MessageRow[]> {
  const res = await client.get<MessageRow[]>(`/api/chat/conversations/${id}/messages`);
  return res.data;
}

/** Fetch a cited document's text for the in-app source viewer. */
export async function getDocumentSource(docId: string): Promise<DocumentSource> {
  const res = await client.get<DocumentSource>(`/api/chat/documents/${docId}/source`);
  return res.data;
}

/** Fetch a cited document's original file bytes (e.g. the source PDF). */
export async function getDocumentOriginal(docId: string): Promise<ArrayBuffer> {
  const res = await client.get<ArrayBuffer>(`/api/chat/documents/${docId}/original`, {
    responseType: 'arraybuffer',
  });
  return res.data;
}

/** Set a thumbs rating on an assistant message: 1 = up, -1 = down, 0 = clear. */
export async function setMessageFeedback(
  conversationId: string,
  messageId: string,
  feedback: number,
): Promise<void> {
  await client.post(
    `/api/chat/conversations/${conversationId}/messages/${messageId}/feedback`,
    { feedback },
  );
}

/**
 * Send a message and consume the first-party SSE protocol. Calls `onEvent` for
 * each `{type:...}` event as it arrives; resolves when the stream ends
 * (`[DONE]`). Each SSE frame is a single `data:` JSON object — comment lines
 * (keep-alive pings) and blank separators are ignored.
 */
/** Consume a first-party SSE body, dispatching each JSON event. Shared by
 * send and resume — both speak the identical protocol. */
async function consumeSse(
  res: Response,
  onEvent: (evt: StreamEvent) => void,
): Promise<void> {
  const reader = res.body?.getReader();
  if (!reader) throw new Error('No response body');
  const decoder = new TextDecoder();
  let buffer = '';
  let dataLines: string[] = [];
  const flush = () => {
    if (dataLines.length === 0) return;
    const data = dataLines.join('\n');
    dataLines = [];
    if (data === '[DONE]') return;
    try {
      onEvent(JSON.parse(data) as StreamEvent);
    } catch {
      /* ignore malformed frame */
    }
  };
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (line.startsWith('data:')) {
        dataLines.push(line.slice(5).trim());
      } else if (line === '') {
        flush();
      }
    }
  }
  flush();
}

/** Reattach to an in-flight generation (server keeps generating even with no
 * client attached). Replays buffered events (rebuilding the partial answer)
 * then follows live. Resolves 'none' when nothing is generating — the caller
 * should treat the persisted messages as final. */
export async function resumeStream(
  conversationId: string,
  onEvent: (evt: StreamEvent) => void,
  signal?: AbortSignal,
): Promise<'resumed' | 'none'> {
  const token = getToken();
  const res = await fetch(`/api/chat/conversations/${conversationId}/stream`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    signal,
  });
  if (res.status === 404) return 'none';
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  await consumeSse(res, onEvent);
  return 'resumed';
}

/** Stop the in-flight generation server-side. The partial answer is
 * persisted by the backend. A 404 (nothing running) is not an error. */
export async function cancelGeneration(conversationId: string): Promise<void> {
  const token = getToken();
  await fetch(`/api/chat/conversations/${conversationId}/cancel`, {
    method: 'POST',
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  }).catch(() => {
    /* best-effort */
  });
}

export function streamMessage(
  conversationId: string,
  content: string,
  onEvent: (evt: StreamEvent) => void,
  signal?: AbortSignal,
  attachments?: Attachment[],
  regenerate?: boolean,
  edit?: boolean,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const token = getToken();
    fetch(`/api/chat/conversations/${conversationId}/messages`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify({
        content,
        ...(attachments && attachments.length > 0 ? { attachments } : {}),
        ...(regenerate ? { regenerate: true } : {}),
        ...(edit ? { edit: true } : {}),
      }),
      signal,
    })
      .then(async (res) => {
        if (!res.ok) {
          const text = await res.text().catch(() => '');
          reject(new Error(`HTTP ${res.status}: ${text}`));
          return;
        }
        await consumeSse(res, onEvent);
        resolve();
      })
      .catch(reject);
  });
}
