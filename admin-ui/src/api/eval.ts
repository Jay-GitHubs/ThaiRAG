import client from './client';

// ── Types ───────────────────────────────────────────────────────────

export interface EvalQuery {
  query: string;
  relevant_doc_ids: string[];
  relevance_scores?: number[];
}

export interface EvalQuerySet {
  id: string;
  name: string;
  queries: EvalQuery[];
  created_at: string;
}

export interface EvalMetrics {
  ndcg_at_5: number;
  ndcg_at_10: number;
  mrr: number;
  precision_at_5: number;
  precision_at_10: number;
  recall_at_10: number;
  mean_latency_ms: number;
}

export interface QueryEvalResult {
  query: string;
  ndcg_at_5: number;
  ndcg_at_10: number;
  mrr: number;
  precision: number;
  recall: number;
  latency_ms: number;
  retrieved_doc_ids: string[];
}

export interface EvalResult {
  query_set_id: string;
  run_at: string;
  metrics: EvalMetrics;
  per_query: QueryEvalResult[];
}

export interface CreateQuerySetRequest {
  name: string;
  queries: {
    query: string;
    relevant_doc_ids: string[];
    relevance_scores?: number[];
  }[];
}

export interface ImportCsvRequest {
  name: string;
  csv_data: string;
}

// ── API Functions ───────────────────────────────────────────────────

export async function listQuerySets(): Promise<EvalQuerySet[]> {
  const res = await client.get<EvalQuerySet[]>('/api/km/eval/query-sets');
  return res.data;
}

export async function getQuerySet(id: string): Promise<EvalQuerySet> {
  const res = await client.get<EvalQuerySet>(`/api/km/eval/query-sets/${id}`);
  return res.data;
}

export async function createQuerySet(data: CreateQuerySetRequest): Promise<EvalQuerySet> {
  const res = await client.post<EvalQuerySet>('/api/km/eval/query-sets', data);
  return res.data;
}

export async function deleteQuerySet(id: string): Promise<void> {
  await client.delete(`/api/km/eval/query-sets/${id}`);
}

export async function runEvaluation(id: string): Promise<EvalResult> {
  const res = await client.post<EvalResult>(`/api/km/eval/query-sets/${id}/run`);
  return res.data;
}

export async function listResults(id: string): Promise<EvalResult[]> {
  const res = await client.get<EvalResult[]>(`/api/km/eval/query-sets/${id}/results`);
  return res.data;
}

export async function importQuerySet(data: ImportCsvRequest): Promise<EvalQuerySet> {
  const res = await client.post<EvalQuerySet>('/api/km/eval/query-sets/import', data);
  return res.data;
}
