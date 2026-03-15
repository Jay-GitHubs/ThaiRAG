import client from './client';
import type {
  Department,
  ListResponse,
  Organization,
  PaginationParams,
  Workspace,
} from './types';

// ── Organizations ──────────────────────────────────────────────────
export async function listOrgs(params?: PaginationParams) {
  const res = await client.get<ListResponse<Organization>>('/api/km/orgs', { params });
  return res.data;
}

export async function getOrg(orgId: string) {
  const res = await client.get<Organization>(`/api/km/orgs/${orgId}`);
  return res.data;
}

export async function createOrg(name: string) {
  const res = await client.post<Organization>('/api/km/orgs', { name });
  return res.data;
}

export async function deleteOrg(orgId: string) {
  await client.delete(`/api/km/orgs/${orgId}`);
}

// ── Departments ────────────────────────────────────────────────────
export async function listDepts(orgId: string, params?: PaginationParams) {
  const res = await client.get<ListResponse<Department>>(`/api/km/orgs/${orgId}/depts`, { params });
  return res.data;
}

export async function getDept(orgId: string, deptId: string) {
  const res = await client.get<Department>(`/api/km/orgs/${orgId}/depts/${deptId}`);
  return res.data;
}

export async function createDept(orgId: string, name: string) {
  const res = await client.post<Department>(`/api/km/orgs/${orgId}/depts`, { name });
  return res.data;
}

export async function deleteDept(orgId: string, deptId: string) {
  await client.delete(`/api/km/orgs/${orgId}/depts/${deptId}`);
}

// ── Workspaces ─────────────────────────────────────────────────────
export async function listWorkspaces(orgId: string, deptId: string, params?: PaginationParams) {
  const res = await client.get<ListResponse<Workspace>>(
    `/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
    { params },
  );
  return res.data;
}

export async function getWorkspace(orgId: string, deptId: string, wsId: string) {
  const res = await client.get<Workspace>(
    `/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`,
  );
  return res.data;
}

export async function createWorkspace(orgId: string, deptId: string, name: string) {
  const res = await client.post<Workspace>(
    `/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
    { name },
  );
  return res.data;
}

export async function deleteWorkspace(orgId: string, deptId: string, wsId: string) {
  await client.delete(`/api/km/orgs/${orgId}/depts/${deptId}/workspaces/${wsId}`);
}
