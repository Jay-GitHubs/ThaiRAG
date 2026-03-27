import client from './client';
import type { ListResponse, PaginationParams, User, UserRole } from './types';

export async function listUsers(params?: PaginationParams) {
  const res = await client.get<ListResponse<User>>('/api/km/users', { params });
  return res.data;
}

export async function deleteUser(id: string) {
  await client.delete(`/api/km/users/${id}`);
}

export async function updateUserRole(id: string, role: UserRole) {
  const res = await client.put<User>(`/api/km/users/${id}/role`, { role });
  return res.data;
}

export async function updateUserStatus(id: string, disabled: boolean) {
  const res = await client.put<User>(`/api/km/users/${id}/status`, { disabled });
  return res.data;
}
