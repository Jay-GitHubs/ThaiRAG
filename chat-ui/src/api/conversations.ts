import client, { getToken } from './client';
import type {
  Attachment,
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

export async function createConversation(
  title?: string,
  workspaceScope?: string | null,
): Promise<Conversation> {
  const res = await client.post<Conversation>('/api/chat/conversations', {
    title: title ?? '',
    ...(workspaceScope ? { workspace_scope: workspaceScope } : {}),
  });
  return res.data;
}

export async function renameConversation(id: string, title: string): Promise<Conversation> {
  const res = await client.patch<Conversation>(`/api/chat/conversations/${id}`, { title });
  return res.data;
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
export function streamMessage(
  conversationId: string,
  content: string,
  onEvent: (evt: StreamEvent) => void,
  signal?: AbortSignal,
  attachments?: Attachment[],
  regenerate?: boolean,
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
      }),
      signal,
    })
      .then(async (res) => {
        if (!res.ok) {
          const text = await res.text().catch(() => '');
          reject(new Error(`HTTP ${res.status}: ${text}`));
          return;
        }
        const reader = res.body?.getReader();
        if (!reader) {
          reject(new Error('No response body'));
          return;
        }
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
            // `:`-prefixed comment lines (keep-alive) and others are ignored.
          }
        }
        flush();
        resolve();
      })
      .catch(reject);
  });
}
