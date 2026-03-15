import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  listPermissions,
  grantPermission,
  revokePermission,
  listDeptPermissions,
  grantDeptPermission,
  revokeDeptPermission,
  listWorkspacePermissions,
  grantWorkspacePermission,
  revokeWorkspacePermission,
} from '../api/permissions';
import type {
  GrantPermissionRequest,
  RevokePermissionRequest,
  ScopedGrantRequest,
  ScopedRevokeRequest,
} from '../api/types';

// ── Org-level ────────────────────────────────────────────────────────
export function useOrgPermissions(orgId: string | undefined) {
  return useQuery({
    queryKey: ['permissions', 'org', orgId],
    queryFn: () => listPermissions(orgId!),
    enabled: !!orgId,
  });
}

export function useGrantOrgPermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, data }: { orgId: string; data: GrantPermissionRequest }) =>
      grantPermission(orgId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}

export function useRevokeOrgPermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ orgId, data }: { orgId: string; data: RevokePermissionRequest }) =>
      revokePermission(orgId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}

// ── Dept-level ───────────────────────────────────────────────────────
export function useDeptPermissions(orgId: string | undefined, deptId: string | undefined) {
  return useQuery({
    queryKey: ['permissions', 'dept', orgId, deptId],
    queryFn: () => listDeptPermissions(orgId!, deptId!),
    enabled: !!orgId && !!deptId,
  });
}

export function useGrantDeptPermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      orgId,
      deptId,
      data,
    }: {
      orgId: string;
      deptId: string;
      data: ScopedGrantRequest;
    }) => grantDeptPermission(orgId, deptId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}

export function useRevokeDeptPermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      orgId,
      deptId,
      data,
    }: {
      orgId: string;
      deptId: string;
      data: ScopedRevokeRequest;
    }) => revokeDeptPermission(orgId, deptId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}

// ── Workspace-level ──────────────────────────────────────────────────
export function useWorkspacePermissions(
  orgId: string | undefined,
  deptId: string | undefined,
  wsId: string | undefined,
) {
  return useQuery({
    queryKey: ['permissions', 'workspace', orgId, deptId, wsId],
    queryFn: () => listWorkspacePermissions(orgId!, deptId!, wsId!),
    enabled: !!orgId && !!deptId && !!wsId,
  });
}

export function useGrantWorkspacePermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      orgId,
      deptId,
      wsId,
      data,
    }: {
      orgId: string;
      deptId: string;
      wsId: string;
      data: ScopedGrantRequest;
    }) => grantWorkspacePermission(orgId, deptId, wsId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}

export function useRevokeWorkspacePermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      orgId,
      deptId,
      wsId,
      data,
    }: {
      orgId: string;
      deptId: string;
      wsId: string;
      data: ScopedRevokeRequest;
    }) => revokeWorkspacePermission(orgId, deptId, wsId, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['permissions'] }),
  });
}
