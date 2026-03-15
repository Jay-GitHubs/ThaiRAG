import { useQuery } from '@tanstack/react-query';
import { getHealth, getMetrics } from '../api/health';

export function useHealth(deep = false) {
  return useQuery({
    queryKey: ['health', deep],
    queryFn: () => getHealth(deep),
  });
}

export function useMetrics(enabled = true) {
  return useQuery({
    queryKey: ['metrics'],
    queryFn: getMetrics,
    enabled,
    refetchInterval: enabled ? 30_000 : false,
  });
}
