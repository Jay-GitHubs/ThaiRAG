export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface ChatChoice {
  index: number;
  message: ChatMessage;
  delta?: Partial<ChatMessage>;
  finish_reason?: string | null;
}

export interface ChatUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface ChatResponse {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: ChatChoice[];
  usage?: ChatUsage;
}

export interface HealthResponse {
  status: string;
  version?: string;
  providers?: Record<string, string>;
}

export interface Organization {
  id: string;
  name: string;
  created_at?: string;
  updated_at?: string;
}

export interface Department {
  id: string;
  org_id: string;
  name: string;
  created_at?: string;
  updated_at?: string;
}

export interface Workspace {
  id: string;
  dept_id: string;
  name: string;
  created_at?: string;
  updated_at?: string;
}

export interface Document {
  id: string;
  workspace_id: string;
  title: string;
  mime_type?: string;
  status?: string;
  chunk_count?: number;
  created_at?: string;
  updated_at?: string;
}

export interface SearchResult {
  query: string;
  results: SearchHit[];
  total?: number;
}

export interface SearchHit {
  doc_id: string;
  title: string;
  score: number;
  content?: string;
}

export interface FeedbackResponse {
  id: string;
  response_id: string;
  rating: number;
  comment?: string;
  created_at?: string;
}

export interface ModelInfo {
  id: string;
  object: string;
  created?: number;
  owned_by?: string;
}

export interface ModelsResponse {
  object: string;
  data: ModelInfo[];
}

export interface ChatOptions {
  model?: string;
  stream?: boolean;
  temperature?: number;
  max_tokens?: number;
  [key: string]: unknown;
}

export interface SearchAnalyticsPopular {
  query: string;
  count: number;
}

export interface SearchAnalyticsSummary {
  total_queries: number;
  avg_latency_ms: number;
  zero_result_rate: number;
}

export interface LineageRecord {
  response_id: string;
  chunk_id: string;
  doc_id: string;
  score: number;
  rank: number;
}

export interface AuditLogEntry {
  id: string;
  timestamp: string;
  user_email?: string;
  action: string;
  detail: string;
  success: boolean;
}

export interface AuditAnalytics {
  actions_by_type: [string, number][];
  events_per_day: [string, number][];
  total_events: number;
  success_rate?: number;
}

export interface Tenant {
  id: string;
  name: string;
  plan: string;
  is_active: boolean;
  created_at: string;
}

export interface Permission {
  resource: string;
  actions: string[];
}

export interface CustomRole {
  id: string;
  name: string;
  description: string;
  permissions: Permission[];
}

export interface PromptTemplate {
  id: string;
  name: string;
  content: string;
  category: string;
  variables: string[];
}

export interface FinetuneDataset {
  id: string;
  name: string;
  description: string;
  pair_count: number;
}

export interface FinetuneJob {
  id: string;
  dataset_id: string;
  status: string;
  created_at: string;
}
