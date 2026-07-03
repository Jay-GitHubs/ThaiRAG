import client from './client';
import type {
  LoginRequest,
  LoginResponse,
  ProviderInfo,
  RegisterRequest,
  User,
} from './types';

export async function listProviders(): Promise<ProviderInfo[]> {
  const res = await client.get<ProviderInfo[]>('/api/auth/providers');
  return res.data;
}

export async function login(data: LoginRequest): Promise<LoginResponse> {
  const res = await client.post<LoginResponse>('/api/auth/login', data);
  return res.data;
}

export async function register(data: RegisterRequest): Promise<User> {
  const res = await client.post<User>('/api/auth/register', data);
  return res.data;
}

export async function changePassword(currentPassword: string, newPassword: string): Promise<void> {
  await client.post('/api/auth/change-password', {
    current_password: currentPassword,
    new_password: newPassword,
  });
}
