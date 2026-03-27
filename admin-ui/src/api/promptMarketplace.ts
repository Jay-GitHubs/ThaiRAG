import client from './client';

export interface PromptTemplate {
  id: string;
  name: string;
  description: string;
  category: string;
  content: string;
  variables: string[];
  author_id: string | null;
  author_name: string | null;
  version: number;
  is_public: boolean;
  rating_avg: number;
  rating_count: number;
  created_at: string;
  updated_at: string;
}

export interface PromptTemplateFilter {
  category?: string;
  search?: string;
  is_public?: boolean;
  author_id?: string;
  limit?: number;
  offset?: number;
}

export interface CreateTemplateRequest {
  name: string;
  description?: string;
  category?: string;
  content: string;
  variables?: string[];
  is_public?: boolean;
}

export interface UpdateTemplateRequest {
  name?: string;
  description?: string;
  category?: string;
  content?: string;
  variables?: string[];
  is_public?: boolean;
}

export async function listPromptTemplates(
  filter?: PromptTemplateFilter,
): Promise<PromptTemplate[]> {
  const params: Record<string, string> = {};
  if (filter?.category) params['category'] = filter.category;
  if (filter?.search) params['search'] = filter.search;
  if (filter?.is_public !== undefined) params['is_public'] = String(filter.is_public);
  if (filter?.author_id) params['author_id'] = filter.author_id;
  if (filter?.limit !== undefined) params['limit'] = String(filter.limit);
  if (filter?.offset !== undefined) params['offset'] = String(filter.offset);
  const res = await client.get<PromptTemplate[]>('/api/km/prompts/marketplace', { params });
  return res.data;
}

export async function createPromptTemplate(req: CreateTemplateRequest): Promise<PromptTemplate> {
  const res = await client.post<PromptTemplate>('/api/km/prompts/marketplace', req);
  return res.data;
}

export async function getPromptTemplate(id: string): Promise<PromptTemplate> {
  const res = await client.get<PromptTemplate>(`/api/km/prompts/marketplace/${id}`);
  return res.data;
}

export async function updatePromptTemplate(
  id: string,
  req: UpdateTemplateRequest,
): Promise<PromptTemplate> {
  const res = await client.put<PromptTemplate>(`/api/km/prompts/marketplace/${id}`, req);
  return res.data;
}

export async function deletePromptTemplate(id: string): Promise<void> {
  await client.delete(`/api/km/prompts/marketplace/${id}`);
}

export async function ratePromptTemplate(id: string, rating: number): Promise<PromptTemplate> {
  const res = await client.post<PromptTemplate>(`/api/km/prompts/marketplace/${id}/rate`, { rating });
  return res.data;
}

export async function forkPromptTemplate(id: string): Promise<PromptTemplate> {
  const res = await client.post<PromptTemplate>(`/api/km/prompts/marketplace/${id}/fork`);
  return res.data;
}
