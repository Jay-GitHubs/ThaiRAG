import client from './client';
import type {
  ChunksResponse,
  Document,
  DocumentContentResponse,
  DocumentHandling,
  DocumentImageInfo,
  DocumentPreview,
  IngestRequest,
  IngestResponse,
  ListResponse,
  PaginationParams,
} from './types';

/** Dry-run: what the pipeline WOULD do for this file — no processing, no DB write. */
export async function previewDocument(workspaceId: string, file: File) {
  const formData = new FormData();
  formData.append('file', file);
  const res = await client.post<DocumentPreview>(
    `/api/km/workspaces/${workspaceId}/documents/preview`,
    formData,
    { headers: { 'Content-Type': 'multipart/form-data' } },
  );
  return res.data;
}

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

export async function uploadDocument(
  workspaceId: string,
  file: File,
  title?: string,
  handling?: DocumentHandling,
) {
  const formData = new FormData();
  formData.append('file', file);
  if (title) formData.append('title', title);
  if (handling?.handling_mode && handling.handling_mode !== 'auto') {
    formData.append('handling_mode', handling.handling_mode);
  }
  if (handling?.image_coverage_threshold != null) {
    formData.append('image_coverage_threshold', String(handling.image_coverage_threshold));
  }
  if (handling?.min_chars_per_page != null) {
    formData.append('min_chars_per_page', String(handling.min_chars_per_page));
  }
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
  // Reading persisted chunks is a fast DB lookup; cap it so a hung/overloaded
  // backend surfaces as a visible error instead of an indefinite spinner.
  const res = await client.get<ChunksResponse>(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/chunks`,
    { timeout: 15000 },
  );
  return res.data;
}

export async function listDocumentImages(workspaceId: string, docId: string) {
  const res = await client.get<DocumentImageInfo[]>(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/images`,
  );
  return res.data;
}

/**
 * Build the URL path for an extracted image blob. ACL is enforced at the API.
 * Note: the endpoint is JWT-gated, so a bare `<img src>` won't authenticate —
 * fetch the bytes with `fetchDocumentImageBlob` (which sends the Bearer header)
 * and render an object URL instead.
 */
export function documentImageUrl(workspaceId: string, docId: string, imageId: string) {
  return `/api/km/workspaces/${workspaceId}/documents/${docId}/images/${imageId}`;
}

/** Fetch an extracted image's bytes through the authed client for use as an object URL. */
export async function fetchDocumentImageBlob(
  workspaceId: string,
  docId: string,
  imageId: string,
) {
  const res = await client.get(documentImageUrl(workspaceId, docId, imageId), {
    responseType: 'blob',
  });
  return res.data as Blob;
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
