import client from './client';

export interface Tenant {
  id: string;
  name: string;
  plan: string;
  is_active: boolean;
  created_at: string;
}

export interface TenantQuota {
  max_documents: number;
  max_storage_bytes: number;
  max_queries_per_day: number;
  max_users: number;
  max_workspaces: number;
}

export interface TenantUsage {
  current_documents: number;
  current_storage_bytes: number;
  queries_today: number;
  current_users: number;
  current_workspaces: number;
}

interface ListResponse<T> {
  data: T[];
  total: number;
}

export async function listTenants(): Promise<Tenant[]> {
  const res = await client.get<ListResponse<Tenant>>('/api/km/tenants');
  return res.data.data;
}

export async function createTenant(name: string, plan: string): Promise<Tenant> {
  const res = await client.post<Tenant>('/api/km/tenants', { name, plan });
  return res.data;
}

export async function getTenant(id: string): Promise<Tenant> {
  const res = await client.get<Tenant>(`/api/km/tenants/${id}`);
  return res.data;
}

export async function updateTenant(id: string, name: string, plan: string): Promise<Tenant> {
  const res = await client.put<Tenant>(`/api/km/tenants/${id}`, { name, plan });
  return res.data;
}

export async function deleteTenant(id: string): Promise<void> {
  await client.delete(`/api/km/tenants/${id}`);
}

export async function getTenantQuota(id: string): Promise<TenantQuota> {
  const res = await client.get<TenantQuota>(`/api/km/tenants/${id}/quota`);
  return res.data;
}

export async function setTenantQuota(id: string, quota: TenantQuota): Promise<void> {
  await client.put(`/api/km/tenants/${id}/quota`, quota);
}

export async function getTenantUsage(id: string): Promise<TenantUsage> {
  const res = await client.get<TenantUsage>(`/api/km/tenants/${id}/usage`);
  return res.data;
}

export async function assignOrg(tenantId: string, orgId: string): Promise<void> {
  await client.post(`/api/km/tenants/${tenantId}/assign-org`, { org_id: orgId });
}
