import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  createIdentityProvider,
  createLlmProfile,
  createVaultKey,
  deleteIdentityProvider,
  deleteLlmProfile,
  deleteVaultKey,
  getProviderConfig,
  listAvailableModels,
  listIdentityProviders,
  listLlmProfiles,
  listVaultKeys,
  testIdpConnection,
  testVaultKey,
  updateIdentityProvider,
  updateLlmProfile,
  updateProviderConfig,
  updateVaultKey,
} from '../api/settings';
import type {
  CreateIdpRequest,
  CreateLlmProfileRequest,
  CreateVaultKeyRequest,
  UpdateIdpRequest,
  UpdateLlmProfileRequest,
  UpdateProviderConfigRequest,
  UpdateVaultKeyRequest,
} from '../api/types';

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

// ── API Key Vault ──────────────────────────────────────────────────

export function useVaultKeys() {
  return useQuery({
    queryKey: ['vault-keys'],
    queryFn: () => listVaultKeys(),
  });
}

export function useCreateVaultKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateVaultKeyRequest) => createVaultKey(data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['vault-keys'] }),
  });
}

export function useUpdateVaultKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateVaultKeyRequest }) =>
      updateVaultKey(id, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['vault-keys'] }),
  });
}

export function useDeleteVaultKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteVaultKey(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['vault-keys'] }),
  });
}

export function useTestVaultKey() {
  return useMutation({
    mutationFn: (id: string) => testVaultKey(id),
  });
}

// ── LLM Profiles ──────────────────────────────────────────────────

export function useLlmProfiles() {
  return useQuery({
    queryKey: ['llm-profiles'],
    queryFn: () => listLlmProfiles(),
  });
}

export function useCreateLlmProfile() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateLlmProfileRequest) => createLlmProfile(data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['llm-profiles'] }),
  });
}

export function useUpdateLlmProfile() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateLlmProfileRequest }) =>
      updateLlmProfile(id, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['llm-profiles'] }),
  });
}

export function useDeleteLlmProfile() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteLlmProfile(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['llm-profiles'] }),
  });
}
