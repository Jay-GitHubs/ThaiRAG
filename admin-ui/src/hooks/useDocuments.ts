import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  deleteDocument,
  getDocument,
  ingestDocument,
  listDocuments,
  reprocessAllDocuments,
  reprocessDocument,
  uploadDocument,
} from '../api/documents';
import type { DocumentHandling, IngestRequest } from '../api/types';

export function useDocuments(workspaceId: string | undefined) {
  const query = useQuery({
    queryKey: ['documents', workspaceId],
    queryFn: () => listDocuments(workspaceId!),
    enabled: !!workspaceId,
    // Auto-poll every 3s when any document is still processing
    refetchInterval: (query) => {
      const hasProcessing = query.state.data?.data?.some(
        (d: { status: string }) => d.status === 'processing',
      );
      return hasProcessing ? 3000 : false;
    },
  });
  return query;
}

/**
 * Poll a single document while it's processing — backs the live upload tracker.
 * Refetches every 1.5s until the document reaches a terminal (ready/failed)
 * state so the per-stage timeline updates in near-real-time. `enabled` lets the
 * caller switch polling on only while the tracker is visible.
 */
export function useDocument(
  workspaceId: string | undefined,
  docId: string | undefined,
  enabled = true,
) {
  return useQuery({
    queryKey: ['document', workspaceId, docId],
    queryFn: () => getDocument(workspaceId!, docId!),
    enabled: enabled && !!workspaceId && !!docId,
    refetchInterval: (query) =>
      query.state.data?.status === 'processing' ? 1500 : false,
  });
}

export function useIngestDocument() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ wsId, data }: { wsId: string; data: IngestRequest }) =>
      ingestDocument(wsId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  });
}

export function useUploadDocument() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      wsId,
      file,
      title,
      handling,
    }: {
      wsId: string;
      file: File;
      title?: string;
      handling?: DocumentHandling;
    }) => uploadDocument(wsId, file, title, handling),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  });
}

export function useDeleteDocument() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ wsId, docId }: { wsId: string; docId: string }) =>
      deleteDocument(wsId, docId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  });
}

export function useReprocessDocument() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ wsId, docId }: { wsId: string; docId: string }) =>
      reprocessDocument(wsId, docId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  });
}

export function useReprocessAllDocuments() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ wsId }: { wsId: string }) => reprocessAllDocuments(wsId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  });
}
