import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  deleteDocument,
  ingestDocument,
  listDocuments,
  uploadDocument,
} from '../api/documents';
import type { IngestRequest } from '../api/types';

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
    mutationFn: ({ wsId, file, title }: { wsId: string; file: File; title?: string }) =>
      uploadDocument(wsId, file, title),
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
