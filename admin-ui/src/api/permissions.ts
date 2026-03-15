import client from './client';
import type {
  GrantPermissionRequest,
  ListResponse,
  PaginationParams,
  PermissionResponse,
  RevokePermissionRequest,
  ScopedGrantRequest,
  ScopedRevokeRequest,
} from './types';

// ── Org-level permissions ──────────────────────────────────────────
export async function listPermissions(orgId: string, params?: PaginationParams) {
  const res = await client.get<ListResponse<PermissionResponse>>(
    `/api/km/orgs/${orgId}/permissions`,
    { params },
  );
  return res.data;
}

export async function grantPermission(orgId: string, data: GrantPermissionRequest) {
  await client.post(`/api/km/orgs/${orgId}/permissions`, data);
}

export async function revokePermission(orgId: string, data: RevokePermissionRequest) {
  await client.delete(`/api/km/orgs/${orgId}/permissions`, { data });
}

// ── Dept-level permissions ─────────────────────────────────────────
export async function listDeptPermissions(
  orgId: string,
  deptId: string,
  params?: PaginationParams,
) {
  const res = await client.get<ListResponse<PermissionResponse>>(
    `/api/km/orgs/${orgId}/depts/${deptId}/permissions`,
    { params },
  );
  return res.data;
}

export async function grantDeptPermission(orgId: string, deptId: string, data: ScopedGrantRequest) {
  await client.post(`/api/km/orgs/${orgId}/depts/${deptId}/permissions`, data);
}

export async function revokeDeptPermission(
  orgId: string,
  deptId: string,
  data: ScopedRevokeRequest,
) {
  await client.delete(`/api/km/orgs/${orgId}/depts/${deptId}/permissions`, { data });
}

// ── Workspace-level permissions ────────────────────────────────────
export async function listWorkspacePermissions(
  orgId: string,
  deptId: string,
  wsId: string,
  params?: PaginationParams,
) {
  const res = await client.get<ListResponse<PermissionResponse>>(
    `/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}/permissions`,
    { params },
  );
  return res.data;
}

export async function grantWorkspacePermission(
  orgId: string,
  deptId: string,
  wsId: string,
  data: ScopedGrantRequest,
) {
  await client.post(
    `/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}/permissions`,
    data,
  );
}

export async function revokeWorkspacePermission(
  orgId: string,
  deptId: string,
  wsId: string,
  data: ScopedRevokeRequest,
) {
  await client.delete(
    `/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}/permissions`,
    { data },
  );
}
