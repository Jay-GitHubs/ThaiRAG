import client from './client';
import type { LoginRequest, LoginResponse, RegisterRequest, User } from './types';

export async function login(data: LoginRequest): Promise<LoginResponse> {
  const res = await client.post<LoginResponse>('/api/auth/login', data);
  return res.data;
}

export async function register(data: RegisterRequest): Promise<User> {
  const res = await client.post<User>('/api/auth/register', data);
  return res.data;
}
