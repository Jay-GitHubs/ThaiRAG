# API Reference

Base URL: `http://localhost:8080`

All protected endpoints require `Authorization: Bearer <jwt-token>` header.

---

## Health & Metrics

### `GET /health`

Health check endpoint.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `deep` | bool | If `true`, probes all providers (LLM, embedding, vector DB) |

**Response:**
```json
{
  "status": "ok",
  "providers": {
    "llm": { "status": "ok" },
    "embedding": { "status": "ok" },
    "vector_store": { "status": "ok" },
    "text_search": { "status": "ok" },
    "reranker": { "status": "ok" }
  }
}
```

### `GET /metrics`

Prometheus-format metrics.

Exposed metrics include `http_requests_total`, `http_request_duration_seconds`, `llm_tokens_total`, `active_sessions_total`, `mcp_sync_runs_total`, `mcp_sync_items_total`, `mcp_sync_duration_seconds`.

---

## Authentication

### `POST /api/auth/register`

Register a new user. The first user becomes super admin.

**Request:**
```json
{
  "email": "user@example.com",
  "password": "SecurePass123",
  "name": "User Name"
}
```

**Response:** `201 Created`
```json
{
  "id": "uuid",
  "email": "user@example.com",
  "name": "User Name",
  "role": "viewer",
  "auth_provider": "local",
  "is_super_admin": false
}
```

**Password Requirements:** Minimum 8 characters, must contain uppercase, lowercase, and digit.

### `POST /api/auth/login`

Authenticate and receive a JWT token.

**Request:**
```json
{
  "email": "user@example.com",
  "password": "SecurePass123"
}
```

**Response:**
```json
{
  "token": "eyJ...",
  "user": {
    "id": "uuid",
    "email": "user@example.com",
    "name": "User Name",
    "role": "super_admin"
  }
}
```

**Error:** `429 Too Many Requests` after too many failed attempts (brute-force protection).

### `GET /api/auth/providers`

List enabled identity providers (public, no auth required). Used by the login page to display SSO buttons.

**Response:**
```json
[
  {
    "id": "uuid",
    "name": "Corporate SSO",
    "provider_type": "oidc"
  }
]
```

### `POST /api/auth/ldap` *(stubbed — returns 501)*

LDAP authentication.

### `GET /api/auth/oauth/{provider_id}/authorize` *(stubbed — returns 501)*

OAuth2/OIDC authorization redirect.

### `GET /api/auth/oauth/callback` *(stubbed — returns 501)*

OAuth2/OIDC callback handler.

---

## OpenAI-Compatible API

### `GET /v1/models`

List available models.

**Response:**
```json
{
  "object": "list",
  "data": [
    {
      "id": "ThaiRAG-1.0",
      "object": "model",
      "created": 1234567890,
      "owned_by": "thairag"
    }
  ]
}
```

### `POST /v1/chat/completions`

Chat completion (streaming and non-streaming). **Auth required.**

**Request:**
```json
{
  "model": "ThaiRAG-1.0",
  "messages": [
    { "role": "user", "content": "What is ThaiRAG?" }
  ],
  "stream": false,
  "session_id": "optional-uuid"
}
```

**Non-Streaming Response:**
```json
{
  "id": "chatcmpl-uuid",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "ThaiRAG-1.0",
  "choices": [
    {
      "index": 0,
      "message": { "role": "assistant", "content": "ThaiRAG is..." },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 150,
    "completion_tokens": 200,
    "total_tokens": 350
  }
}
```

**Streaming Response:** Server-Sent Events (SSE)
```
data: {"id":"chatcmpl-uuid","choices":[{"delta":{"content":"Thai"},"index":0}]}

data: {"id":"chatcmpl-uuid","choices":[{"delta":{"content":"RAG"},"index":0}]}

data: {"id":"chatcmpl-uuid","choices":[{"delta":{},"finish_reason":"stop","index":0}],"usage":{"prompt_tokens":150,"completion_tokens":200,"total_tokens":350}}

data: [DONE]
```

### `POST /v1/chat/feedback`

Submit feedback for a chat response. **Auth required.**

**Request:**
```json
{
  "response_id": "chatcmpl-uuid",
  "thumbs_up": true,
  "comment": "Optional feedback comment",
  "query": "The original question",
  "answer": "The response that was given",
  "workspace_id": "optional-workspace-uuid",
  "doc_ids": ["doc-uuid-1"],
  "chunk_ids": ["chunk-uuid-1"],
  "chunk_scores": [0.85]
}
```

**Response:** `200 OK`
```json
{ "status": "ok" }
```

---

## Knowledge Management

All KM routes are under `/api/km` and require authentication.

### Organizations

#### `GET /api/km/orgs`
List all organizations.

#### `POST /api/km/orgs`
Create an organization.
```json
{ "name": "Acme Corp" }
```

#### `GET /api/km/orgs/{org_id}`
Get a single organization.

#### `DELETE /api/km/orgs/{org_id}`
Delete an organization (cascades to departments, workspaces, documents).

### Departments

#### `GET /api/km/orgs/{org_id}/depts`
List departments in an organization.

#### `POST /api/km/orgs/{org_id}/depts`
Create a department.
```json
{ "name": "Engineering" }
```

#### `GET /api/km/orgs/{org_id}/depts/{dept_id}`
Get a single department.

#### `DELETE /api/km/orgs/{org_id}/depts/{dept_id}`
Delete a department (cascades).

### Workspaces

#### `GET /api/km/orgs/{org_id}/depts/{dept_id}/workspaces`
List workspaces in a department.

#### `POST /api/km/orgs/{org_id}/depts/{dept_id}/workspaces`
Create a workspace.
```json
{ "name": "Knowledge Base" }
```

#### `GET /api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}`
Get a single workspace.

#### `DELETE /api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}`
Delete a workspace (cascades to documents).

### Permissions

Permissions can be managed at organization, department, or workspace level.

#### `GET /api/km/orgs/{org_id}/permissions`
#### `POST /api/km/orgs/{org_id}/permissions`
#### `DELETE /api/km/orgs/{org_id}/permissions`

#### `GET /api/km/orgs/{org_id}/depts/{dept_id}/permissions`
#### `POST /api/km/orgs/{org_id}/depts/{dept_id}/permissions`
#### `DELETE /api/km/orgs/{org_id}/depts/{dept_id}/permissions`

#### `GET /api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions`
#### `POST /api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions`
#### `DELETE /api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions`

**Grant Request:**
```json
{ "user_id": "uuid" }
```

**Revoke Request:**
```json
{ "user_id": "uuid" }
```

### Users

#### `GET /api/km/users`
List all users.

#### `DELETE /api/km/users/{user_id}`
Delete a user. Returns 403 if the user is a super admin.

---

## Documents

### `GET /api/km/workspaces/{workspace_id}/documents`
List documents in a workspace.

**Query Parameters:**
| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `page` | u32 | 1 | Page number |
| `per_page` | u32 | 20 | Items per page |

### `POST /api/km/workspaces/{workspace_id}/documents`
Create a document from raw text.

```json
{
  "title": "My Document",
  "content": "Document text content...",
  "format": "text/plain"
}
```

### `POST /api/km/workspaces/{workspace_id}/documents/upload`
Upload a file. **Multipart form data.**

| Field | Type | Description |
|-------|------|-------------|
| `file` | file | The document file |
| `title` | string | Optional title (defaults to filename) |

**Supported formats:** `text/plain`, `text/markdown`, `text/csv`, `text/html`, `application/pdf`, `application/vnd.openxmlformats-officedocument.wordprocessingml.document` (DOCX), `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet` (XLSX)

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}`
Get document metadata.

### `DELETE /api/km/workspaces/{workspace_id}/documents/{doc_id}`
Delete a document and all its chunks.

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}/content`
Get the extracted text content.

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}/download`
Download the original file.

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}/chunks`
List chunks for a document.

### `POST /api/km/workspaces/{workspace_id}/documents/{doc_id}/reprocess`
Re-chunk and re-embed a document.

---

## Test Query

### `POST /api/km/workspaces/{workspace_id}/test-query`

Run a search + RAG answer against a specific workspace. Returns retrieved chunks with scores, timing, and provider info.

**Request:**
```json
{ "query": "How does authentication work?" }
```

**Response:**
```json
{
  "response_id": "uuid",
  "query": "How does authentication work?",
  "chunks": [
    {
      "chunk_id": "uuid",
      "doc_id": "uuid",
      "content": "Authentication is handled via...",
      "score": 0.92,
      "chunk_index": 3,
      "page_numbers": [5],
      "section_title": "Authentication",
      "doc_title": "Security Guide"
    }
  ],
  "answer": "Authentication works by...",
  "usage": {
    "prompt_tokens": 200,
    "completion_tokens": 150,
    "total_tokens": 350,
    "chunks_retrieved": 5
  },
  "timing": {
    "search_ms": 45,
    "generation_ms": 1200,
    "total_ms": 1250
  },
  "provider_info": {
    "llm_kind": "claude",
    "llm_model": "claude-sonnet-4-20250514",
    "embedding_kind": "openai",
    "embedding_model": "text-embedding-3-small"
  },
  "pipeline_stages": [
    {
      "stage": "query_analyzer",
      "status": "completed",
      "duration_ms": 120,
      "model": "qwen3:4b"
    },
    {
      "stage": "retrieval",
      "status": "completed",
      "duration_ms": 45,
      "model": null
    },
    {
      "stage": "response_generator",
      "status": "completed",
      "duration_ms": 1200,
      "model": "qwen3:14b"
    }
  ]
}
```

The `pipeline_stages` array provides per-stage timing and model information for the chat pipeline. Each entry includes the stage name, completion status, duration in milliseconds, and the model used (if applicable).

### `POST /api/km/workspaces/{workspace_id}/test-query-stream`

SSE streaming variant of the test query endpoint. Sends real-time progress events as each pipeline stage executes, followed by the full result.

**Request:**
```json
{ "query": "How does authentication work?" }
```

**Response:** Server-Sent Events (SSE) stream.

**Event types:**

`event: progress` — Emitted as each pipeline stage starts and completes.
```json
{
  "stage": "query_analyzer",
  "status": "started",
  "duration_ms": 0,
  "model": "qwen3:4b"
}
```
```json
{
  "stage": "query_analyzer",
  "status": "completed",
  "duration_ms": 120,
  "model": "qwen3:4b"
}
```

`event: result` — Full `TestQueryResponse` JSON (same schema as the non-streaming endpoint above).

`event: error` — Emitted if the pipeline fails.
```json
{ "error": "LLM provider returned an error" }
```

`data: [DONE]` — Stream complete.

---

## MCP Connectors (Super Admin)

All connector routes are under `/api/km/connectors` and require super admin access.

### Templates

#### `GET /api/km/connectors/templates`
List available connector templates (presets for common MCP servers).

**Response:**
```json
[
  {
    "id": "github",
    "name": "GitHub",
    "description": "Access GitHub repositories, issues, and pull requests",
    "transport": "stdio",
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-github"],
    "env_keys": ["GITHUB_TOKEN"],
    "url": null,
    "resource_filters": []
  }
]
```

Available templates: `filesystem`, `fetch`, `postgres`, `sqlite`, `github`, `slack`, `google-drive`, `notion`, `confluence`, `onedrive`.

#### `POST /api/km/connectors/from-template`
Create a connector from a template.

**Request:**
```json
{
  "template_id": "github",
  "workspace_id": "workspace-uuid",
  "name": "My GitHub Connector",
  "env": {
    "GITHUB_TOKEN": "ghp_..."
  },
  "sync_mode": "on_demand"
}
```

**Response:** `201 Created` — Same as connector response below.

### CRUD

#### `POST /api/km/connectors`
Create a connector.

**Request:**
```json
{
  "name": "My MCP Server",
  "transport": "stdio",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/data"],
  "workspace_id": "workspace-uuid",
  "sync_mode": "on_demand",
  "webhook_url": "https://hooks.example.com/thairag",
  "webhook_secret": "my-secret"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Connector display name |
| `transport` | string | yes | `stdio` or `sse` |
| `command` | string | stdio only | Command to run |
| `args` | string[] | no | Command arguments |
| `env` | object | no | Environment variables for the MCP process |
| `url` | string | sse only | MCP server URL |
| `headers` | object | no | HTTP headers for SSE transport |
| `workspace_id` | uuid | yes | Target workspace for synced content |
| `sync_mode` | string | no | `on_demand` (default) or `scheduled` |
| `schedule_cron` | string | scheduled only | Cron expression (e.g., `0 */6 * * *`) |
| `resource_filters` | string[] | no | Glob patterns to filter resources |
| `max_items_per_sync` | number | no | Limit items per sync run |
| `webhook_url` | string | no | URL to POST sync notifications |
| `webhook_secret` | string | no | Bearer token for webhook auth |

**Response:** `201 Created`
```json
{
  "id": "uuid",
  "name": "My MCP Server",
  "transport": "stdio",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/data"],
  "url": null,
  "workspace_id": "workspace-uuid",
  "sync_mode": "on_demand",
  "schedule_cron": null,
  "resource_filters": [],
  "max_items_per_sync": null,
  "tool_calls": [],
  "webhook_url": "https://hooks.example.com/thairag",
  "status": "active",
  "created_at": "2026-03-18T12:00:00Z",
  "updated_at": "2026-03-18T12:00:00Z",
  "last_sync_at": null,
  "last_sync_status": null
}
```

#### `GET /api/km/connectors`
List all connectors. Supports pagination (`?page=1&per_page=20`).

#### `GET /api/km/connectors/{id}`
Get a single connector with latest sync status.

#### `PUT /api/km/connectors/{id}`
Update a connector. All fields optional.

#### `DELETE /api/km/connectors/{id}`
Delete a connector and all its sync state/history.

### Actions

#### `POST /api/km/connectors/{id}/sync`
Trigger a sync run. Connects to the MCP server, discovers resources, and ingests content into the workspace through the document pipeline (convert, chunk, embed, index).

**Response:**
```json
{
  "id": "run-uuid",
  "connector_id": "connector-uuid",
  "started_at": "2026-03-18T12:00:00Z",
  "completed_at": "2026-03-18T12:01:30Z",
  "status": "completed",
  "items_discovered": 25,
  "items_created": 20,
  "items_updated": 3,
  "items_skipped": 2,
  "items_failed": 0,
  "error_message": null,
  "duration_secs": 90.5
}
```

Sync includes:
- **Content hashing** (SHA-256) for change detection — unchanged resources are skipped
- **Retry with exponential backoff** on connection/discovery failures (default: 3 attempts)
- **Webhook notification** if `webhook_url` is configured

#### `POST /api/km/connectors/{id}/pause`
Pause a connector (stops scheduled syncs). Returns `200 OK`.

#### `POST /api/km/connectors/{id}/resume`
Resume a paused connector. Returns `200 OK`.

#### `POST /api/km/connectors/{id}/test`
Test connection to the MCP server and list available resources.

**Response:**
```json
{
  "resources": [
    {
      "uri": "file:///data/readme.md",
      "name": "readme.md",
      "mime_type": "text/markdown",
      "description": null
    }
  ]
}
```

#### `GET /api/km/connectors/{id}/sync-runs`
List sync run history for a connector.

---

## Settings (Super Admin)

All settings routes are under `/api/km/settings` and require super admin access.

### Identity Providers

#### `GET /api/km/settings/identity-providers`
List all configured identity providers.

#### `POST /api/km/settings/identity-providers`
Create an identity provider.
```json
{
  "name": "Corporate OIDC",
  "provider_type": "oidc",
  "enabled": true,
  "config": {
    "issuer_url": "https://auth.example.com",
    "client_id": "thairag",
    "client_secret": "secret",
    "scopes": "openid profile email",
    "redirect_uri": "http://localhost:8080/api/auth/oauth/callback"
  }
}
```

#### `GET /api/km/settings/identity-providers/{id}`
Get a single identity provider.

#### `PUT /api/km/settings/identity-providers/{id}`
Update an identity provider.

#### `DELETE /api/km/settings/identity-providers/{id}`
Delete an identity provider.

#### `POST /api/km/settings/identity-providers/{id}/test`
Test connectivity to the identity provider.

### Provider Configuration

#### `GET /api/km/settings/providers`
Get current provider configuration.

#### `PUT /api/km/settings/providers`
Update provider configuration.

#### `GET /api/km/settings/providers/models`
List available models from configured providers.

#### `POST /api/km/settings/providers/models/sync`
Sync model list from LLM provider.

#### `POST /api/km/settings/providers/embedding-models/sync`
Sync model list from embedding provider.

#### `POST /api/km/settings/providers/reranker-models/sync`
Sync model list from reranker provider.

### Document Configuration

#### `GET /api/km/settings/document`
Get document processing configuration.

#### `PUT /api/km/settings/document`
Update document processing configuration.

### Chat Pipeline

#### `GET /api/km/settings/chat-pipeline`
Get chat pipeline configuration. Response includes LLM mode, per-agent LLM configs, context compaction, and personal memory settings:

```json
{
  "enabled": true,
  "llm_mode": "per_agent",
  "response_generator_llm": { "kind": "ollama", "model": "qwen3:14b" },
  "query_analyzer_llm": { "kind": "ollama", "model": "qwen3:4b" },
  "context_compaction_enabled": false,
  "model_context_window": 0,
  "compaction_threshold": 0.8,
  "compaction_keep_recent": 6,
  "personal_memory_enabled": false,
  "personal_memory_top_k": 5,
  "personal_memory_max_per_user": 200,
  "personal_memory_decay_factor": 0.95,
  "personal_memory_min_relevance": 0.1,
  "live_retrieval_enabled": false,
  "live_retrieval_timeout_secs": 15,
  "live_retrieval_max_connectors": 3,
  "live_retrieval_max_content_chars": 30000,
  "live_retrieval_llm": null
}
```

**LLM modes:**
- `chat_llm` — All agents use the main LLM Provider
- `shared` — All agents share a dedicated chat LLM
- `per_agent` — Each agent can have its own LLM (falls back to shared → main LLM Provider if not set)

#### `PUT /api/km/settings/chat-pipeline`
Update chat pipeline configuration. All fields are optional — only send fields you want to change.

```json
{
  "llm_mode": "per_agent",
  "response_generator_llm": { "kind": "ollama", "model": "qwen3:14b" },
  "context_compaction_enabled": true,
  "model_context_window": 128000,
  "compaction_threshold": 0.8,
  "compaction_keep_recent": 6,
  "personal_memory_enabled": true,
  "personal_memory_top_k": 5,
  "personal_memory_max_per_user": 200,
  "personal_memory_decay_factor": 0.95,
  "personal_memory_min_relevance": 0.1,
  "live_retrieval_enabled": true,
  "live_retrieval_timeout_secs": 15,
  "live_retrieval_max_connectors": 3,
  "live_retrieval_max_content_chars": 30000,
  "live_retrieval_llm": { "kind": "ollama", "model": "qwen3:4b" }
}
```

### Presets

#### `GET /api/km/settings/presets`
List available configuration presets.

#### `POST /api/km/settings/presets/apply`
Apply a preset configuration.
```json
{ "preset": "standard" }
```

### Ollama Management

#### `GET /api/km/settings/ollama/models`
List downloaded Ollama models.

#### `POST /api/km/settings/ollama/pull`
Pull a new Ollama model.
```json
{ "model": "llama3.2" }
```

### Prompts

#### `GET /api/km/settings/prompts`
List all prompt templates.

#### `GET /api/km/settings/prompts/{key}`
Get a specific prompt template.

#### `PUT /api/km/settings/prompts/{key}`
Override a prompt template.
```json
{ "content": "You are a helpful assistant..." }
```

#### `DELETE /api/km/settings/prompts/{key}`
Delete a prompt override (reverts to default).

### Feedback & Tuning

#### `GET /api/km/settings/feedback/stats`
Get feedback statistics.

**Response:**
```json
{
  "total": 100,
  "positive": 75,
  "negative": 25,
  "satisfaction_rate": 0.75,
  "adaptive_threshold": 0.65
}
```

#### `GET /api/km/settings/feedback/entries`
List feedback entries with pagination and filtering.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | u32 | Page number |
| `per_page` | u32 | Items per page |
| `filter` | string | `all`, `positive`, `negative` |
| `workspace_id` | string | Filter by workspace |

#### `GET /api/km/settings/feedback/document-boosts`
Get per-document boost/penalty multipliers.

#### `GET /api/km/settings/feedback/golden-examples`
List golden Q&A examples.

#### `POST /api/km/settings/feedback/golden-examples`
Create a golden example.
```json
{
  "query": "What is ThaiRAG?",
  "answer": "ThaiRAG is a production-ready RAG platform...",
  "workspace_id": "optional-uuid"
}
```

#### `DELETE /api/km/settings/feedback/golden-examples`
Delete a golden example.
```json
{ "id": "example-id" }
```

#### `GET /api/km/settings/feedback/retrieval-params`
Get current retrieval parameters and suggestions.

**Response:**
```json
{
  "top_k": 5,
  "rrf_k": 60,
  "vector_weight": 0.6,
  "bm25_weight": 0.4,
  "min_score_threshold": 0.0,
  "auto_tuned": false,
  "suggested": {
    "top_k": 7,
    "vector_weight": 0.65,
    "bm25_weight": 0.35,
    "reason": "Feedback suggests increasing retrieval depth"
  }
}
```

#### `PUT /api/km/settings/feedback/retrieval-params`
Update retrieval parameters.

### Config Snapshots

Save and restore complete configuration snapshots. Useful for rollback or environment migration.

#### `POST /api/km/settings/snapshots`
Create a snapshot of the current configuration.

**Request:**
```json
{
  "name": "Before embedding migration",
  "description": "Optional description"
}
```

**Response:** `201 Created`
```json
{
  "id": "uuid",
  "name": "Before embedding migration",
  "description": "Optional description",
  "created_at": "2026-03-23T10:00:00Z",
  "created_by": "user-uuid",
  "embedding_fingerprint": "openai/text-embedding-3-small",
  "settings": {
    "providers": { "..." : "..." },
    "chat_pipeline": { "..." : "..." },
    "document": { "..." : "..." }
  }
}
```

The `embedding_fingerprint` records the embedding provider and model at snapshot time. This is checked during restore to warn about incompatible embedding changes.

#### `GET /api/km/settings/snapshots`
List all snapshots. Returns metadata only (no settings payload).

**Response:**
```json
{
  "snapshots": [
    {
      "id": "uuid",
      "name": "Before embedding migration",
      "description": "Optional description",
      "created_at": "2026-03-23T10:00:00Z",
      "created_by": "user-uuid",
      "embedding_fingerprint": "openai/text-embedding-3-small"
    }
  ]
}
```

#### `POST /api/km/settings/snapshots/{id}/restore`
Restore a configuration snapshot.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `force` | bool | If `true`, proceed even if the embedding fingerprint differs from the current config |

**Response:**
```json
{
  "restored": true,
  "warning": null
}
```

If the embedding fingerprint has changed and `force` is not set, the request returns `409 Conflict` with a warning. With `?force=true`, the response includes the warning message:
```json
{
  "restored": true,
  "warning": "Embedding model changed from openai/text-embedding-3-small to ollama/nomic-embed-text. Re-embedding may be required."
}
```

#### `DELETE /api/km/settings/snapshots/{id}`
Delete a snapshot.

### Audit Log

#### `GET /api/km/settings/audit-log`
Get audit log entries.

### Usage Stats

#### `GET /api/km/settings/usage`
Get usage statistics.

---

## API v2

### `GET /v2/models`

V2 models list with additional metadata (capabilities, context window, tier availability).

### `POST /v2/chat/completions`

V2 chat completions with enriched response including sources, intent classification, and processing time. **Auth required.**

Supports streaming (SSE). When streaming, an `event: metadata` frame is emitted before `data: [DONE]` containing sources and processing time.

### `POST /v2/search`

Direct search endpoint. Runs hybrid retrieval and reranking but bypasses LLM generation. Returns ranked chunks with scores.

**Request:**
```json
{
  "query": "What is ThaiRAG?",
  "workspace_id": "optional-uuid",
  "top_k": 5
}
```

### `GET /api/version`

Returns API version information (no auth required).

**Response:**
```json
{
  "version": "1.0.0",
  "api_versions": ["v1", "v2"]
}
```

---

## WebSocket Chat

### `WS /ws/chat`

WebSocket chat endpoint. Bi-directional JSON protocol.

**Authentication:** Pass token as query parameter — `?token=<jwt>` or `?api_key=<key>`.

**Client message:**
```json
{
  "type": "chat",
  "session_id": "optional-uuid",
  "message": "What is ThaiRAG?",
  "workspace_id": "optional-uuid"
}
```

**Server messages:**
```json
{ "type": "delta", "content": "Thai" }
{ "type": "delta", "content": "RAG is..." }
{ "type": "done", "usage": { "prompt_tokens": 150, "completion_tokens": 200, "total_tokens": 350 } }
```

---

## API Key Management

API keys use `X-API-Key: <key>` header for authentication (alternative to JWT Bearer tokens).

### `GET /api/auth/api-keys`
List all API keys for the authenticated user. Key values are not returned — only metadata.

### `POST /api/auth/api-keys`
Create a new API key. The full key value (prefixed with `trag_`) is returned only once in this response.

**Request:**
```json
{ "name": "My Integration Key" }
```

**Response:** `201 Created`
```json
{
  "id": "uuid",
  "name": "My Integration Key",
  "key": "trag_xxxxxxxxxxxxxxxxxxxx",
  "created_at": "2026-03-27T10:00:00Z"
}
```

### `DELETE /api/auth/api-keys/{key_id}`
Revoke an API key immediately.

---

## Users (extended)

The following endpoints extend the [Users](#users) section above.

### `PUT /api/km/users/{user_id}/role`
Update a user's role. **Super admin only.**

**Request:**
```json
{ "role": "admin" }
```

### `PUT /api/km/users/{user_id}/status`
Enable or disable a user account. **Super admin only.**

**Request:**
```json
{ "active": false }
```

---

## Document Versioning

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}/versions`
List all versions of a document.

**Response:**
```json
[
  { "version": 3, "created_at": "2026-03-27T10:00:00Z", "created_by": "user-uuid", "size_bytes": 4096 },
  { "version": 2, "created_at": "2026-03-20T08:00:00Z", "created_by": "user-uuid", "size_bytes": 3800 }
]
```

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}/versions/{version}`
Get the content of a specific document version.

### `GET /api/km/workspaces/{workspace_id}/documents/{doc_id}/diff`
Get a diff between two versions of a document.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `from` | u32 | Source version number |
| `to` | u32 | Target version number |

---

## Batch Document Upload

### `POST /api/km/workspaces/{workspace_id}/documents/batch`
Upload multiple documents in a single request. **Multipart form data.**

Each document part follows the same schema as the single upload endpoint. Returns a list of created document IDs and any per-file errors.

**Response:** `207 Multi-Status`
```json
{
  "created": ["doc-uuid-1", "doc-uuid-2"],
  "errors": [
    { "filename": "corrupt.pdf", "error": "Failed to extract text" }
  ]
}
```

---

## Document Refresh Schedule

### `PATCH /api/km/workspaces/{workspace_id}/documents/{doc_id}/schedule`
Update the automatic refresh schedule for a document (e.g., for URL-sourced documents).

**Request:**
```json
{
  "schedule_cron": "0 0 * * *",
  "enabled": true
}
```

---

## ACLs

Fine-grained access control lists at workspace and document level.

### Workspace ACLs

#### `GET /api/km/workspaces/{ws_id}/acl`
List ACL entries for a workspace.

#### `POST /api/km/workspaces/{ws_id}/acl`
Grant a user access to a workspace.
```json
{ "user_id": "uuid", "permission": "read" }
```

#### `DELETE /api/km/workspaces/{ws_id}/acl/{user_id}`
Revoke a user's workspace ACL entry.

### Document ACLs

#### `POST /api/km/workspaces/{ws_id}/documents/{doc_id}/acl`
Grant a user access to a specific document.
```json
{ "user_id": "uuid", "permission": "read" }
```

#### `DELETE /api/km/workspaces/{ws_id}/documents/{doc_id}/acl/{user_id}`
Revoke a user's document ACL entry.

---

## Background Jobs

Track long-running operations (document processing, batch uploads, reprocessing).

### `GET /api/km/workspaces/{workspace_id}/jobs`
List jobs for a workspace. Supports pagination (`?page=1&per_page=20`).

### `GET /api/km/workspaces/{workspace_id}/jobs/stream`
SSE stream of job status updates for a workspace. Emits `event: job` frames as job state changes.

### `GET /api/km/workspaces/{workspace_id}/jobs/{job_id}`
Get status and progress of a specific job.

**Response:**
```json
{
  "id": "uuid",
  "type": "document_processing",
  "status": "running",
  "progress": 0.65,
  "created_at": "2026-03-27T10:00:00Z",
  "completed_at": null,
  "error": null
}
```

### `DELETE /api/km/workspaces/{workspace_id}/jobs/{job_id}`
Cancel a running job.

---

## Webhooks

Register webhook endpoints to receive event notifications (document processed, sync completed, etc.).

### `GET /api/km/webhooks`
List all registered webhooks.

### `POST /api/km/webhooks`
Register a new webhook.
```json
{
  "url": "https://example.com/hook",
  "events": ["document.processed", "sync.completed"],
  "secret": "my-signing-secret"
}
```

### `DELETE /api/km/webhooks/{webhook_id}`
Delete a webhook registration.

### `POST /api/km/webhooks/{webhook_id}/test`
Send a test ping to a webhook URL. Returns the HTTP status received.

---

## Enterprise Admin

All enterprise admin routes are under `/api/km/admin` and require super admin access.

### Backup & Restore

#### `POST /api/km/admin/backup`
Create a full backup of configuration and data.

**Response:** Backup archive (binary) or a signed download URL.

#### `POST /api/km/admin/backup/preview`
Preview what a backup would include (file list and size estimates) without creating the backup.

#### `POST /api/km/admin/restore`
Restore from a backup archive. **Multipart form data.**

| Field | Type | Description |
|-------|------|-------------|
| `file` | file | Backup archive |
| `dry_run` | bool | Validate without applying |

### Vector DB Migration

Migrate vector data between providers without downtime.

#### `POST /api/km/admin/vector-migration/start`
Start a vector DB migration job.
```json
{ "target_provider": "qdrant", "target_config": { "url": "http://qdrant:6333" } }
```

#### `GET /api/km/admin/vector-migration/status`
Get current migration status and progress.

#### `POST /api/km/admin/vector-migration/validate`
Validate migration integrity by comparing vector counts and spot-checking embeddings.

#### `POST /api/km/admin/vector-migration/switch`
Atomically switch the active vector DB to the migration target (after validation).

### Rate Limit Stats

#### `GET /api/km/admin/rate-limits/stats`
Get rate limiting statistics (requests, rejections, top clients).

#### `GET /api/km/admin/rate-limits/blocked`
List currently blocked IP addresses and their lockout expiry times.

---

## Search Quality

### Evaluation Query Sets

#### `GET /api/km/eval/query-sets`
List all evaluation query sets.

#### `POST /api/km/eval/query-sets`
Create a new evaluation query set.
```json
{
  "name": "Finance Q&A",
  "workspace_id": "uuid",
  "queries": [
    { "query": "What is the refund policy?", "expected_doc_ids": ["doc-uuid-1"] }
  ]
}
```

#### `POST /api/km/eval/query-sets/import`
Import a query set from a CSV or JSON file. **Multipart form data.**

#### `GET /api/km/eval/query-sets/{id}`
Get a query set with all queries.

#### `DELETE /api/km/eval/query-sets/{id}`
Delete a query set.

#### `POST /api/km/eval/query-sets/{id}/run`
Run evaluation against the current retrieval configuration. Returns recall@k, MRR, and NDCG metrics.

#### `GET /api/km/eval/query-sets/{id}/results`
Get historical evaluation results for a query set.

### A/B Testing

#### `GET /api/km/ab-tests`
List all A/B tests.

#### `POST /api/km/ab-tests`
Create a new A/B test comparing two retrieval configurations.
```json
{
  "name": "RRF k=60 vs k=20",
  "query_set_id": "uuid",
  "config_a": { "rrf_k": 60 },
  "config_b": { "rrf_k": 20 }
}
```

#### `GET /api/km/ab-tests/{id}`
Get A/B test details.

#### `DELETE /api/km/ab-tests/{id}`
Delete an A/B test.

#### `POST /api/km/ab-tests/{id}/run`
Run both configurations against the query set and record results.

#### `POST /api/km/ab-tests/{id}/compare`
Generate a statistical comparison report between the two configurations.

---

## Plugins

Extend ThaiRAG with optional feature plugins.

### `GET /api/km/plugins`
List all available plugins and their enabled status.

### `POST /api/km/plugins/{name}/enable`
Enable a plugin by name.

### `POST /api/km/plugins/{name}/disable`
Disable a plugin by name.

---

## Knowledge Graph

Extract and query entity relationships from workspace documents.

### `GET /api/km/workspaces/{workspace_id}/knowledge-graph`
Get the knowledge graph for a workspace (entities and relationships).

### `GET /api/km/workspaces/{workspace_id}/entities`
List extracted entities in a workspace. Supports pagination and filtering by entity type.

### `GET /api/km/workspaces/{workspace_id}/entities/{entity_id}`
Get a single entity with its relationships.

### `DELETE /api/km/workspaces/{workspace_id}/entities/{entity_id}`
Delete an entity from the knowledge graph.

### `POST /api/km/workspaces/{workspace_id}/documents/{doc_id}/extract`
Trigger entity extraction on a document (adds results to the workspace knowledge graph).

---

## Chat Sessions

### `GET /api/chat/sessions/{session_id}/summary`
Get the current summary of a chat session (generated during context compaction).

### `POST /api/chat/sessions/{session_id}/summarize`
Manually trigger summarization of a chat session.

---

## Settings (extended)

The following endpoints extend the [Settings](#settings-super-admin) section above.

### Inference Logs

#### `GET /api/km/settings/inference-logs`
Get recent LLM inference logs (prompts, completions, token counts, latency).

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | u32 | Page number |
| `per_page` | u32 | Items per page |
| `workspace_id` | string | Filter by workspace |

#### `DELETE /api/km/settings/inference-logs`
Clear all inference logs.

#### `GET /api/km/settings/inference-logs/export`
Export inference logs as JSONL or CSV.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `format` | string | `jsonl` (default) or `csv` |

#### `GET /api/km/settings/inference-analytics`
Get aggregated inference analytics (token usage over time, cost estimates, latency percentiles).

### Vector DB Management

#### `GET /api/km/settings/vectordb/info`
Get vector DB status: provider, collection stats, index size, and document count.

#### `POST /api/km/settings/vectordb/clear`
Clear all vectors from the vector DB. **Irreversible.** Requires confirmation:
```json
{ "confirm": true }
```

### Scoped Settings

#### `GET /api/km/settings/scope-info`
Get information about the current settings scope (org, dept, workspace overrides in effect).

#### `DELETE /api/km/settings/scoped`
Delete all scoped (non-global) settings overrides, reverting to global defaults.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `scope` | string | `org`, `dept`, or `workspace` |
| `scope_id` | uuid | ID of the scope to clear |

### Document Configuration (extended)

#### `POST /api/km/settings/document/ai-preprocessing`
Configure AI-assisted preprocessing for documents (e.g., table extraction, image captioning, layout analysis).

**Request:**
```json
{
  "enabled": true,
  "extract_tables": true,
  "caption_images": false,
  "layout_analysis": true,
  "llm_override": { "kind": "ollama", "model": "llava:7b" }
}
```

---

## Error Responses

All errors follow a consistent format:

```json
{
  "error": {
    "type": "validation",
    "message": "query must not be empty"
  }
}
```

| HTTP Status | Error Type | Description |
|-------------|-----------|-------------|
| 400 | `validation` | Invalid request data |
| 401 | `authentication` | Missing or invalid JWT |
| 403 | `authorization` | Insufficient permissions |
| 404 | `not_found` | Resource not found |
| 429 | `rate_limit` | Rate limit exceeded |
| 500 | `internal` | Server error |

---

## Rate Limiting

Rate limiting uses a per-IP token bucket algorithm:
- Default: 10 requests/second with burst of 20
- Health and metrics endpoints are exempt
- Returns `429 Too Many Requests` with `Retry-After` header when exceeded

## CSRF Protection

State-changing endpoints (POST, PUT, DELETE) on protected routes require a valid auth token. The CSRF middleware validates the presence of the `Authorization` header to prevent cross-site request forgery.
