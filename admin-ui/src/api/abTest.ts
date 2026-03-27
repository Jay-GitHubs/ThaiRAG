import client from './client';

// ── Types ───────────────────────────────────────────────────────────

export interface SearchOverrides {
  top_k?: number;
  rerank_top_k?: number;
  vector_weight?: number;
  text_weight?: number;
}

export interface AbVariant {
  name: string;
  search_config?: SearchOverrides;
  llm_model?: string;
  prompt_template?: string;
}

export interface AbMetrics {
  avg_latency_ms: number;
  avg_relevance_score: number;
  total_queries: number;
  avg_token_count: number;
}

export interface AbQueryVariantResult {
  answer: string;
  latency_ms: number;
  token_count: number;
  relevance_score: number;
  chunks_retrieved: number;
}

export interface AbQueryResult {
  query: string;
  variant_a: AbQueryVariantResult;
  variant_b: AbQueryVariantResult;
}

export interface AbTestResults {
  variant_a_metrics: AbMetrics;
  variant_b_metrics: AbMetrics;
  winner?: string;
  per_query: AbQueryResult[];
}

export type AbTestStatus = 'draft' | 'running' | 'completed';

export interface AbTest {
  id: string;
  name: string;
  description: string;
  variant_a: AbVariant;
  variant_b: AbVariant;
  status: AbTestStatus;
  created_at: string;
  completed_at?: string;
  results?: AbTestResults;
}

export interface CreateAbTestRequest {
  name: string;
  description?: string;
  variant_a: AbVariant;
  variant_b: AbVariant;
}

export interface RunAbTestRequest {
  queries: string[];
}

export interface CompareRequest {
  query: string;
}

// ── API Functions ───────────────────────────────────────────────────

export async function listAbTests(): Promise<AbTest[]> {
  const res = await client.get<AbTest[]>('/api/km/ab-tests');
  return res.data;
}

export async function getAbTest(id: string): Promise<AbTest> {
  const res = await client.get<AbTest>(`/api/km/ab-tests/${id}`);
  return res.data;
}

export async function createAbTest(data: CreateAbTestRequest): Promise<AbTest> {
  const res = await client.post<AbTest>('/api/km/ab-tests', data);
  return res.data;
}

export async function deleteAbTest(id: string): Promise<void> {
  await client.delete(`/api/km/ab-tests/${id}`);
}

export async function runAbTest(id: string, queries: string[]): Promise<AbTest> {
  const res = await client.post<AbTest>(`/api/km/ab-tests/${id}/run`, { queries }, { timeout: 300000 });
  return res.data;
}

export async function compareAbTest(id: string, query: string): Promise<AbQueryResult> {
  const res = await client.post<AbQueryResult>(`/api/km/ab-tests/${id}/compare`, { query }, { timeout: 300000 });
  return res.data;
}
