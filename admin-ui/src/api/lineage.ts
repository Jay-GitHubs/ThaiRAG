import client from './client';

export interface LineageRecord {
  response_id: string;
  query: string;
  chunk_id: string;
  doc_title: string;
  doc_id: string;
  score: number;
  rank: number;
  contributed: boolean;
  timestamp?: string;
}

export async function getLineageByResponse(response_id: string): Promise<LineageRecord[]> {
  const res = await client.get<LineageRecord[]>(`/api/km/lineage/response/${encodeURIComponent(response_id)}`);
  return res.data;
}

export async function getLineageByDocument(doc_id: string, limit = 50): Promise<LineageRecord[]> {
  const res = await client.get<LineageRecord[]>(`/api/km/lineage/document/${encodeURIComponent(doc_id)}`, { params: { limit } });
  return res.data;
}
