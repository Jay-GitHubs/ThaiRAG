import client from './client';

export interface DocumentComment {
  id: string;
  doc_id: string;
  user_id: string;
  user_name?: string;
  text: string;
  parent_id?: string;
  created_at: string;
}

export interface DocumentAnnotation {
  id: string;
  doc_id: string;
  user_id: string;
  user_name?: string;
  chunk_id?: string;
  text: string;
  highlight_start?: number;
  highlight_end?: number;
  created_at: string;
}

export interface DocumentReview {
  id: string;
  doc_id: string;
  reviewer_id: string;
  reviewer_name?: string;
  status: string;
  comments?: string;
  created_at: string;
  updated_at: string;
}

interface ListResponse<T> {
  data: T[];
  total: number;
}

// ── Comments ─────────────────────────────────────────────────────────

export async function listComments(wsId: string, docId: string): Promise<DocumentComment[]> {
  const res = await client.get<ListResponse<DocumentComment>>(
    `/api/km/workspaces/${wsId}/documents/${docId}/comments`,
  );
  return res.data.data;
}

export async function createComment(
  wsId: string,
  docId: string,
  text: string,
  parentId?: string,
): Promise<DocumentComment> {
  const res = await client.post<DocumentComment>(
    `/api/km/workspaces/${wsId}/documents/${docId}/comments`,
    { user_id: 'admin', text, parent_id: parentId },
  );
  return res.data;
}

export async function deleteComment(
  wsId: string,
  docId: string,
  commentId: string,
): Promise<void> {
  await client.delete(
    `/api/km/workspaces/${wsId}/documents/${docId}/comments/${commentId}`,
  );
}

// ── Annotations ───────────────────────────────────────────────────────

export async function listAnnotations(
  wsId: string,
  docId: string,
): Promise<DocumentAnnotation[]> {
  const res = await client.get<ListResponse<DocumentAnnotation>>(
    `/api/km/workspaces/${wsId}/documents/${docId}/annotations`,
  );
  return res.data.data;
}

export async function createAnnotation(
  wsId: string,
  docId: string,
  data: { text: string; chunk_id?: string; highlight_start?: number; highlight_end?: number },
): Promise<DocumentAnnotation> {
  const res = await client.post<DocumentAnnotation>(
    `/api/km/workspaces/${wsId}/documents/${docId}/annotations`,
    { user_id: 'admin', ...data },
  );
  return res.data;
}

export async function deleteAnnotation(
  wsId: string,
  docId: string,
  annotationId: string,
): Promise<void> {
  await client.delete(
    `/api/km/workspaces/${wsId}/documents/${docId}/annotations/${annotationId}`,
  );
}

// ── Reviews ───────────────────────────────────────────────────────────

export async function listReviews(wsId: string, docId: string): Promise<DocumentReview[]> {
  const res = await client.get<ListResponse<DocumentReview>>(
    `/api/km/workspaces/${wsId}/documents/${docId}/reviews`,
  );
  return res.data.data;
}

export async function createReview(
  wsId: string,
  docId: string,
  status: string,
  comments?: string,
): Promise<DocumentReview> {
  const res = await client.post<DocumentReview>(
    `/api/km/workspaces/${wsId}/documents/${docId}/reviews`,
    { reviewer_id: 'admin', status, comments },
  );
  return res.data;
}

export async function updateReviewStatus(
  wsId: string,
  docId: string,
  reviewId: string,
  status: string,
  comments?: string,
): Promise<void> {
  await client.put(
    `/api/km/workspaces/${wsId}/documents/${docId}/reviews/${reviewId}`,
    { status, comments },
  );
}
