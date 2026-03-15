import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { createWorkspace, deleteWorkspace, listWorkspaces } from '../api/km';

export function useWorkspaces(orgId: string | undefined, deptId: string | undefined) {
  return useQuery({
    queryKey: ['workspaces', orgId, deptId],
    queryFn: () => listWorkspaces(orgId!, deptId!),
    enabled: !!orgId && !!deptId,
  });
}

export function useCreateWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, deptId, name }: { orgId: string; deptId: string; name: string }) =>
      createWorkspace(orgId, deptId, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['workspaces'] }),
  });
}

export function useDeleteWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, deptId, wsId }: { orgId: string; deptId: string; wsId: string }) =>
      deleteWorkspace(orgId, deptId, wsId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['workspaces'] }),
  });
}
