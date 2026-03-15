import { useQuery } from '@tanstack/react-query';
import { listEnabledProviders } from '../api/settings';

export function useEnabledProviders() {
  return useQuery({
    queryKey: ['enabled-providers'],
    queryFn: () => listEnabledProviders(),
  });
}
