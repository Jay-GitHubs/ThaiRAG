import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { cancelJob, listJobs } from '../api/jobs';

export function useJobs(workspaceId: string | undefined) {
  return useQuery({
    queryKey: ['jobs', workspaceId],
    queryFn: () => listJobs(workspaceId!),
    enabled: !!workspaceId,
    // Poll every 3s when there are active jobs
    refetchInterval: (query) => {
      const hasActive = query.state.data?.jobs?.some(
        (j) => j.status === 'queued' || j.status === 'running',
      );
      return hasActive ? 3000 : false;
    },
  });
}

export function useCancelJob() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ wsId, jobId }: { wsId: string; jobId: string }) =>
      cancelJob(wsId, jobId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }),
  });
}
