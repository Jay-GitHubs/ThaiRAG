import client from './client';
import type {
  Connector,
  ConnectorTemplate,
  CreateConnectorRequest,
  CreateFromTemplateRequest,
  UpdateConnectorRequest,
  SyncRunResponse,
  ListResponse,
  ResourceListResponse,
} from './types';

export async function listConnectors() {
  const res = await client.get<ListResponse<Connector>>('/api/km/connectors');
  return res.data;
}

export async function getConnector(id: string) {
  const res = await client.get<Connector>(`/api/km/connectors/${id}`);
  return res.data;
}

export async function createConnector(data: CreateConnectorRequest) {
  const res = await client.post<Connector>('/api/km/connectors', data);
  return res.data;
}

export async function createFromTemplate(data: CreateFromTemplateRequest) {
  const res = await client.post<Connector>(
    '/api/km/connectors/from-template',
    data,
  );
  return res.data;
}

export async function updateConnector(id: string, data: UpdateConnectorRequest) {
  const res = await client.put<Connector>(`/api/km/connectors/${id}`, data);
  return res.data;
}

export async function deleteConnector(id: string) {
  await client.delete(`/api/km/connectors/${id}`);
}

export async function triggerSync(id: string) {
  const res = await client.post<SyncRunResponse>(
    `/api/km/connectors/${id}/sync`,
  );
  return res.data;
}

export async function pauseConnector(id: string) {
  await client.post(`/api/km/connectors/${id}/pause`);
}

export async function resumeConnector(id: string) {
  await client.post(`/api/km/connectors/${id}/resume`);
}

export async function listSyncRuns(id: string) {
  const res = await client.get<ListResponse<SyncRunResponse>>(
    `/api/km/connectors/${id}/sync-runs`,
  );
  return res.data;
}

export async function testConnection(id: string) {
  const res = await client.post<ResourceListResponse>(
    `/api/km/connectors/${id}/test`,
  );
  return res.data;
}

export async function listConnectorTemplates() {
  const res = await client.get<ConnectorTemplate[]>(
    '/api/km/connectors/templates',
  );
  return res.data;
}
