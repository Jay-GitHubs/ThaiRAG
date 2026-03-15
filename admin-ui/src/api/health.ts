import client from './client';
import type { HealthResponse } from './types';

export async function getHealth(deep = false) {
  const params = deep ? { deep: 'true' } : {};
  const res = await client.get<HealthResponse>('/health', { params });
  return res.data;
}

export async function getMetrics(): Promise<string> {
  const res = await client.get<string>('/metrics', {
    headers: { Accept: 'text/plain' },
    transformResponse: [(data) => data],
  });
  return res.data;
}
