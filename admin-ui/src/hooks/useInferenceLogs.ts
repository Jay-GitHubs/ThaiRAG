import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  listInferenceLogs,
  getInferenceAnalytics,
  deleteInferenceLogs,
  exportInferenceLogs,
} from '../api/inferenceLogs';
import type { InferenceLogFilter } from '../api/types';

export function useInferenceLogs(filter: InferenceLogFilter) {
  return useQuery({
    queryKey: ['inference-logs', filter],
    queryFn: () => listInferenceLogs(filter),
  });
}

export function useInferenceAnalytics(filter?: Partial<InferenceLogFilter>) {
  return useQuery({
    queryKey: ['inference-analytics', filter],
    queryFn: () => getInferenceAnalytics(filter),
  });
}

export function useDeleteInferenceLogs() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: deleteInferenceLogs,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['inference-logs'] });
      qc.invalidateQueries({ queryKey: ['inference-analytics'] });
    },
  });
}

export function useExportInferenceLogs() {
  return useMutation({ mutationFn: exportInferenceLogs });
}
