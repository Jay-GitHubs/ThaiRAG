import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { createOrg, deleteOrg, listOrgs } from '../api/km';

export function useOrgs() {
  return useQuery({ queryKey: ['orgs'], queryFn: () => listOrgs() });
}

export function useCreateOrg() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => createOrg(name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['orgs'] }),
  });
}

export function useDeleteOrg() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (orgId: string) => deleteOrg(orgId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['orgs'] }),
  });
}
