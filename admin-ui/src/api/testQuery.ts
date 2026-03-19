import client from './client';
import type { TestQueryResponse } from './types';

export async function testQuery(workspaceId: string, query: string, timeoutMs?: number) {
  const res = await client.post<TestQueryResponse>(
    `/api/km/workspaces/${workspaceId}/test-query`,
    { query },
    timeoutMs ? { timeout: timeoutMs } : undefined,
  );
  return res.data;
}
