import client from './client';
import type {
  InferenceLogListResponse,
  InferenceLogFilter,
  InferenceStats,
  InferenceLogEntry,
  InferenceLogDeleteResponse,
} from './types';

export async function listInferenceLogs(filter?: InferenceLogFilter): Promise<InferenceLogListResponse> {
  const res = await client.get<InferenceLogListResponse>(
    '/api/km/settings/inference-logs',
    { params: filter },
  );
  return res.data;
}

export async function getInferenceAnalytics(filter?: Partial<InferenceLogFilter>): Promise<InferenceStats> {
  const res = await client.get<InferenceStats>(
    '/api/km/settings/inference-analytics',
    { params: filter },
  );
  return res.data;
}

export async function deleteInferenceLogs(filter?: Partial<InferenceLogFilter>): Promise<InferenceLogDeleteResponse> {
  const res = await client.delete<InferenceLogDeleteResponse>(
    '/api/km/settings/inference-logs',
    { params: filter },
  );
  return res.data;
}

export async function exportInferenceLogs(filter?: Partial<InferenceLogFilter>): Promise<InferenceLogEntry[]> {
  const res = await client.get<InferenceLogEntry[]>(
    '/api/km/settings/inference-logs/export',
    { params: filter },
  );
  return res.data;
}
