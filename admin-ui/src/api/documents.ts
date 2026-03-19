import client from './client';
import type {
  ChunksResponse,
  Document,
  DocumentContentResponse,
  IngestRequest,
  IngestResponse,
  ListResponse,
  PaginationParams,
} from './types';

export async function listDocuments(workspaceId: string, params?: PaginationParams) {
  const res = await client.get<ListResponse<Document>>(
    `/api/km/workspaces/${workspaceId}/documents`,
    { params },
  );
  return res.data;
}

export async function getDocument(workspaceId: string, docId: string) {
  const res = await client.get<Document>(
    `/api/km/workspaces/${workspaceId}/documents/${docId}`,
  );
  return res.data;
}

export async function ingestDocument(workspaceId: string, data: IngestRequest) {
  const res = await client.post<IngestResponse>(
    `/api/km/workspaces/${workspaceId}/documents`,
    data,
  );
  return res.data;
}

export async function uploadDocument(workspaceId: string, file: File, title?: string) {
  const formData = new FormData();
  formData.append('file', file);
  if (title) formData.append('title', title);
  const res = await client.post<IngestResponse>(
    `/api/km/workspaces/${workspaceId}/documents/upload`,
    formData,
    { headers: { 'Content-Type': 'multipart/form-data' } },
  );
  return res.data;
}

export async function deleteDocument(workspaceId: string, docId: string) {
  await client.delete(`/api/km/workspaces/${workspaceId}/documents/${docId}`);
}

export async function getDocumentContent(workspaceId: string, docId: string) {
  const res = await client.get<DocumentContentResponse>(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/content`,
  );
  return res.data;
}

export async function downloadDocument(workspaceId: string, docId: string) {
  const res = await client.get(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/download`,
    { responseType: 'blob' },
  );
  return res.data as Blob;
}

export async function getDocumentChunks(workspaceId: string, docId: string) {
  const res = await client.get<ChunksResponse>(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/chunks`,
  );
  return res.data;
}

export async function reprocessDocument(workspaceId: string, docId: string) {
  const res = await client.post(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/reprocess`,
  );
  return res.data;
}

export async function reprocessAllDocuments(workspaceId: string) {
  const res = await client.post<{ queued: number; message: string }>(
    `/api/km/workspaces/${workspaceId}/documents/reprocess-all`,
  );
  return res.data;
}
