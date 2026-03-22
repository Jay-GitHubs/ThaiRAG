import client from './client';
import type {
  ChatPipelineConfigResponse,
  CreateGoldenExampleRequest,
  CreateIdpRequest,
  DocumentBoost,
  DocumentConfigResponse,
  FeedbackListResponse,
  FeedbackStats,
  GoldenExample,
  IdentityProvider,
  ListResponse,
  ModelsResponse,
  OllamaPullResponse,
  PresetInfo,
  PromptEntry,
  ProviderConfigResponse,
  PublicIdpInfo,
  RetrievalParams,
  TestConnectionResponse,
  UpdateChatPipelineRequest,
  UpdateDocumentConfigRequest,
  UpdateIdpRequest,
  UpdatePromptRequest,
  UpdateProviderConfigRequest,
  UpdateRetrievalParamsRequest,
  UsageStatsResponse,
  ConfigSnapshot,
  SnapshotListItem,
  VectorDbClearResponse,
  VectorDbInfo,
} from './types';

export async function listIdentityProviders() {
  const res = await client.get<ListResponse<IdentityProvider>>(
    '/api/km/settings/identity-providers',
  );
  return res.data;
}

export async function createIdentityProvider(data: CreateIdpRequest) {
  const res = await client.post<IdentityProvider>('/api/km/settings/identity-providers', data);
  return res.data;
}

export async function getIdentityProvider(id: string) {
  const res = await client.get<IdentityProvider>(`/api/km/settings/identity-providers/${id}`);
  return res.data;
}

export async function updateIdentityProvider(id: string, data: UpdateIdpRequest) {
  const res = await client.put<IdentityProvider>(
    `/api/km/settings/identity-providers/${id}`,
    data,
  );
  return res.data;
}

export async function deleteIdentityProvider(id: string) {
  await client.delete(`/api/km/settings/identity-providers/${id}`);
}

export async function testIdpConnection(id: string) {
  const res = await client.post<TestConnectionResponse>(
    `/api/km/settings/identity-providers/${id}/test`,
  );
  return res.data;
}

export async function getProviderConfig() {
  const res = await client.get<ProviderConfigResponse>('/api/km/settings/providers');
  return res.data;
}

export async function updateProviderConfig(data: UpdateProviderConfigRequest) {
  const res = await client.put<ProviderConfigResponse>('/api/km/settings/providers', data);
  return res.data;
}

export async function listAvailableModels() {
  const res = await client.get<ModelsResponse>('/api/km/settings/providers/models');
  return res.data;
}

export async function syncModels(data: { kind: string; base_url?: string; api_key?: string }) {
  const res = await client.post<ModelsResponse>('/api/km/settings/providers/models/sync', data);
  return res.data;
}

export async function syncEmbeddingModels(data: { kind: string; base_url?: string; api_key?: string }) {
  const res = await client.post<ModelsResponse>('/api/km/settings/providers/embedding-models/sync', data);
  return res.data;
}

export async function syncRerankerModels(data: { kind: string }) {
  const res = await client.post<ModelsResponse>('/api/km/settings/providers/reranker-models/sync', data);
  return res.data;
}

export async function getDocumentConfig() {
  const res = await client.get<DocumentConfigResponse>('/api/km/settings/document');
  return res.data;
}

export async function updateDocumentConfig(data: UpdateDocumentConfigRequest) {
  const res = await client.put<DocumentConfigResponse>('/api/km/settings/document', data);
  return res.data;
}

export async function getChatPipelineConfig() {
  const res = await client.get<ChatPipelineConfigResponse>('/api/km/settings/chat-pipeline');
  return res.data;
}

export async function updateChatPipelineConfig(data: UpdateChatPipelineRequest) {
  const res = await client.put<ChatPipelineConfigResponse>('/api/km/settings/chat-pipeline', data);
  return res.data;
}

export async function listEnabledProviders() {
  const res = await client.get<PublicIdpInfo[]>('/api/auth/providers');
  return res.data;
}

export async function getFeedbackStats() {
  const res = await client.get<FeedbackStats>('/api/km/settings/feedback/stats');
  return res.data;
}

export async function listFeedbackEntries(params?: {
  limit?: number;
  offset?: number;
  filter?: string;
  workspace_id?: string;
}) {
  const res = await client.get<FeedbackListResponse>('/api/km/settings/feedback/entries', {
    params,
  });
  return res.data;
}

export async function getDocumentBoosts() {
  const res = await client.get<DocumentBoost[]>('/api/km/settings/feedback/document-boosts');
  return res.data;
}

export async function listGoldenExamples() {
  const res = await client.get<GoldenExample[]>('/api/km/settings/feedback/golden-examples');
  return res.data;
}

export async function createGoldenExample(data: CreateGoldenExampleRequest) {
  const res = await client.post<GoldenExample>('/api/km/settings/feedback/golden-examples', data);
  return res.data;
}

export async function deleteGoldenExample(id: string) {
  await client.delete('/api/km/settings/feedback/golden-examples', { params: { id } });
}

export async function getRetrievalParams() {
  const res = await client.get<RetrievalParams>('/api/km/settings/feedback/retrieval-params');
  return res.data;
}

export async function updateRetrievalParams(data: UpdateRetrievalParamsRequest) {
  const res = await client.put<RetrievalParams>('/api/km/settings/feedback/retrieval-params', data);
  return res.data;
}

// ── Presets ──────────────────────────────────────────────────────────

export async function listPresets() {
  const res = await client.get<PresetInfo[]>('/api/km/settings/presets');
  return res.data;
}

export async function applyPreset(presetId: string, ollamaUrl?: string) {
  const res = await client.post('/api/km/settings/presets/apply', {
    preset_id: presetId,
    ollama_url: ollamaUrl || 'http://host.docker.internal:11435',
  });
  return res.data;
}

// ── Ollama Model Management ─────────────────────────────────────────

export async function listOllamaModels() {
  const res = await client.get<ModelsResponse>('/api/km/settings/ollama/models');
  return res.data;
}

export async function pullOllamaModel(model: string, ollamaUrl?: string) {
  const res = await client.post<OllamaPullResponse>('/api/km/settings/ollama/pull', {
    model,
    ollama_url: ollamaUrl || 'http://host.docker.internal:11435',
  });
  return res.data;
}

// ── Prompt Management ───────────────────────────────────────────────

export async function listPrompts() {
  const res = await client.get<PromptEntry[]>('/api/km/settings/prompts');
  return res.data;
}

export async function getPrompt(key: string) {
  const res = await client.get<PromptEntry>(`/api/km/settings/prompts/${key}`);
  return res.data;
}

export async function updatePrompt(key: string, data: UpdatePromptRequest) {
  const res = await client.put<{ status: string; key: string }>(
    `/api/km/settings/prompts/${key}`,
    data,
  );
  return res.data;
}

export async function deletePromptOverride(key: string) {
  const res = await client.delete<{ status: string; key: string }>(
    `/api/km/settings/prompts/${key}`,
  );
  return res.data;
}

export async function getUsageStats() {
  const res = await client.get<UsageStatsResponse>('/api/km/settings/usage');
  return res.data;
}

// ── Vector Database Management ─────────────────────────────────────

export async function getVectorDbInfo() {
  const res = await client.get<VectorDbInfo>('/api/km/settings/vectordb/info');
  return res.data;
}

export async function clearVectorDb() {
  const res = await client.post<VectorDbClearResponse>('/api/km/settings/vectordb/clear');
  return res.data;
}

// ── Config Snapshots ────────────────────────────────────────────

export async function listSnapshots() {
  const res = await client.get<SnapshotListItem[]>('/api/km/settings/snapshots');
  return res.data;
}

export async function createSnapshot(data: { name: string; description?: string }) {
  const res = await client.post<ConfigSnapshot>('/api/km/settings/snapshots', data);
  return res.data;
}

export async function restoreSnapshot(
  id: string,
  opts?: { force?: boolean; skipEmbedding?: boolean },
) {
  const params = new URLSearchParams();
  if (opts?.force) params.set('force', 'true');
  if (opts?.skipEmbedding) params.set('skip_embedding', 'true');
  const qs = params.toString();
  const res = await client.post<{ status: string; warning?: string }>(
    `/api/km/settings/snapshots/${id}/restore${qs ? `?${qs}` : ''}`,
  );
  return res.data;
}

export async function deleteSnapshot(id: string) {
  await client.delete(`/api/km/settings/snapshots/${id}`);
}
