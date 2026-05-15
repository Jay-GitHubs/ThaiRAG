import client from './client';
import { getToken } from './client';
import type { TestQueryResponse, PipelineProgress } from './types';
import type { Attachment } from './attachments';

export async function testQuery(
  workspaceId: string,
  query: string,
  timeoutMs?: number,
  attachments?: Attachment[],
) {
  const res = await client.post<TestQueryResponse>(
    `/api/km/workspaces/${workspaceId}/test-query`,
    { query, ...(attachments && attachments.length > 0 ? { attachments } : {}) },
    timeoutMs ? { timeout: timeoutMs } : undefined,
  );
  return res.data;
}

/**
 * Stream a test query via SSE. Calls `onProgress` for each pipeline stage event
 * in real-time, then resolves with the final TestQueryResponse.
 */
export function testQueryStream(
  workspaceId: string,
  query: string,
  onProgress: (evt: PipelineProgress) => void,
  signal?: AbortSignal,
  attachments?: Attachment[],
): Promise<TestQueryResponse> {
  return new Promise((resolve, reject) => {
    const token = getToken();

    fetch(`/api/km/workspaces/${workspaceId}/test-query-stream`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify({
        query,
        ...(attachments && attachments.length > 0 ? { attachments } : {}),
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
        let eventType = '';
        let dataLines: string[] = [];

        // eslint-disable-next-line no-constant-condition
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop() ?? '';

          for (const line of lines) {
            if (line.startsWith('event:')) {
              eventType = line.slice(6).trim();
            } else if (line.startsWith('data:')) {
              dataLines.push(line.slice(5).trim());
            } else if (line === '') {
              // End of event
              const data = dataLines.join('\n');
              dataLines = [];

              if (data === '[DONE]') {
                // Stream complete
              } else if (eventType === 'progress') {
                try {
                  const progress: PipelineProgress = JSON.parse(data);
                  onProgress(progress);
                } catch { /* ignore parse errors */ }
              } else if (eventType === 'result') {
                try {
                  const result: TestQueryResponse = JSON.parse(data);
                  resolve(result);
                } catch (e) {
                  reject(new Error(`Failed to parse result: ${e}`));
                }
              } else if (eventType === 'error') {
                try {
                  const err = JSON.parse(data);
                  reject(new Error(err.error ?? 'Pipeline error'));
                } catch {
                  reject(new Error(data));
                }
              }

              eventType = '';
            }
          }
        }
      })
      .catch(reject);
  });
}
