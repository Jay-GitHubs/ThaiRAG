// ── Auth ─────────────────────────────────────────────────────────────
export interface User {
  id: string;
  email: string;
  name: string;
  role: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface LoginResponse {
  token: string;
  user: User;
}

export interface RegisterRequest {
  email: string;
  name: string;
  password: string;
}

/** A public identity provider for an SSO "Sign in with X" button. */
export interface ProviderInfo {
  id: string;
  name: string;
  provider_type: string;
}

// ── Conversations & messages (mirror the backend store rows) ──────────
export interface Conversation {
  id: string;
  user_id: string;
  title: string;
  workspace_scope?: string | null;
  created_at: string;
  updated_at: string;
}

/** A stored message. `citations`/`images`/`token_stats` are JSON strings the
 *  backend persists verbatim; parse with the helpers below. */
export interface MessageRow {
  id: string;
  conversation_id: string;
  role: 'user' | 'assistant' | string;
  content: string;
  citations: string;
  images: string;
  token_stats: string;
  created_at: string;
  feedback: number;
}

export interface Citation {
  doc_id: string;
  title: string;
  page?: number;
  section?: string;
  url?: string;
  /** Snippet of the cited passage, used to locate + highlight it in the viewer. */
  snippet?: string;
}

export interface DocumentSource {
  doc_id: string;
  title: string;
  mime_type: string;
  content: string;
}

/** One named contributor to the deterministic confidence score. */
export interface ConfidenceFactor {
  label: string;
  detail: string;
}

export interface ImageRef {
  image_id: string;
  url: string;
  page?: number;
}

/** A workspace the user can scope a conversation to (chat scope picker). */
export interface WorkspaceOption {
  id: string;
  name: string;
}

/** A file attached to a chat turn (base64). Matches the backend `Attachment`. */
export interface Attachment {
  name: string;
  mime_type: string;
  data: string;
}

// ── First-party streaming chat protocol (SSE `data:` JSON objects) ─────
export type StreamEvent =
  | { type: 'progress'; stage: string; status: string }
  | { type: 'token'; text: string }
  | { type: 'citation'; citations: Citation[] }
  | { type: 'image'; images: ImageRef[] }
  | {
      type: 'done';
      message_id: string;
      usage: { prompt_tokens: number; completion_tokens: number };
      confidence?: number | null;
      confidence_summary?: string | null;
      confidence_factors?: ConfidenceFactor[] | null;
    }
  | { type: 'error'; message: string };

export function parseCitations(json: string): Citation[] {
  try {
    return JSON.parse(json) as Citation[];
  } catch {
    return [];
  }
}

export function parseImages(json: string): ImageRef[] {
  try {
    return JSON.parse(json) as ImageRef[];
  } catch {
    return [];
  }
}
