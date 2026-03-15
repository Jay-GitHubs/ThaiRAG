import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  createIdentityProvider,
  deleteIdentityProvider,
  getProviderConfig,
  listAvailableModels,
  listIdentityProviders,
  testIdpConnection,
  updateIdentityProvider,
  updateProviderConfig,
} from '../api/settings';
import type { CreateIdpRequest, UpdateIdpRequest, UpdateProviderConfigRequest } from '../api/types';

export function useIdentityProviders() {
  return useQuery({
    queryKey: ['identity-providers'],
    queryFn: () => listIdentityProviders(),
  });
}

export function useCreateIdp() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateIdpRequest) => createIdentityProvider(data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['identity-providers'] }),
  });
}

export function useUpdateIdp() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateIdpRequest }) =>
      updateIdentityProvider(id, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['identity-providers'] }),
  });
}

export function useDeleteIdp() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteIdentityProvider(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['identity-providers'] }),
  });
}

export function useTestIdpConnection() {
  return useMutation({
    mutationFn: (id: string) => testIdpConnection(id),
  });
}

export function useProviderConfig() {
  return useQuery({
    queryKey: ['provider-config'],
    queryFn: () => getProviderConfig(),
  });
}

export function useUpdateProviderConfig() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: UpdateProviderConfigRequest) => updateProviderConfig(data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['provider-config'] });
      qc.invalidateQueries({ queryKey: ['available-models'] });
    },
  });
}

export function useAvailableModels() {
  return useQuery({
    queryKey: ['available-models'],
    queryFn: () => listAvailableModels(),
  });
}
