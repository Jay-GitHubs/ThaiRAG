import client from './client';
import type { ListResponse, PaginationParams, User } from './types';

export async function listUsers(params?: PaginationParams) {
  const res = await client.get<ListResponse<User>>('/api/km/users', { params });
  return res.data;
}

export async function deleteUser(id: string) {
  await client.delete(`/api/km/users/${id}`);
}
