import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  grantPermission,
  listPermissions,
  revokePermission,
} from '../api/permissions';
import type { GrantPermissionRequest, RevokePermissionRequest } from '../api/types';

export function usePermissions(orgId: string | undefined) {
  return useQuery({
    queryKey: ['permissions', orgId],
    queryFn: () => listPermissions(orgId!),
    enabled: !!orgId,
  });
}

export function useGrantPermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, data }: { orgId: string; data: GrantPermissionRequest }) =>
      grantPermission(orgId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}

export function useRevokePermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, data }: { orgId: string; data: RevokePermissionRequest }) =>
      revokePermission(orgId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}
