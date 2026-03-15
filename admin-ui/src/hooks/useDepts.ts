import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { createDept, deleteDept, listDepts } from '../api/km';

export function useDepts(orgId: string | undefined) {
  return useQuery({
    queryKey: ['depts', orgId],
    queryFn: () => listDepts(orgId!),
    enabled: !!orgId,
  });
}

export function useCreateDept() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, name }: { orgId: string; name: string }) => createDept(orgId, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['depts'] }),
  });
}

export function useDeleteDept() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, deptId }: { orgId: string; deptId: string }) =>
      deleteDept(orgId, deptId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['depts'] }),
  });
}
