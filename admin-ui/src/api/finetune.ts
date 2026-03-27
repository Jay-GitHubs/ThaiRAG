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

export interface FinetuneJob {
  id: string;
  dataset_id: string;
  base_model: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  metrics?: string;
  output_model_path?: string;
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
