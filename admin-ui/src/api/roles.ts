import client from './client';

export interface RolePermission {
  resource: string;
  actions: string[];
}

export interface CustomRole {
  id: string;
  name: string;
  description: string;
  permissions: RolePermission[];
  is_system: boolean;
  created_at: string;
}

interface ListResponse<T> {
  data: T[];
  total: number;
}

export async function listRoles(): Promise<CustomRole[]> {
  const res = await client.get<ListResponse<CustomRole>>('/api/km/roles');
  return res.data.data;
}

export async function createRole(
  role: Omit<CustomRole, 'id' | 'created_at' | 'is_system'>,
): Promise<CustomRole> {
  const res = await client.post<CustomRole>('/api/km/roles', role);
  return res.data;
}

export async function getRole(id: string): Promise<CustomRole> {
  const res = await client.get<CustomRole>(`/api/km/roles/${id}`);
  return res.data;
}

export async function updateRole(id: string, role: Partial<CustomRole>): Promise<void> {
  await client.put(`/api/km/roles/${id}`, role);
}

export async function deleteRole(id: string): Promise<void> {
  await client.delete(`/api/km/roles/${id}`);
}
