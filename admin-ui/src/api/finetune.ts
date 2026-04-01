import client from './client';

// ── Types ────────────────────────────────────────────────────────────

export interface TrainingDataset {
  id: string;
  name: string;
  description: string;
  pair_count: number;
  created_at: string;
}

export interface TrainingPair {
  id: string;
  dataset_id: string;
  query: string;
  positive_doc: string;
  negative_doc?: string;
  created_at: string;
}

export interface TrainingConfig {
  epochs: number;
  learning_rate: number;
  lora_rank: number;
  lora_alpha: number;
  batch_size: number;
  warmup_ratio: number;
  max_seq_length: number;
  quantization: string;
  preset?: string;
}

export interface FinetuneJob {
  id: string;
  dataset_id: string;
  base_model: string;
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';
  metrics?: string;
  output_model_path?: string;
  config?: string;
  created_at: string;
  updated_at: string;
}

export interface ListResponse<T> {
  data: T[];
  total: number;
}

export interface CreateDatasetRequest {
  name: string;
  description?: string;
}

export interface AddPairRequest {
  query: string;
  positive_doc: string;
  negative_doc?: string;
}

export interface CreateJobRequest {
  dataset_id: string;
  base_model: string;
  model_source?: 'ollama' | 'huggingface';
  config?: TrainingConfig;
}

// ── Dataset API ──────────────────────────────────────────────────────

export async function listDatasets(): Promise<ListResponse<TrainingDataset>> {
  const res = await client.get('/api/km/finetune/datasets');
  return res.data;
}

export async function createDataset(
  req: CreateDatasetRequest,
): Promise<TrainingDataset> {
  const res = await client.post('/api/km/finetune/datasets', req);
  return res.data;
}

export async function getDataset(id: string): Promise<TrainingDataset> {
  const res = await client.get(`/api/km/finetune/datasets/${id}`);
  return res.data;
}

export async function deleteDataset(id: string): Promise<void> {
  await client.delete(`/api/km/finetune/datasets/${id}`);
}

// ── Pairs API ────────────────────────────────────────────────────────

export async function listPairs(
  datasetId: string,
): Promise<ListResponse<TrainingPair>> {
  const res = await client.get(`/api/km/finetune/datasets/${datasetId}/pairs`);
  return res.data;
}

export async function addPair(
  datasetId: string,
  req: AddPairRequest,
): Promise<TrainingPair> {
  const res = await client.post(
    `/api/km/finetune/datasets/${datasetId}/pairs`,
    req,
  );
  return res.data;
}

export async function deletePair(
  datasetId: string,
  pairId: string,
): Promise<void> {
  await client.delete(`/api/km/finetune/datasets/${datasetId}/pairs/${pairId}`);
}

// ── Jobs API ─────────────────────────────────────────────────────────

export async function listJobs(): Promise<ListResponse<FinetuneJob>> {
  const res = await client.get('/api/km/finetune/jobs');
  return res.data;
}

export async function createJob(req: CreateJobRequest): Promise<FinetuneJob> {
  const res = await client.post('/api/km/finetune/jobs', req);
  return res.data;
}

export async function getJob(id: string): Promise<FinetuneJob> {
  const res = await client.get(`/api/km/finetune/jobs/${id}`);
  return res.data;
}

export async function startJob(id: string): Promise<{ status: string; job_id: string }> {
  const res = await client.post(`/api/km/finetune/jobs/${id}/start`);
  return res.data;
}

export async function cancelJob(id: string): Promise<{ status: string; job_id: string }> {
  const res = await client.post(`/api/km/finetune/jobs/${id}/cancel`);
  return res.data;
}

export async function getJobLogs(id: string): Promise<{ job_id: string; lines: string[] }> {
  const res = await client.get(`/api/km/finetune/jobs/${id}/logs`);
  return res.data;
}

export async function deleteJob(id: string): Promise<void> {
  await client.delete(`/api/km/finetune/jobs/${id}`);
}

// ── Ollama Models ────────────────────────────────────────────────────

export interface OllamaModel {
  name: string;
  size?: number;
  modified_at?: string;
}

export async function listOllamaModels(): Promise<OllamaModel[]> {
  try {
    const res = await client.get('/api/km/settings/ollama/models');
    return res.data?.models ?? res.data ?? [];
  } catch {
    return [];
  }
}

// ── Import Feedback ─────────────────────────────────────────────────

export interface ImportFeedbackRequest {
  source: 'positive_feedback' | 'golden_examples' | 'both';
  min_score?: number;
  workspace_id?: string;
}

export interface ImportFeedbackResponse {
  imported: number;
  skipped_duplicates: number;
}

export async function importFeedback(
  datasetId: string,
  req: ImportFeedbackRequest,
): Promise<ImportFeedbackResponse> {
  const res = await client.post(
    `/api/km/finetune/datasets/${datasetId}/import-feedback`,
    req,
  );
  return res.data;
}

// ── Export ───────────────────────────────────────────────────────────

export function getExportUrl(
  datasetId: string,
  format: 'openai' | 'alpaca',
): string {
  return `/api/km/finetune/datasets/${datasetId}/export?format=${format}`;
}
