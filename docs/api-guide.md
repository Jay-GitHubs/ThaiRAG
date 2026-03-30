# ThaiRAG API Guide

## Overview

ThaiRAG exposes an OpenAI-compatible API under a single model identity: **ThaiRAG-1.0**.

## Endpoints

### Health Check
```
GET /health
GET /health?deep=true
```

### Prometheus Metrics
```
GET /metrics
```

### List Models
```
GET /v1/models
```

### Chat Completions
```
POST /v1/chat/completions
Content-Type: application/json

{
  "model": "ThaiRAG-1.0",
  "messages": [
    {"role": "user", "content": "Your question here"}
  ]
}
```

Supports streaming (`"stream": true`) and optional session persistence (`"session_id": "<uuid>"`).

---

## Authentication

All `/api/km/*` and `/api/auth/register` endpoints require a valid JWT token when auth is enabled (`THAIRAG__AUTH__ENABLED=true`).

### Register
```
POST /api/auth/register
Content-Type: application/json

{"email": "user@example.com", "name": "User Name", "password": "MyPass123"}
```

**Password policy:** Passwords must be at least 8 characters (configurable via `THAIRAG__AUTH__PASSWORD_MIN_LENGTH`) and contain at least one uppercase letter, one lowercase letter, and one digit.

**First-user bootstrap:** The first registered user is automatically promoted to `super_admin`. This provides a zero-config bootstrap mechanism — no env vars required for initial setup.

### Login
```
POST /api/auth/login
Content-Type: application/json

{"email": "user@example.com", "password": "MyPass123"}
```

Returns `{ "token": "<JWT>", "user": { ... }, "csrf_token": "<uuid>" }`.

The `csrf_token` is used for cookie-based auth flows. When using Bearer token auth (the default), CSRF protection is automatic since browsers don't auto-send Bearer headers.

**Brute-force protection:** After 5 consecutive failed login attempts (configurable via `THAIRAG__AUTH__MAX_LOGIN_ATTEMPTS`), the account is locked for 300 seconds (configurable via `THAIRAG__AUTH__LOCKOUT_DURATION_SECS`). The lockout is per-email and case-insensitive.

### Super Admin Seeding

On startup, if `THAIRAG__ADMIN__EMAIL` and `THAIRAG__ADMIN__PASSWORD` env vars are set, a super admin user is created (or updated). Super admins can manage identity providers via the settings API.

Additionally, the first user registered via the API is automatically promoted to super_admin (see above).

### OIDC / OAuth2 SSO

```
GET /api/auth/providers              # Public — list enabled IdPs (no secrets)
GET /api/auth/oauth/{provider_id}/authorize   # Redirect to IdP login
GET /api/auth/oauth/callback         # IdP callback (exchanges code for token)
POST /api/auth/ldap                  # LDAP login (not yet implemented)
```

The OIDC flow:
1. Browser hits `/api/auth/oauth/{provider_id}/authorize`
2. Server performs OIDC discovery, builds authorization URL with PKCE, redirects browser to IdP
3. User authenticates at IdP (e.g., Keycloak, Duende IdentityServer)
4. IdP redirects to `/api/auth/oauth/callback` with authorization code
5. Server exchanges code for tokens, verifies id_token, upserts user, issues JWT
6. Server redirects to `/login#token=<jwt>&user=<json>` — frontend picks up the fragment

---

## KM Hierarchy

All KM endpoints require `Authorization: Bearer <token>`.

### Organizations
```
GET    /api/km/orgs
POST   /api/km/orgs
DELETE /api/km/orgs/{org_id}
```

### Departments
```
GET    /api/km/orgs/{org_id}/depts
POST   /api/km/orgs/{org_id}/depts
DELETE /api/km/depts/{dept_id}
```

### Workspaces
```
GET    /api/km/depts/{dept_id}/workspaces
POST   /api/km/depts/{dept_id}/workspaces
DELETE /api/km/workspaces/{ws_id}
```

### Documents
```
GET    /api/km/workspaces/{ws_id}/documents
POST   /api/km/workspaces/{ws_id}/documents/ingest    # multipart or JSON
DELETE /api/km/documents/{doc_id}
```

### Users
```
GET    /api/km/users
DELETE /api/km/users/{user_id}    # Cannot delete super admins
```

### Permissions
```
GET    /api/km/orgs/{org_id}/permissions
POST   /api/km/orgs/{org_id}/permissions
DELETE /api/km/permissions/{perm_id}
```

Supports scoped permissions: org, dept, workspace.

---

## Settings (Super Admin Only)

These endpoints require a super admin JWT.

### Identity Providers
```
GET    /api/km/settings/identity-providers
POST   /api/km/settings/identity-providers
GET    /api/km/settings/identity-providers/{id}
PUT    /api/km/settings/identity-providers/{id}
DELETE /api/km/settings/identity-providers/{id}
POST   /api/km/settings/identity-providers/{id}/test
```

Provider types: `oidc`, `oauth2`, `saml`, `ldap`.

OIDC/OAuth2 config fields: `issuer_url`, `client_id`, `client_secret`, `scopes`, `redirect_uri`.

> **Docker note:** The `issuer_url` must be reachable from the ThaiRAG container — use `host.docker.internal` instead of `localhost`. The `redirect_uri` is browser-facing — use `localhost` with the admin-ui port (8081). See [OIDC_TESTING.md](OIDC_TESTING.md) for details.

### Provider Configuration
```
GET    /api/km/settings/providers          # Get provider config
PUT    /api/km/settings/providers          # Update + hot-reload
```

### Chat Pipeline Configuration
```
GET    /api/km/settings/chat-pipeline      # Get pipeline config
PUT    /api/km/settings/chat-pipeline      # Update + hot-reload
```

Includes **LLM Mode**, **Context Compaction**, and **Personal Memory** settings:
- `llm_mode` — LLM assignment mode: `chat_llm` (use main LLM), `shared` (dedicated chat LLM), `per_agent` (individual LLM per agent)
- Per-agent LLM configs (when `llm_mode=per_agent`): `query_analyzer_llm`, `retriever_llm`, `response_generator_llm`, etc.
- `context_compaction_enabled` — Auto-summarize older messages when near context limit
- `model_context_window` — Context window size in tokens (0 = auto-detect)
- `compaction_threshold` — Trigger at this fraction of context window (default: 0.8)
- `compaction_keep_recent` — Recent messages to keep intact (default: 6)
- `personal_memory_enabled` — Per-user memory across sessions
- `personal_memory_top_k` — Memories retrieved per query (default: 5)
- `personal_memory_max_per_user` — Max memories per user (default: 200)
- `personal_memory_decay_factor` — Relevance decay rate (default: 0.95)
- `personal_memory_min_relevance` — Prune threshold (default: 0.1)

### Document Processing Configuration
```
GET    /api/km/settings/document           # Get document config
PUT    /api/km/settings/document           # Update + hot-reload
```

### Prompt Management
```
GET    /api/km/settings/prompts            # List all prompts (38 templates)
GET    /api/km/settings/prompts/{key}      # Get single prompt
PUT    /api/km/settings/prompts/{key}      # Override prompt template
DELETE /api/km/settings/prompts/{key}      # Reset to default
```

### Audit Log (OWASP A09)
```
GET    /api/km/settings/audit-log          # Query audit log (super admin only)
       ?action=login_failed               # Filter by action type
       &limit=100                         # Max entries (default: 100, max: 1000)
```

Actions logged: `login`, `login_failed`, `register`, `user_deleted`, `permission_granted`, `permission_revoked`, `settings_changed`, `idp_created`, `idp_updated`, `idp_deleted`, `prompt_updated`, `prompt_deleted`.

### Config Snapshots
```
POST   /api/km/settings/snapshots           # Create snapshot (captures current config)
GET    /api/km/settings/snapshots           # List all snapshots
POST   /api/km/settings/snapshots/restore   # Restore a snapshot by ID
DELETE /api/km/settings/snapshots/{id}      # Delete a snapshot
```

Snapshots capture the complete system configuration (providers, chat pipeline, document processing, prompts). Stored in the `settings` KV table with `snapshot.{uuid}` key prefix.

### Test Query (with Pipeline Stages)
```
GET    /api/km/test-query?q=<query>         # Test query with pipeline_stages in response
GET    /api/km/test-query-stream?q=<query>  # SSE stream with real-time pipeline progress
```

The `test-query` response includes a `pipeline_stages` array showing timing for each pipeline stage (query analysis, retrieval, reranking, context assembly, response generation).

The `test-query-stream` endpoint returns Server-Sent Events:
- `event: pipeline_progress` — Sent as each stage starts and completes, with `stage`, `status`, and `duration_ms` fields
- `event: result` — Final complete test-query response (same shape as the non-streaming endpoint)

### Feedback
```
POST   /v1/chat/feedback                   # Submit quality feedback
GET    /api/km/settings/feedback/stats      # Feedback statistics (super admin)
```

---

## Search Analytics

### Popular Queries
```bash
curl -s http://localhost:8080/api/km/search-analytics/popular \
  -H "Authorization: Bearer $TOKEN"
```

Returns the most frequently searched queries, ranked by count. Useful for understanding what users are looking for and identifying content gaps.

### Analytics Summary
```bash
curl -s http://localhost:8080/api/km/search-analytics/summary \
  -H "Authorization: Bearer $TOKEN"
```

Returns aggregated search metrics including total queries, average latency, zero-result rate, and top workspaces by query volume.

---

## Document Lineage

Track the provenance of RAG responses back to their source documents and chunks.

### Response Lineage
```bash
curl -s http://localhost:8080/api/km/lineage/response/{response_id} \
  -H "Authorization: Bearer $TOKEN"
```

Returns the full lineage for a chat response: which documents and chunks were retrieved, their scores, and how they contributed to the generated answer.

### Document Lineage
```bash
curl -s http://localhost:8080/api/km/lineage/document/{doc_id} \
  -H "Authorization: Bearer $TOKEN"
```

Returns all responses that referenced a given document, showing how the document has been used across queries.

---

## Audit Log (Extended)

### Export Audit Log
```bash
# Export as JSON
curl -s "http://localhost:8080/api/km/settings/audit-log/export?format=json" \
  -H "Authorization: Bearer $TOKEN" -o audit-log.json

# Export as CSV
curl -s "http://localhost:8080/api/km/settings/audit-log/export?format=csv" \
  -H "Authorization: Bearer $TOKEN" -o audit-log.csv
```

Exports the full audit log in the specified format. Supports `json` and `csv`. Super admin only.

### Audit Log Analytics
```bash
curl -s http://localhost:8080/api/km/settings/audit-log/analytics \
  -H "Authorization: Bearer $TOKEN"
```

Returns aggregated audit analytics: event counts by action type, most active users, and activity trends over time.

---

## Personal Memory

Per-user memories that persist across chat sessions when `personal_memory_enabled` is turned on in the chat pipeline settings.

### List Memories
```bash
curl -s http://localhost:8080/api/km/users/{user_id}/memories \
  -H "Authorization: Bearer $TOKEN"
```

Returns all stored memories for a user, including content, relevance score, and creation timestamp.

### Delete a Memory
```bash
curl -s -X DELETE http://localhost:8080/api/km/users/{user_id}/memories/{memory_id} \
  -H "Authorization: Bearer $TOKEN"
```

Deletes a specific memory entry. Users can delete their own memories; super admins can delete any user's memories.

---

## Multi-Tenancy

Isolate organizations into separate tenants with quota management.

### List Tenants
```bash
curl -s http://localhost:8080/api/km/tenants \
  -H "Authorization: Bearer $TOKEN"
```

### Create a Tenant
```bash
curl -s -X POST http://localhost:8080/api/km/tenants \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Acme Corp", "max_users": 50, "max_documents": 10000, "max_storage_mb": 5120}'
```

### Get a Tenant
```bash
curl -s http://localhost:8080/api/km/tenants/{tenant_id} \
  -H "Authorization: Bearer $TOKEN"
```

### Update a Tenant
```bash
curl -s -X PUT http://localhost:8080/api/km/tenants/{tenant_id} \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Acme Corp (Updated)", "max_users": 100}'
```

### Delete a Tenant
```bash
curl -s -X DELETE http://localhost:8080/api/km/tenants/{tenant_id} \
  -H "Authorization: Bearer $TOKEN"
```

### Get Tenant Quota
```bash
curl -s http://localhost:8080/api/km/tenants/{tenant_id}/quota \
  -H "Authorization: Bearer $TOKEN"
```

Returns the tenant's configured limits (max users, documents, storage).

### Get Tenant Usage
```bash
curl -s http://localhost:8080/api/km/tenants/{tenant_id}/usage \
  -H "Authorization: Bearer $TOKEN"
```

Returns current usage against quota (active users, document count, storage used).

### Assign Organization to Tenant
```bash
curl -s -X POST http://localhost:8080/api/km/tenants/{tenant_id}/orgs \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"org_id": "org-uuid"}'
```

Associates an organization with a tenant, applying the tenant's quotas and isolation policies.

---

## Custom Roles

Define custom roles with fine-grained permissions beyond the built-in `viewer`, `admin`, and `super_admin` roles.

### List Roles
```bash
curl -s http://localhost:8080/api/km/roles \
  -H "Authorization: Bearer $TOKEN"
```

### Create a Role
```bash
curl -s -X POST http://localhost:8080/api/km/roles \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "content_manager", "description": "Can manage documents but not settings", "permissions": ["documents.read", "documents.write", "documents.delete"]}'
```

### Get a Role
```bash
curl -s http://localhost:8080/api/km/roles/{role_id} \
  -H "Authorization: Bearer $TOKEN"
```

### Update a Role
```bash
curl -s -X PUT http://localhost:8080/api/km/roles/{role_id} \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"description": "Updated description", "permissions": ["documents.read", "documents.write"]}'
```

### Delete a Role
```bash
curl -s -X DELETE http://localhost:8080/api/km/roles/{role_id} \
  -H "Authorization: Bearer $TOKEN"
```

---

## Document Collaboration

Collaborate on documents with comments, annotations, and review workflows.

### Comments

```bash
# Add a comment
curl -s -X POST http://localhost:8080/api/km/workspaces/{ws_id}/documents/{doc_id}/comments \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content": "This section needs updating.", "chunk_index": 3}'

# List comments
curl -s http://localhost:8080/api/km/workspaces/{ws_id}/documents/{doc_id}/comments \
  -H "Authorization: Bearer $TOKEN"
```

### Annotations

```bash
# Add an annotation (anchored to a text range)
curl -s -X POST http://localhost:8080/api/km/workspaces/{ws_id}/documents/{doc_id}/annotations \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content": "Verify this claim", "start_offset": 120, "end_offset": 180, "label": "needs-review"}'

# List annotations
curl -s http://localhost:8080/api/km/workspaces/{ws_id}/documents/{doc_id}/annotations \
  -H "Authorization: Bearer $TOKEN"
```

### Reviews

```bash
# Submit a review
curl -s -X POST http://localhost:8080/api/km/workspaces/{ws_id}/documents/{doc_id}/reviews \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"status": "approved", "comment": "Content is accurate and up to date."}'

# List reviews
curl -s http://localhost:8080/api/km/workspaces/{ws_id}/documents/{doc_id}/reviews \
  -H "Authorization: Bearer $TOKEN"
```

Review statuses: `pending`, `approved`, `rejected`, `changes_requested`.

---

## Prompt Marketplace

Share, discover, and reuse prompt templates across the organization.

### List Prompts
```bash
curl -s http://localhost:8080/api/km/prompts \
  -H "Authorization: Bearer $TOKEN"
```

Supports query parameters: `?category=rag&sort=popular&page=1&per_page=20`.

### Create a Prompt
```bash
curl -s -X POST http://localhost:8080/api/km/prompts \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Summarizer", "description": "Concise document summarization", "content": "Summarize the following document in 3 bullet points:\n\n{context}", "category": "summarization", "tags": ["summary", "concise"]}'
```

### Get a Prompt
```bash
curl -s http://localhost:8080/api/km/prompts/{prompt_id} \
  -H "Authorization: Bearer $TOKEN"
```

### Update a Prompt
```bash
curl -s -X PUT http://localhost:8080/api/km/prompts/{prompt_id} \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"description": "Updated description", "content": "Updated template..."}'
```

### Delete a Prompt
```bash
curl -s -X DELETE http://localhost:8080/api/km/prompts/{prompt_id} \
  -H "Authorization: Bearer $TOKEN"
```

### Rate a Prompt
```bash
curl -s -X POST http://localhost:8080/api/km/prompts/{prompt_id}/rate \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"rating": 5, "comment": "Works great for technical docs"}'
```

Rating is an integer from 1 to 5.

### Fork a Prompt
```bash
curl -s -X POST http://localhost:8080/api/km/prompts/{prompt_id}/fork \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Summarizer (Thai)", "content": "สรุปเอกสารต่อไปนี้ใน 3 หัวข้อ:\n\n{context}"}'
```

Creates a new prompt based on an existing one, preserving a link to the original for attribution.

---

## Embedding Fine-Tuning

Fine-tune embedding models on your domain-specific data to improve retrieval quality.

### Datasets

```bash
# List datasets
curl -s http://localhost:8080/api/km/finetune/datasets \
  -H "Authorization: Bearer $TOKEN"

# Create a dataset
curl -s -X POST http://localhost:8080/api/km/finetune/datasets \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Legal QA Pairs", "description": "Query-document pairs from legal workspace", "workspace_id": "ws-uuid"}'

# Get a dataset
curl -s http://localhost:8080/api/km/finetune/datasets/{dataset_id} \
  -H "Authorization: Bearer $TOKEN"

# Delete a dataset
curl -s -X DELETE http://localhost:8080/api/km/finetune/datasets/{dataset_id} \
  -H "Authorization: Bearer $TOKEN"
```

### Jobs

```bash
# List fine-tuning jobs
curl -s http://localhost:8080/api/km/finetune/jobs \
  -H "Authorization: Bearer $TOKEN"

# Create a fine-tuning job
curl -s -X POST http://localhost:8080/api/km/finetune/jobs \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"dataset_id": "dataset-uuid", "base_model": "text-embedding-3-small", "epochs": 3, "learning_rate": 0.0001}'

# Get job status
curl -s http://localhost:8080/api/km/finetune/jobs/{job_id} \
  -H "Authorization: Bearer $TOKEN"

# Cancel a job
curl -s -X DELETE http://localhost:8080/api/km/finetune/jobs/{job_id} \
  -H "Authorization: Bearer $TOKEN"
```

Job statuses: `queued`, `running`, `completed`, `failed`, `cancelled`.

---

## Streaming Reranking Search

Run a hybrid search with streaming results via Server-Sent Events. Chunks are sent incrementally as they are retrieved and reranked, allowing the client to display results progressively.

```bash
curl -s -N -X POST http://localhost:8080/api/km/workspaces/{ws_id}/search-stream \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query": "contract termination clause", "top_k": 10}'
```

**SSE event types:**

- `event: chunk` -- A reranked chunk result with score, content, and document metadata.
- `event: summary` -- Final summary with total chunks, timing, and retrieval stats.
- `data: [DONE]` -- Stream complete.

Example stream:
```
event: chunk
data: {"chunk_id":"uuid","doc_id":"uuid","content":"The contract may be terminated...","score":0.94,"chunk_index":7,"doc_title":"Service Agreement"}

event: chunk
data: {"chunk_id":"uuid","doc_id":"uuid","content":"Termination notice must be...","score":0.87,"chunk_index":12,"doc_title":"Service Agreement"}

event: summary
data: {"total_chunks":10,"search_ms":45,"rerank_ms":120,"total_ms":165}

data: [DONE]
```

---

## Additional Endpoints

For complete endpoint documentation including the following, see [API_REFERENCE.md](API_REFERENCE.md):

- **API v2** — `/v2/chat/completions`, `/v2/search`, `/v2/models` with metadata and sources
- **WebSocket** — `/ws/chat` for real-time bidirectional chat
- **API Keys** — `/api/auth/api-keys` for key management
- **Webhooks** — `/api/km/webhooks` for event notifications
- **Background Jobs** — `/api/km/workspaces/{id}/jobs` with SSE streaming
- **Backup & Restore** — `/api/km/admin/backup`, `/api/km/admin/restore`
- **Vector Migration** — `/api/km/admin/vector-migration/*`
- **Rate Limit Stats** — `/api/km/admin/rate-limits/*`
- **Evaluation** — `/api/km/eval/query-sets/*`
- **A/B Testing** — `/api/km/ab-tests/*`
- **Plugins** — `/api/km/plugins/*`
- **Knowledge Graph** — `/api/km/workspaces/{id}/knowledge-graph`, `/api/km/workspaces/{id}/entities`
- **ACLs** — Workspace and document-level access control lists
- **Document Versioning** — Version history and diff endpoints
- **Inference Logs** — `/api/km/settings/inference-logs/*`
- **Chat Sessions** — `/api/chat/sessions/{id}/summary`

---

## Configuration

See `config/default.toml` for all available settings. Override with:
- `config/local.toml` for local development
- `THAIRAG_TIER` env var to select a tier preset (free/standard/premium)
- `THAIRAG__*` env vars for individual settings

### Security Headers

All API responses include OWASP-recommended security headers:

| Header | Value |
|--------|-------|
| `X-Content-Type-Options` | `nosniff` |
| `X-Frame-Options` | `DENY` |
| `X-XSS-Protection` | `1; mode=block` |
| `Referrer-Policy` | `strict-origin-when-cross-origin` |
| `Content-Security-Policy` | `default-src 'none'; script-src 'self'; ...` |
| `X-Request-Id` | UUID per request (for tracing) |

### CORS

By default, CORS is permissive (allows all origins). For production, configure allowed origins:

```
THAIRAG__SERVER__CORS_ORIGINS=["https://admin.example.com","https://chat.example.com"]
```

When `cors_origins` is set, only listed origins are allowed, credentials are permitted, and standard methods (GET, POST, PUT, DELETE, OPTIONS) are accepted.

### CSRF Protection

State-changing requests (POST, PUT, DELETE) on protected routes require either:
- **Bearer token auth** (inherently CSRF-safe — browsers don't auto-send Bearer headers), OR
- **`X-CSRF-Token` header** (for cookie-based auth flows)

The `csrf_token` is returned in the login response for use with cookie-based auth.

### Rate Limiting

- **Per-IP**: Token-bucket rate limiting (default: 10 req/s, burst 20)
- **Per-user**: Token-bucket rate limiting per authenticated user (same config)
- **Per-user concurrent**: Max 5 simultaneous requests per user
- Health and metrics endpoints are exempt from rate limiting

### Input Validation

- **Chat messages**: Max 50 messages per request (`THAIRAG__SERVER__MAX_CHAT_MESSAGES`)
- **Message length**: Max 32,000 chars per message (`THAIRAG__SERVER__MAX_MESSAGE_LENGTH`)
- **Upload size**: Max 50MB per file (`THAIRAG__DOCUMENT__MAX_UPLOAD_SIZE_MB`)

### Proxy Configuration

```
THAIRAG__SERVER__TRUST_PROXY=false    # Default: don't trust X-Forwarded-For
```

Only set `trust_proxy=true` when running behind a trusted reverse proxy (nginx, load balancer). When false, rate limiting uses the actual TCP peer IP, preventing header spoofing.

### Key Environment Variables

| Variable | Description |
|----------|-------------|
| `THAIRAG_TIER` | Config tier: `free`, `standard`, `premium` |
| `THAIRAG__AUTH__ENABLED` | Enable JWT authentication |
| `THAIRAG__AUTH__JWT_SECRET` | Secret key for JWT signing |
| `THAIRAG__AUTH__PASSWORD_MIN_LENGTH` | Minimum password length (default: 8) |
| `THAIRAG__AUTH__MAX_LOGIN_ATTEMPTS` | Failed logins before lockout (default: 5) |
| `THAIRAG__AUTH__LOCKOUT_DURATION_SECS` | Lockout duration in seconds (default: 300) |
| `THAIRAG__SERVER__CORS_ORIGINS` | Allowed CORS origins (JSON array, default: permissive) |
| `THAIRAG__ADMIN__EMAIL` | Super admin email (seeded on startup) |
| `THAIRAG__ADMIN__PASSWORD` | Super admin password (seeded on startup) |
| `THAIRAG__DATABASE__URL` | PostgreSQL or SQLite connection URL |
| `THAIRAG__PROVIDERS__LLM__BASE_URL` | LLM provider base URL |
| `THAIRAG__SERVER__TRUST_PROXY` | Trust X-Forwarded-For header (default: false) |
| `THAIRAG__SERVER__MAX_CHAT_MESSAGES` | Max messages per chat request (default: 50) |
| `THAIRAG__SERVER__MAX_MESSAGE_LENGTH` | Max chars per message (default: 32000) |
| `THAIRAG__DOCUMENT__MAX_UPLOAD_SIZE_MB` | Max upload file size in MB (default: 50) |

## Smoke Testing

Run the end-to-end smoke test script to verify all features:

```bash
# Against running instance (default: http://localhost:8080)
./scripts/smoke-test.sh

# Against custom URL
./scripts/smoke-test.sh http://thairag.staging.example.com:8080
```

The script runs 55+ checks covering health, auth, KM hierarchy, documents, chat, permissions, audit log, and cleanup. Requires `curl` and `jq`.

## Docker

```bash
# Basic stack (ThaiRAG + Postgres + Admin UI)
docker compose up -d

# With test identity provider (Keycloak + Open WebUI)
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up -d
```

See [OIDC_TESTING.md](OIDC_TESTING.md) for OIDC integration testing with Keycloak.
