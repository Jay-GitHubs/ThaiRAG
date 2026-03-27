import client from './client';

// ── Types ───────────────────────────────────────────────────────────

export interface Entity {
  id: string;
  name: string;
  entity_type: string;
  workspace_id: string;
  doc_ids: string[];
  metadata: Record<string, unknown>;
  created_at: string;
}

export interface Relation {
  id: string;
  from_entity_id: string;
  to_entity_id: string;
  relation_type: string;
  confidence: number;
  doc_id: string;
  created_at: string;
}

export interface KnowledgeGraph {
  entities: Entity[];
  relations: Relation[];
}

export interface EntityWithRelations extends Entity {
  relations: Relation[];
}

export interface ExtractionResult {
  entities_created: number;
  relations_created: number;
  entities: Entity[];
}

// ── API Functions ────────────────────────────────────────────────────

export async function getKnowledgeGraph(workspaceId: string): Promise<KnowledgeGraph> {
  const res = await client.get(`/api/km/workspaces/${workspaceId}/knowledge-graph`);
  return res.data;
}

export async function listEntities(
  workspaceId: string,
  params?: { type?: string; q?: string },
): Promise<Entity[]> {
  const res = await client.get(`/api/km/workspaces/${workspaceId}/entities`, { params });
  return res.data;
}

export async function getEntity(
  workspaceId: string,
  entityId: string,
): Promise<EntityWithRelations> {
  const res = await client.get(`/api/km/workspaces/${workspaceId}/entities/${entityId}`);
  return res.data;
}

export async function extractFromDocument(
  workspaceId: string,
  docId: string,
): Promise<ExtractionResult> {
  const res = await client.post(
    `/api/km/workspaces/${workspaceId}/documents/${docId}/extract`,
  );
  return res.data;
}

export async function deleteEntity(
  workspaceId: string,
  entityId: string,
): Promise<void> {
  await client.delete(`/api/km/workspaces/${workspaceId}/entities/${entityId}`);
}
