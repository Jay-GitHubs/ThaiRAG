import { useEffect, useRef } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { cancelJob, listJobs } from '../api/jobs';
import { getToken } from '../api/client';
import type { JobListResponse } from '../api/types';

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

/**
 * SSE-based job streaming hook with automatic polling fallback.
 * Uses EventSource to receive real-time job updates via SSE.
 * Falls back to polling (useJobs) if the SSE connection fails.
 */
export function useJobsStream(workspaceId: string | undefined) {
  const qc = useQueryClient();
  const esRef = useRef<EventSource | null>(null);
  const fallbackRef = useRef(false);

  // Always set up the polling query (used as fallback and for initial data)
  const pollQuery = useJobs(workspaceId);

  useEffect(() => {
    if (!workspaceId) return;

    // Build SSE URL — token goes as query param since EventSource
    // doesn't support custom headers
    const token = getToken();
    const params = token ? `?token=${encodeURIComponent(token)}` : '';
    const url = `/api/km/workspaces/${workspaceId}/jobs/stream${params}`;

    let es: EventSource;
    try {
      es = new EventSource(url);
    } catch {
      // EventSource not available — stay on polling
      fallbackRef.current = true;
      return;
    }
    esRef.current = es;

    es.addEventListener('jobs', (event: MessageEvent) => {
      try {
        const data = JSON.parse(event.data) as JobListResponse;
        // Write directly into the react-query cache so JobsTable updates
        qc.setQueryData(['jobs', workspaceId], data);
      } catch {
        // Ignore malformed events
      }
    });

    es.onerror = () => {
      // On error, close SSE and let polling take over
      es.close();
      esRef.current = null;
      fallbackRef.current = true;
    };

    return () => {
      es.close();
      esRef.current = null;
    };
  }, [workspaceId, qc]);

  return pollQuery;
}

export function useCancelJob() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ wsId, jobId }: { wsId: string; jobId: string }) =>
      cancelJob(wsId, jobId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }),
  });
}
