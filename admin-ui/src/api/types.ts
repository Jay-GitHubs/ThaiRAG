// ── ID types ────────────────────────────────────────────────────────
export type OrgId = string;
export type DeptId = string;
export type WorkspaceId = string;
export type DocId = string;
export type UserId = string;

// ── Domain models ──────────────────────────────────────────────────
export interface Organization {
  id: OrgId;
  name: string;
  created_at: string;
  updated_at: string;
}

export interface Department {
  id: DeptId;
  org_id: OrgId;
  name: string;
  created_at: string;
  updated_at: string;
}

export interface Workspace {
  id: WorkspaceId;
  dept_id: DeptId;
  name: string;
  created_at: string;
  updated_at: string;
}

export type DocStatus = 'processing' | 'ready' | 'failed';

export interface Document {
  id: DocId;
  workspace_id: WorkspaceId;
  title: string;
  mime_type: string;
  size_bytes: number;
  status: DocStatus;
  chunk_count: number;
  error_message?: string;
  processing_step?: string;
  created_at: string;
  updated_at: string;
}

// ── Document content / chunks ────────────────────────────────────────
export interface DocumentContentResponse {
  doc_id: DocId;
  converted_text: string | null;
  image_count: number;
  table_count: number;
}

export interface ChunkInfo {
  chunk_id: string;
  text: string;
  page: number | null;
  index: number;
}

export interface ChunksResponse {
  doc_id: DocId;
  chunks: ChunkInfo[];
  total: number;
}

// ── Presets & Ollama ─────────────────────────────────────────────────
export interface PresetModelInfo {
  model: string;
  role: string;
  task_weight: string;
  description: string;
}

export interface SettingsSummaryItem {
  label: string;
  value: string;
}

export interface PresetInfo {
  id: string;
  name: string;
  description: string;
  category: 'chat' | 'document';
  required_models: PresetModelInfo[];
  settings_summary: SettingsSummaryItem[];
}

export interface OllamaPullResponse {
  model: string;
  status: string;
}

export type UserRole = 'super_admin' | 'admin' | 'editor' | 'viewer';

export interface User {
  id: UserId;
  email: string;
  name: string;
  auth_provider: string;
  external_id?: string;
  is_super_admin: boolean;
  role: UserRole;
  created_at: string;
}

// ── Identity Providers ────────────────────────────────────────────
export type IdpId = string;
export type IdpType = 'oidc' | 'oauth2' | 'saml' | 'ldap';

export interface IdentityProvider {
  id: IdpId;
  name: string;
  provider_type: IdpType;
  enabled: boolean;
  config: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface CreateIdpRequest {
  name: string;
  provider_type: IdpType;
  enabled?: boolean;
  config?: Record<string, unknown>;
}

export interface UpdateIdpRequest {
  name: string;
  provider_type: IdpType;
  enabled: boolean;
  config: Record<string, unknown>;
}

export interface TestConnectionResponse {
  success: boolean;
  message: string;
}

export interface PublicIdpInfo {
  id: string;
  name: string;
  provider_type: IdpType;
}

// ── Auth ───────────────────────────────────────────────────────────
export interface LoginRequest {
  email: string;
  password: string;
}

export interface RegisterRequest {
  email: string;
  name: string;
  password: string;
}

export interface LoginResponse {
  token: string;
  user: User;
  csrf_token: string;
}

// ── Pagination ─────────────────────────────────────────────────────
export interface ListResponse<T> {
  data: T[];
  total: number;
}

export interface PaginationParams {
  limit?: number;
  offset?: number;
}

// ── Permissions ────────────────────────────────────────────────────
export type Role = 'owner' | 'admin' | 'editor' | 'viewer';

export type PermissionScope =
  | { level: 'Org'; org_id: OrgId }
  | { level: 'Dept'; org_id: OrgId; dept_id: DeptId }
  | { level: 'Workspace'; org_id: OrgId; dept_id: DeptId; workspace_id: WorkspaceId };

export interface PermissionResponse {
  user_id: string;
  email: string;
  role: Role;
  scope: PermissionScope;
}

export interface GrantPermissionRequest {
  email: string;
  role: Role;
  scope: ScopeRequest;
}

export type ScopeRequest =
  | { level: 'Org' }
  | { level: 'Dept'; dept_id: string }
  | { level: 'Workspace'; dept_id: string; workspace_id: string };

export interface RevokePermissionRequest {
  email: string;
  scope: ScopeRequest;
}

export interface ScopedGrantRequest {
  email: string;
  role: Role;
}

export interface ScopedRevokeRequest {
  email: string;
}

// ── Documents ──────────────────────────────────────────────────────
export interface IngestRequest {
  title: string;
  content: string;
  mime_type?: string;
}

export interface IngestResponse {
  doc_id: string;
  chunks: number;
  filename: string;
  mime_type: string;
  size_bytes: number;
}

// ── Provider Config ─────────────────────────────────────────────────
export interface LlmProviderInfo {
  kind: string;
  model: string;
  base_url?: string;
  has_api_key: boolean;
  supports_vision: boolean;
  max_tokens?: number;
}

export interface EmbeddingProviderInfo {
  kind: string;
  model: string;
  dimension: number;
  base_url?: string;
  has_api_key: boolean;
}

export interface VectorStoreProviderInfo {
  kind: string;
  url?: string;
  collection?: string;
  has_api_key: boolean;
  isolation?: string;
}

export interface TextSearchProviderInfo {
  kind: string;
  index_path: string;
}

export interface RerankerProviderInfo {
  kind: string;
  model?: string;
  has_api_key: boolean;
}

export interface ProviderConfigResponse {
  llm: LlmProviderInfo;
  embedding: EmbeddingProviderInfo;
  vector_store: VectorStoreProviderInfo;
  text_search: TextSearchProviderInfo;
  reranker: RerankerProviderInfo;
}

export interface AvailableModel {
  id: string;
  name: string;
  size?: number;
  modified_at?: string;
}

export interface ModelsResponse {
  provider: string;
  models: AvailableModel[];
}

export interface UpdateProviderConfigRequest {
  llm?: { kind?: string; model?: string; base_url?: string; api_key?: string };
  embedding?: { kind?: string; model?: string; dimension?: number; api_key?: string };
  vector_store?: { kind?: string; url?: string; collection?: string; isolation?: string };
  reranker?: { kind?: string; model?: string; api_key?: string };
}

// ── Document Config ────────────────────────────────────────────────
export interface AiRetryConfig {
  enabled: boolean;
  converter_max_retries: number;
  chunker_max_retries: number;
  analyzer_max_retries: number;
  analyzer_retry_below_confidence: number;
}

export interface AiPreprocessingConfig {
  enabled: boolean;
  auto_params: boolean;
  quality_threshold: number;
  max_llm_input_chars: number;
  agent_max_tokens: number;
  min_ai_size_bytes: number;
  llm?: LlmProviderInfo;
  analyzer_llm?: LlmProviderInfo;
  converter_llm?: LlmProviderInfo;
  quality_llm?: LlmProviderInfo;
  chunker_llm?: LlmProviderInfo;
  retry: AiRetryConfig;
  orchestrator_enabled: boolean;
  auto_orchestrator_budget: boolean;
  max_orchestrator_calls: number;
  orchestrator_llm?: LlmProviderInfo;
  enricher_enabled: boolean;
  enricher_llm?: LlmProviderInfo;
}

export interface DocumentConfigResponse {
  max_chunk_size: number;
  chunk_overlap: number;
  max_upload_size_mb: number;
  ai_preprocessing: AiPreprocessingConfig;
}

export type LlmConfigUpdate = { kind?: string; model?: string; base_url?: string; api_key?: string; max_tokens?: number };

export interface UpdateDocumentConfigRequest {
  max_chunk_size?: number;
  chunk_overlap?: number;
  max_upload_size_mb?: number;
  ai_preprocessing?: Partial<Omit<AiPreprocessingConfig, 'llm' | 'analyzer_llm' | 'converter_llm' | 'quality_llm' | 'chunker_llm' | 'orchestrator_llm' | 'enricher_llm' | 'retry'>> & {
    llm?: LlmConfigUpdate;
    remove_llm?: boolean;
    analyzer_llm?: LlmConfigUpdate;
    remove_analyzer_llm?: boolean;
    converter_llm?: LlmConfigUpdate;
    remove_converter_llm?: boolean;
    quality_llm?: LlmConfigUpdate;
    remove_quality_llm?: boolean;
    chunker_llm?: LlmConfigUpdate;
    remove_chunker_llm?: boolean;
    orchestrator_llm?: LlmConfigUpdate;
    remove_orchestrator_llm?: boolean;
    enricher_llm?: LlmConfigUpdate;
    remove_enricher_llm?: boolean;
    retry_enabled?: boolean;
    converter_max_retries?: number;
    chunker_max_retries?: number;
    analyzer_max_retries?: number;
    analyzer_retry_below_confidence?: number;
    orchestrator_enabled?: boolean;
    max_orchestrator_calls?: number;
  };
}

// ── Chat Pipeline Config ────────────────────────────────────────────
export interface ChatPipelineConfigResponse {
  enabled: boolean;
  llm_mode: string;
  llm?: LlmProviderInfo;
  query_analyzer_enabled: boolean;
  query_analyzer_llm?: LlmProviderInfo;
  query_rewriter_enabled: boolean;
  query_rewriter_llm?: LlmProviderInfo;
  context_curator_enabled: boolean;
  context_curator_llm?: LlmProviderInfo;
  response_generator_llm?: LlmProviderInfo;
  quality_guard_enabled: boolean;
  quality_guard_llm?: LlmProviderInfo;
  quality_guard_max_retries: number;
  quality_guard_threshold: number;
  language_adapter_enabled: boolean;
  language_adapter_llm?: LlmProviderInfo;
  orchestrator_enabled: boolean;
  max_orchestrator_calls: number;
  orchestrator_llm?: LlmProviderInfo;
  max_context_tokens: number;
  agent_max_tokens: number;
  // Feature: Conversation Memory
  conversation_memory_enabled: boolean;
  memory_max_summaries: number;
  memory_summary_max_tokens: number;
  memory_llm?: LlmProviderInfo;
  // Feature: Multi-turn Retrieval Refinement
  retrieval_refinement_enabled: boolean;
  refinement_min_relevance: number;
  refinement_max_retries: number;
  // Feature: Agentic Tool Use
  tool_use_enabled: boolean;
  tool_use_max_calls: number;
  tool_use_llm?: LlmProviderInfo;
  // Feature: Adaptive Quality Thresholds
  adaptive_threshold_enabled: boolean;
  feedback_decay_days: number;
  adaptive_min_samples: number;
  // Self-RAG
  self_rag_enabled: boolean;
  self_rag_threshold: number;
  self_rag_llm?: LlmProviderInfo;
  // Graph RAG
  graph_rag_enabled: boolean;
  graph_rag_max_entities: number;
  graph_rag_max_depth: number;
  graph_rag_llm?: LlmProviderInfo;
  // CRAG
  crag_enabled: boolean;
  crag_relevance_threshold: number;
  crag_web_search_url: string;
  crag_max_web_results: number;
  // Speculative RAG
  speculative_rag_enabled: boolean;
  speculative_candidates: number;
  // Map-Reduce RAG
  map_reduce_enabled: boolean;
  map_reduce_max_chunks: number;
  map_reduce_llm?: LlmProviderInfo;
  // RAGAS
  ragas_enabled: boolean;
  ragas_sample_rate: number;
  ragas_llm?: LlmProviderInfo;
  // Contextual Compression
  compression_enabled: boolean;
  compression_target_ratio: number;
  compression_llm?: LlmProviderInfo;
  // Multi-modal RAG
  multimodal_enabled: boolean;
  multimodal_max_images: number;
  multimodal_llm?: LlmProviderInfo;
  // RAPTOR
  raptor_enabled: boolean;
  raptor_max_depth: number;
  raptor_group_size: number;
  raptor_llm?: LlmProviderInfo;
  // ColBERT
  colbert_enabled: boolean;
  colbert_top_n: number;
  colbert_llm?: LlmProviderInfo;
  // Active Learning
  active_learning_enabled: boolean;
  active_learning_min_interactions: number;
  active_learning_max_low_confidence: number;
}

export interface UpdateChatPipelineRequest {
  enabled?: boolean;
  llm_mode?: string;
  llm?: LlmConfigUpdate;
  remove_llm?: boolean;
  query_analyzer_enabled?: boolean;
  query_analyzer_llm?: LlmConfigUpdate;
  remove_query_analyzer_llm?: boolean;
  query_rewriter_enabled?: boolean;
  query_rewriter_llm?: LlmConfigUpdate;
  remove_query_rewriter_llm?: boolean;
  context_curator_enabled?: boolean;
  context_curator_llm?: LlmConfigUpdate;
  remove_context_curator_llm?: boolean;
  response_generator_llm?: LlmConfigUpdate;
  remove_response_generator_llm?: boolean;
  quality_guard_enabled?: boolean;
  quality_guard_llm?: LlmConfigUpdate;
  remove_quality_guard_llm?: boolean;
  quality_guard_max_retries?: number;
  quality_guard_threshold?: number;
  language_adapter_enabled?: boolean;
  language_adapter_llm?: LlmConfigUpdate;
  remove_language_adapter_llm?: boolean;
  orchestrator_enabled?: boolean;
  max_orchestrator_calls?: number;
  orchestrator_llm?: LlmConfigUpdate;
  remove_orchestrator_llm?: boolean;
  max_context_tokens?: number;
  agent_max_tokens?: number;
  // Feature: Conversation Memory
  conversation_memory_enabled?: boolean;
  memory_max_summaries?: number;
  memory_summary_max_tokens?: number;
  memory_llm?: LlmConfigUpdate;
  remove_memory_llm?: boolean;
  // Feature: Multi-turn Retrieval Refinement
  retrieval_refinement_enabled?: boolean;
  refinement_min_relevance?: number;
  refinement_max_retries?: number;
  // Feature: Agentic Tool Use
  tool_use_enabled?: boolean;
  tool_use_max_calls?: number;
  tool_use_llm?: LlmConfigUpdate;
  remove_tool_use_llm?: boolean;
  // Feature: Adaptive Quality Thresholds
  adaptive_threshold_enabled?: boolean;
  feedback_decay_days?: number;
  adaptive_min_samples?: number;
  // Self-RAG
  self_rag_enabled?: boolean;
  self_rag_threshold?: number;
  self_rag_llm?: LlmConfigUpdate;
  remove_self_rag_llm?: boolean;
  // Graph RAG
  graph_rag_enabled?: boolean;
  graph_rag_max_entities?: number;
  graph_rag_max_depth?: number;
  graph_rag_llm?: LlmConfigUpdate;
  remove_graph_rag_llm?: boolean;
  // CRAG
  crag_enabled?: boolean;
  crag_relevance_threshold?: number;
  crag_web_search_url?: string;
  crag_max_web_results?: number;
  // Speculative RAG
  speculative_rag_enabled?: boolean;
  speculative_candidates?: number;
  // Map-Reduce RAG
  map_reduce_enabled?: boolean;
  map_reduce_max_chunks?: number;
  map_reduce_llm?: LlmConfigUpdate;
  remove_map_reduce_llm?: boolean;
  // RAGAS
  ragas_enabled?: boolean;
  ragas_sample_rate?: number;
  ragas_llm?: LlmConfigUpdate;
  remove_ragas_llm?: boolean;
  // Contextual Compression
  compression_enabled?: boolean;
  compression_target_ratio?: number;
  compression_llm?: LlmConfigUpdate;
  remove_compression_llm?: boolean;
  // Multi-modal RAG
  multimodal_enabled?: boolean;
  multimodal_max_images?: number;
  multimodal_llm?: LlmConfigUpdate;
  remove_multimodal_llm?: boolean;
  // RAPTOR
  raptor_enabled?: boolean;
  raptor_max_depth?: number;
  raptor_group_size?: number;
  raptor_llm?: LlmConfigUpdate;
  remove_raptor_llm?: boolean;
  // ColBERT
  colbert_enabled?: boolean;
  colbert_top_n?: number;
  colbert_llm?: LlmConfigUpdate;
  remove_colbert_llm?: boolean;
  // Active Learning
  active_learning_enabled?: boolean;
  active_learning_min_interactions?: number;
  active_learning_max_low_confidence?: number;
}

// ── Feedback ─────────────────────────────────────────────────────────
export interface FeedbackRequest {
  response_id: string;
  thumbs_up: boolean;
  comment?: string;
  query?: string;
  answer?: string;
  workspace_id?: string;
  doc_ids?: string[];
  chunk_scores?: number[];
  chunk_ids?: string[];
}

export interface FeedbackResponse {
  ok: boolean;
}

export interface FeedbackEntry {
  response_id: string;
  user_id: string;
  thumbs_up: boolean;
  comment?: string;
  timestamp: number;
  query?: string;
  answer?: string;
  workspace_id?: string;
  doc_ids: string[];
  chunk_scores: number[];
  chunk_ids: string[];
}

export interface FeedbackStats {
  total: number;
  positive: number;
  negative: number;
  positive_rate: number;
  current_threshold: number;
  adaptive_threshold?: number;
  adaptive_enabled: boolean;
  min_samples: number;
}

export interface FeedbackListResponse {
  entries: FeedbackEntry[];
  total: number;
  total_filtered: number;
}

export interface DocumentBoost {
  doc_id: string;
  boost: number;
  positive_count: number;
  negative_count: number;
  total_count: number;
}

export interface GoldenExample {
  id: string;
  query: string;
  answer: string;
  workspace_id?: string;
  created_at: number;
  source_response_id?: string;
}

export interface CreateGoldenExampleRequest {
  response_id?: string;
  query: string;
  answer: string;
  workspace_id?: string;
}

export interface RetrievalParams {
  top_k: number;
  rrf_k: number;
  vector_weight: number;
  bm25_weight: number;
  min_score_threshold: number;
  auto_tuned: boolean;
  samples_used: number;
  suggested?: SuggestedParams;
}

export interface SuggestedParams {
  top_k: number;
  vector_weight: number;
  bm25_weight: number;
  reason: string;
}

export interface UpdateRetrievalParamsRequest {
  top_k?: number;
  vector_weight?: number;
  bm25_weight?: number;
  min_score_threshold?: number;
  apply_suggestions?: boolean;
}

// ── Prompts ─────────────────────────────────────────────────────────
export interface PromptEntry {
  key: string;
  description: string;
  category: string;
  source: 'default' | 'override';
  template: string;
}

export interface UpdatePromptRequest {
  template: string;
  description?: string;
}

// ── Test Query (KM Chat) ────────────────────────────────────────────
export interface TestQueryRequest {
  query: string;
}

export interface RetrievedChunk {
  chunk_id: string;
  doc_id: string;
  content: string;
  score: number;
  chunk_index: number;
  page_numbers?: number[];
  section_title?: string;
  doc_title?: string;
}

export interface TestQueryUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  chunks_retrieved: number;
}

export interface TestQueryTiming {
  search_ms: number;
  generation_ms: number;
  total_ms: number;
}

export interface TestQueryProviderInfo {
  llm_kind: string;
  llm_model: string;
  embedding_kind: string;
  embedding_model: string;
}

export interface TestQueryResponse {
  response_id: string;
  query: string;
  chunks: RetrievedChunk[];
  answer: string;
  usage: TestQueryUsage;
  timing: TestQueryTiming;
  provider_info: TestQueryProviderInfo;
}

// ── Usage Stats ────────────────────────────────────────────────────
export interface UsageStatsResponse {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  llm_kind: string;
  llm_model: string;
  embedding_kind: string;
  embedding_model: string;
  estimated_cost_usd: number | null;
}

// ── Health ──────────────────────────────────────────────────────────
export interface HealthResponse {
  status: string;
  version: string;
  uptime_secs?: number;
  embedding?: string;
}
