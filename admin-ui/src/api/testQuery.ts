import client from './client';
import type { TestQueryResponse } from './types';

export async function testQuery(workspaceId: string, query: string) {
  const res = await client.post<TestQueryResponse>(
    `/api/km/workspaces/${workspaceId}/test-query`,
    { query },
  );
  return res.data;
}
