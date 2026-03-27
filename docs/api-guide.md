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
