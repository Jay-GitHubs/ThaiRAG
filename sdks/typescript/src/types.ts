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
