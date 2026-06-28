# Deployment Guide

## Quick Start (Pre-built Images)

The fastest way to run ThaiRAG — no source code or build tools needed.

```bash
# 1. Download the two files you need
curl -O https://raw.githubusercontent.com/Jay-GitHubs/ThaiRAG/main/docker-compose.registry.yml
curl -O https://raw.githubusercontent.com/Jay-GitHubs/ThaiRAG/main/.env.example

# 2. Configure
cp .env.example .env
# Edit .env — at minimum set POSTGRES_PASSWORD and THAIRAG__AUTH__JWT_SECRET

# 3. Start
docker compose -f docker-compose.registry.yml up -d

# 4. Verify
curl http://localhost:8080/health        # API
open http://localhost:8081               # Admin UI
```

Images are pulled from GitHub Container Registry by default. Published as **multi-arch manifests** covering both `linux/amd64` and `linux/arm64` — Docker selects the right variant automatically, so the same tag pulls on x86 Linux servers and Apple Silicon dev machines.

| Image | GHCR | Docker Hub |
|-------|------|------------|
| ThaiRAG API | `ghcr.io/jay-githubs/thairag` | `jdevspecialist/thairag` |
| Admin UI | `ghcr.io/jay-githubs/thairag-admin` | `jdevspecialist/thairag-admin` |

To use Docker Hub instead, set in `.env`:
```bash
THAIRAG_IMAGE=jdevspecialist/thairag
THAIRAG_ADMIN_IMAGE=jdevspecialist/thairag-admin
```

To pin a specific version (Git SHA):
```bash
THAIRAG_TAG=abc1234
```

---

## Docker Compose (Build from Source)

### Prerequisites
- Docker Engine 24+
- Docker Compose v2

### Services

> **Which compose file?** The base `docker-compose.yml` (below) includes redis, prometheus, and grafana. The pre-built registry stack `docker-compose.registry.yml` is leaner — only postgres, qdrant, thairag, and admin-ui (no redis/prometheus/grafana).

The `docker-compose.yml` defines these services:

| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| `thairag` | Built from Dockerfile | 8080 | ThaiRAG API server |
| `admin-ui` | Built from admin-ui/Dockerfile | 8081 | Admin dashboard |
| `postgres` | postgres:16-alpine | 5432 | Database |
| `qdrant` | qdrant/qdrant:latest | 6333, 6334 | Vector database |
| `redis` | redis:7-alpine | 6379 | Session store, embedding cache, job queue |
| `prometheus` | prom/prometheus | 9091 | Metrics collection (host 9091 → container 9090) |
| `grafana` | grafana/grafana | 3001 | Dashboards & visualization |
| `ollama` | ollama/ollama (commented) | 11435 | Local LLM (free tier) |
| `paddleocr` | Built from services/paddleocr-sidecar | 8086 | Deterministic Thai OCR tier (opt-in, `--profile ocr`) |

### Setup

1. **Create `.env` file:**

```bash
# Database
POSTGRES_DB=thairag
POSTGRES_USER=thairag
POSTGRES_PASSWORD=your-secure-password

# ThaiRAG
THAIRAG_TIER=standard
THAIRAG__AUTH__ENABLED=true
THAIRAG__AUTH__JWT_SECRET=your-jwt-secret-min-32-chars
THAIRAG__DATABASE__URL=postgresql://thairag:your-secure-password@postgres:5432/thairag

# Provider API keys (standard/premium tier)
THAIRAG__PROVIDERS__LLM__API_KEY=sk-ant-...
THAIRAG__PROVIDERS__EMBEDDING__API_KEY=sk-...
THAIRAG__PROVIDERS__RERANKER__API_KEY=...

# Optional: seed super admin
THAIRAG__ADMIN__EMAIL=admin@yourcompany.com
THAIRAG__ADMIN__PASSWORD=SecurePassword123

# MCP Connectors (optional)
# THAIRAG__MCP__ENABLED=true

# Redis (for scaling)
# THAIRAG__SESSION__BACKEND=redis
# THAIRAG__EMBEDDING_CACHE__BACKEND=redis
# THAIRAG__JOB_QUEUE__BACKEND=redis
# THAIRAG__REDIS__URL=redis://redis:6379

# OpenTelemetry (optional)
# THAIRAG__OTEL__ENABLED=true
# THAIRAG__OTEL__ENDPOINT=http://localhost:4317
```

2. **Start services:**

```bash
# Core services only (API + Admin UI + PostgreSQL + Qdrant)
docker compose up --build -d

# Full stack with Keycloak (OIDC SSO)
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up --build -d
```

3. **Verify:**

```bash
# Health check
curl http://localhost:8080/health?deep=true

# Admin UI
open http://localhost:8081

# If using full stack (only present when -f docker-compose.test-idp.yml is included):
# Keycloak:   http://localhost:9090  (admin / admin)
```

> **Note:** Keycloak is defined only in `docker-compose.test-idp.yml` (on port 9090). The base `docker-compose.yml` and the registry stack do not include Keycloak — it appears only when you add `-f docker-compose.test-idp.yml`.

4. **Stop services:**

```bash
# Core only
docker compose down

# Full stack
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml down

# Full stack + remove all data (clean restart)
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml down -v
```

### Using Ollama (Free Tier)

For the free tier with local models:

1. Uncomment the `ollama` service in `docker-compose.yml`
2. Set `THAIRAG_TIER=free`
3. Pull a model after startup:

```bash
docker compose exec ollama ollama pull llama3.2
```

**macOS with Metal GPU:** Use native Ollama instead of Docker for GPU acceleration:

```bash
# Install Ollama natively
brew install ollama
ollama serve &
ollama pull llama3.2

# Set ThaiRAG to use host Ollama
THAIRAG__PROVIDERS__LLM__BASE_URL=http://host.docker.internal:11435
```

### Chat UI

The first-party **chat-ui** (`chat-ui/`, port 8082) is the recommended end-user
interface and ships with the stack. It provides streaming chat, durable history,
native citations + inline images, scope selector, file upload, and native + SSO
login.

### Connecting an external OpenAI-compatible client

Any OpenAI-compatible client can use ThaiRAG's `/v1` surface as a drop-in
backend. Point it at `http://<host>:8080/v1` and authenticate with a static API
key (set `THAIRAG_API_KEY` in `.env`, sent as `X-API-Key` or as the bearer
token, depending on the client). Allow long timeouts — the multi-agent pipeline
can take 60+ seconds. See the [Integration Guide](INTEGRATION_GUIDE.md) for the
full client setup, including how API-key scope differs from per-user (chat-ui)
permission enforcement.

### PaddleOCR Sidecar (Optional — Deterministic Thai OCR)

The `paddleocr` service is an optional, deterministic OCR tier for the document
pipeline. It wraps PaddleOCR's Thai `th_PP-OCRv5_mobile_rec` model behind a small
FastAPI HTTP service (port 8086) and transcribes OCR-needing PDF pages (scanned or
corrupted-CMap text) locally — faster, with no hallucination, and more accurate on
Thai than the vision LLM. When it is not enabled, OCR-needing pages fall back to the
vision LLM. The vision LLM is still used for figure *description* either way.

The service is gated behind the `ocr` compose profile, so it does **not** start by
default. To enable it:

1. Start the sidecar:

```bash
docker compose --profile ocr up -d --build paddleocr
```

2. Point ThaiRAG at it (env or `.env` on the `thairag` service), then restart:

```bash
THAIRAG__DOCUMENT__OCR_SIDECAR_URL=http://paddleocr:8086
```

With the URL empty or unset, the OCR tier is off and PDF extraction is unchanged.
The same value is also tunable at runtime via the `document.ocr_sidecar_url` setting
in the Admin UI (km-store). See `services/paddleocr-sidecar/README.md` for build and
endpoint details.

### Persistent Volumes

| Volume | Purpose |
|--------|---------|
| `postgres-data` | PostgreSQL database |
| `thairag-data` | Tantivy BM25 search index (disk-persisted via MmapDirectory) |
| `qdrant-data` | Qdrant vector storage |
| `redis-data` | Redis persistence |
| `prometheus-data` | Prometheus metrics storage |
| `grafana-data` | Grafana dashboard data |
| `ollama-models` | Ollama model files |

> **Tantivy auto-recovery:** On startup, ThaiRAG automatically detects and removes stale Tantivy writer lock files (from previous crashes or ungraceful shutdowns). If the Tantivy index is empty but the database has stored chunks, the index is rebuilt automatically in batches. No manual intervention is needed after a container restart.

### Docker Build Details

The Dockerfile uses a multi-stage build:

1. **Builder stage** (`rust:1.88-bookworm`):
   - Copies Cargo manifests first for dependency caching
   - Creates stub source files for workspace resolution
   - Builds dependencies (cached layer)
   - Copies real source and rebuilds
   - Touches all `.rs` files to invalidate fingerprint cache

2. **Runtime stage** (`debian:bookworm-slim`):
   - Minimal image with only `ca-certificates`
   - Copies binary, config files, and prompt templates
   - Creates `/data` directory for persistent state
   - Exposes port 8080

### Nginx SSE Configuration

The admin UI's test-query-stream endpoint uses Server-Sent Events (SSE) for real-time pipeline progress. If you run nginx as a reverse proxy (including the admin-ui Docker container's built-in nginx), the following directives are required for SSE to work correctly:

```nginx
location /api/ {
    proxy_pass http://thairag:8080;

    # SSE requires these settings to prevent buffering
    proxy_buffering off;
    proxy_cache off;
    chunked_transfer_encoding off;
    proxy_http_version 1.1;
    proxy_set_header Connection '';
}
```

Without these settings, nginx buffers the SSE stream and the frontend will not receive pipeline progress events until the entire response completes (or times out).

---

## Local Development

### Prerequisites
- Rust 1.88+ (edition 2024)
- Node.js 20+ with npm
- Optional: PostgreSQL 16, Qdrant, Ollama

### Backend

```bash
# Free tier (no external dependencies except Ollama)
THAIRAG_TIER=free cargo run -p thairag-api

# Standard tier (requires API keys + PostgreSQL + Qdrant)
THAIRAG_TIER=standard \
THAIRAG__AUTH__ENABLED=true \
THAIRAG__DATABASE__URL=postgresql://user:pass@localhost:5432/thairag \
THAIRAG__PROVIDERS__LLM__API_KEY=sk-ant-... \
THAIRAG__PROVIDERS__EMBEDDING__API_KEY=sk-... \
cargo run -p thairag-api
```

### Admin UI

```bash
cd admin-ui
npm install
npm run dev  # Starts on http://localhost:5173
```

The dev server proxies API requests to `http://localhost:8080`.

### Running Tests

```bash
# All backend tests
cargo test

# Specific crate
cargo test -p thairag-api

# Admin UI type check
cd admin-ui && npx tsc --noEmit

# Playwright e2e tests
cd admin-ui && npx playwright test
```

---

## Configuration Reference

### Server

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `server.host` | `THAIRAG__SERVER__HOST` | `0.0.0.0` | Bind address |
| `server.port` | `THAIRAG__SERVER__PORT` | `8080` | HTTP port |
| `server.shutdown_timeout_secs` | `THAIRAG__SERVER__SHUTDOWN_TIMEOUT_SECS` | `30` | Graceful shutdown timeout |
| `server.rate_limit.enabled` | `THAIRAG__SERVER__RATE_LIMIT__ENABLED` | `true` | Enable rate limiting |
| `server.rate_limit.requests_per_second` | `THAIRAG__SERVER__RATE_LIMIT__REQUESTS_PER_SECOND` | `10` | Rate limit per IP |
| `server.rate_limit.burst_size` | `THAIRAG__SERVER__RATE_LIMIT__BURST_SIZE` | `20` | Burst allowance |

### Authentication

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `auth.enabled` | `THAIRAG__AUTH__ENABLED` | `false` | Enable authentication |
| `auth.jwt_secret` | `THAIRAG__AUTH__JWT_SECRET` | `dev-secret-change-me` | JWT signing secret |
| `auth.token_expiry_hours` | `THAIRAG__AUTH__TOKEN_EXPIRY_HOURS` | `24` | Token expiration |

### Database

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `database.url` | `THAIRAG__DATABASE__URL` | (empty = SQLite) | PostgreSQL connection URL |
| `database.max_connections` | `THAIRAG__DATABASE__MAX_CONNECTIONS` | `5` | Connection pool size |

### Providers

| Key | Env Override | Description |
|-----|-------------|-------------|
| `providers.llm.kind` | `THAIRAG__PROVIDERS__LLM__KIND` | `ollama`, `claude`, `openai`, `openai_compatible`, `gemini` |
| `providers.llm.model` | `THAIRAG__PROVIDERS__LLM__MODEL` | Model name |
| `providers.llm.api_key` | `THAIRAG__PROVIDERS__LLM__API_KEY` | API key |
| `providers.llm.base_url` | `THAIRAG__PROVIDERS__LLM__BASE_URL` | Base URL (Ollama/OpenAI) |
| `providers.embedding.kind` | `THAIRAG__PROVIDERS__EMBEDDING__KIND` | `fastembed`, `openai`, `ollama`, `cohere` |
| `providers.embedding.model` | `THAIRAG__PROVIDERS__EMBEDDING__MODEL` | Model name |
| `providers.embedding.dimension` | `THAIRAG__PROVIDERS__EMBEDDING__DIMENSION` | Vector dimension |
| `providers.vector_store.kind` | `THAIRAG__PROVIDERS__VECTOR_STORE__KIND` | `in_memory`, `qdrant`, `pgvector`, `chroma_db`, `pinecone`, `weaviate`, `milvus` |
| `providers.vector_store.url` | `THAIRAG__PROVIDERS__VECTOR_STORE__URL` | Qdrant gRPC URL |
| `providers.reranker.kind` | `THAIRAG__PROVIDERS__RERANKER__KIND` | `passthrough`, `cohere`, `jina` |

### Search

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `search.top_k` | `THAIRAG__SEARCH__TOP_K` | `10` | Results per search source |
| `search.rerank_top_k` | `THAIRAG__SEARCH__RERANK_TOP_K` | `5` | Final results after reranking |
| `search.rrf_k` | `THAIRAG__SEARCH__RRF_K` | `60` | RRF fusion parameter |
| `search.vector_weight` | `THAIRAG__SEARCH__VECTOR_WEIGHT` | `0.6` | Vector search weight |
| `search.text_weight` | `THAIRAG__SEARCH__TEXT_WEIGHT` | `0.4` | BM25 search weight |

### Chat Pipeline — Context Compaction & Personal Memory

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `chat_pipeline.context_compaction_enabled` | `THAIRAG__CHAT_PIPELINE__CONTEXT_COMPACTION_ENABLED` | `false` | Auto-summarize older messages when near context limit |
| `chat_pipeline.model_context_window` | `THAIRAG__CHAT_PIPELINE__MODEL_CONTEXT_WINDOW` | `0` | Context window in tokens (0 = auto-detect) |
| `chat_pipeline.compaction_threshold` | `THAIRAG__CHAT_PIPELINE__COMPACTION_THRESHOLD` | `0.8` | Trigger compaction at this fraction of context window |
| `chat_pipeline.compaction_keep_recent` | `THAIRAG__CHAT_PIPELINE__COMPACTION_KEEP_RECENT` | `6` | Recent messages to keep intact |
| `chat_pipeline.personal_memory_enabled` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_ENABLED` | `false` | Per-user memory across sessions |
| `chat_pipeline.personal_memory_top_k` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_TOP_K` | `5` | Memories retrieved per query |
| `chat_pipeline.personal_memory_max_per_user` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_MAX_PER_USER` | `200` | Max memories stored per user |
| `chat_pipeline.personal_memory_decay_factor` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_DECAY_FACTOR` | `0.95` | Relevance decay rate (0.0–1.0) |
| `chat_pipeline.personal_memory_min_relevance` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_MIN_RELEVANCE` | `0.1` | Prune memories below this score |
| `chat_pipeline.live_retrieval_enabled` | `THAIRAG__CHAT_PIPELINE__LIVE_RETRIEVAL_ENABLED` | `false` | Auto-fetch from MCP connectors when KB has no results |
| `chat_pipeline.live_retrieval_timeout_secs` | `THAIRAG__CHAT_PIPELINE__LIVE_RETRIEVAL_TIMEOUT_SECS` | `15` | Overall timeout for live retrieval stage |
| `chat_pipeline.live_retrieval_max_connectors` | `THAIRAG__CHAT_PIPELINE__LIVE_RETRIEVAL_MAX_CONNECTORS` | `3` | Max connectors to query in parallel |
| `chat_pipeline.live_retrieval_max_content_chars` | `THAIRAG__CHAT_PIPELINE__LIVE_RETRIEVAL_MAX_CONTENT_CHARS` | `30000` | Max total chars fetched from all connectors |

### MCP Connectors

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `mcp.enabled` | `THAIRAG__MCP__ENABLED` | `false` | Enable MCP connector integration |
| `mcp.max_concurrent_syncs` | `THAIRAG__MCP__MAX_CONCURRENT_SYNCS` | `3` | Max concurrent sync operations |
| `mcp.connect_timeout_secs` | `THAIRAG__MCP__CONNECT_TIMEOUT_SECS` | `30` | MCP server connection timeout |
| `mcp.read_timeout_secs` | `THAIRAG__MCP__READ_TIMEOUT_SECS` | `120` | Resource read timeout |
| `mcp.max_resource_size_bytes` | `THAIRAG__MCP__MAX_RESOURCE_SIZE_BYTES` | `52428800` | Max resource size (50MB) |
| `mcp.sync_retry_max_attempts` | `THAIRAG__MCP__SYNC_RETRY_MAX_ATTEMPTS` | `3` | Retry attempts on sync failure |
| `mcp.sync_retry_base_delay_secs` | `THAIRAG__MCP__SYNC_RETRY_BASE_DELAY_SECS` | `2` | Base delay for exponential backoff |
| `mcp.sync_retry_max_delay_secs` | `THAIRAG__MCP__SYNC_RETRY_MAX_DELAY_SECS` | `60` | Max retry delay |

### Document Processing

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `document.max_chunk_size` | `THAIRAG__DOCUMENT__MAX_CHUNK_SIZE` | `512` | Chunk size in tokens |
| `document.chunk_overlap` | `THAIRAG__DOCUMENT__CHUNK_OVERLAP` | `64` | Overlap between chunks |
| `document.pdf_vision_concurrency` | `THAIRAG__DOCUMENT__PDF_VISION_CONCURRENCY` | `2` | Max per-page vision OCR calls in flight at once (`1` = sequential) |
| `document.ocr_sidecar_url` | `THAIRAG__DOCUMENT__OCR_SIDECAR_URL` | (empty = off) | Base URL of the deterministic OCR sidecar (e.g. `http://paddleocr:8086`); when set, OCR-needing PDF pages prefer it over the vision LLM |

### Session

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `session.backend` | `THAIRAG__SESSION__BACKEND` | `memory` | Backend: `memory` or `redis` |
| `session.max_history` | `THAIRAG__SESSION__MAX_HISTORY` | `50` | Max messages stored per session |
| `session.stale_timeout_secs` | `THAIRAG__SESSION__STALE_TIMEOUT_SECS` | `3600` | Auto-expire inactive sessions |

### Embedding Cache

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `embedding_cache.backend` | `THAIRAG__EMBEDDING_CACHE__BACKEND` | `memory` | Backend: `memory` or `redis` |
| `embedding_cache.max_entries` | `THAIRAG__EMBEDDING_CACHE__MAX_ENTRIES` | `10000` | Max cached embeddings (memory backend) |
| `embedding_cache.ttl_secs` | `THAIRAG__EMBEDDING_CACHE__TTL_SECS` | `3600` | Cache entry TTL in seconds |

### Job Queue

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `job_queue.backend` | `THAIRAG__JOB_QUEUE__BACKEND` | `memory` | Backend: `memory` or `redis` |
| `job_queue.retention_secs` | `THAIRAG__JOB_QUEUE__RETENTION_SECS` | `86400` | How long to retain completed job records |

### Redis

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `redis.url` | `THAIRAG__REDIS__URL` | `redis://127.0.0.1:6379` | Redis connection URL |

### OpenTelemetry

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `otel.enabled` | `THAIRAG__OTEL__ENABLED` | `false` | Enable OpenTelemetry tracing |
| `otel.endpoint` | `THAIRAG__OTEL__ENDPOINT` | `http://localhost:4317` | OTLP gRPC collector endpoint |
| `otel.service_name` | `THAIRAG__OTEL__SERVICE_NAME` | `thairag` | Service name reported to collector |

### Knowledge Graph

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `knowledge_graph.enabled` | `THAIRAG__KNOWLEDGE_GRAPH__ENABLED` | `false` | Enable knowledge graph features |
| `knowledge_graph.extract_on_ingest` | `THAIRAG__KNOWLEDGE_GRAPH__EXTRACT_ON_INGEST` | `false` | Auto-extract entities/relations on document ingest |

### Plugins

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `plugins.enabled_plugins` | `THAIRAG__PLUGINS__ENABLED_PLUGINS` | `[]` | Default enabled plugin names at startup. Operators can override per-deployment by toggling on `/plugins` (super-admin UI); changes persist to the KV store under `plugins.enabled` and survive restart |

Built-in plugins shipped with every deployment:
- `metadata-strip` — DocumentPlugin; strips HTML/XML `<script>`, `<style>`, `<meta>`, `<link>` tags from document content before chunking.
- `query-expansion` — SearchPlugin; expands user queries with a synonym table (English-only). Fires on both `/v2/search`, `/api/km/.../test-query`, and the main `/v1`/`/v2` chat retrieval path.
- `summary-chunk` — ChunkPlugin; prepends a one-line `[Summary: ...]` header to each chunk.

### Guardrails

Deterministic content safety for chat input and output. All detectors default to off — operators opt in per detector. Streaming output is filtered with a sliding-window hold-back so matches are redacted **before** transmission; see `docs/STREAMING_GUARDRAILS_DESIGN.md`.

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `guardrails.max_query_chars` | `THAIRAG__GUARDRAILS__MAX_QUERY_CHARS` | `8000` | Reject inbound queries longer than this. Always-on once any guardrail is configured. |
| `guardrails.max_response_chars` | `THAIRAG__GUARDRAILS__MAX_RESPONSE_CHARS` | `64000` | Cap on the response length that output detectors scan. Redaction still applies to the full response; only the detector input is bounded. |
| `guardrails.detect_thai_id` | `THAIRAG__GUARDRAILS__DETECT_THAI_ID` | `false` | Thai national ID with mod-11 checksum. Critical severity — always blocks on input. |
| `guardrails.detect_thai_phone` | `THAIRAG__GUARDRAILS__DETECT_THAI_PHONE` | `false` | Thai phone numbers (`+66` and `0X-XXX-XXXX` formats). |
| `guardrails.detect_email` | `THAIRAG__GUARDRAILS__DETECT_EMAIL` | `false` | Email addresses. |
| `guardrails.detect_credit_card` | `THAIRAG__GUARDRAILS__DETECT_CREDIT_CARD` | `false` | Credit-card numbers (Luhn-validated to suppress false positives). Critical severity. |
| `guardrails.detect_secrets` | `THAIRAG__GUARDRAILS__DETECT_SECRETS` | `false` | API secrets — AWS keys (`AKIA…`/`ASIA…`), JWTs, GitHub PATs (`gh[psoru]_…`), and generic `key=` / `Bearer …` tokens with ≥ 24-char suffix. |
| `guardrails.detect_prompt_injection` | `THAIRAG__GUARDRAILS__DETECT_PROMPT_INJECTION` | `false` | Multilingual jailbreak / instruction-override pattern set (English + Thai). |
| `guardrails.blocklist_phrases` | `THAIRAG__GUARDRAILS__BLOCKLIST_PHRASES` | `[]` | Case-insensitive substring matches. Compiled as one combined `(?i)` regex so byte offsets stay correct on non-ASCII text. |
| `guardrails.input_on_violation` | `THAIRAG__GUARDRAILS__INPUT_ON_VIOLATION` | `"block"` | `"block"` or `"sanitize"`. Critical violations always block regardless of this setting. |
| `guardrails.output_on_violation` | `THAIRAG__GUARDRAILS__OUTPUT_ON_VIOLATION` | `"redact"` | `"block"`, `"redact"`, or `"regenerate"`. In streaming mode `block` / `regenerate` are downgraded to redact because content has already started flowing. |
| `guardrails.redaction_token` | `THAIRAG__GUARDRAILS__REDACTION_TOKEN` | `"[REDACTED]"` | Replacement token inserted in place of matched spans. |
| `guardrails.fail_open` | `THAIRAG__GUARDRAILS__FAIL_OPEN` | `true` | If a detector errors, pass through (`true`) or treat as a violation (`false`). Honored by both the non-streaming and streaming paths. |
| `guardrails.streaming_window_chars` | `THAIRAG__GUARDRAILS__STREAMING_WINDOW_CHARS` | `256` | Sliding-window size for streaming output. Bigger = catches longer secrets (e.g. JWT prefixes) at the cost of TTFB. The default covers every bounded pattern in the current detector set; raise it for stricter JWT prevention. |

Operators can also tune detectors live from `/guardrails` (super-admin UI) without editing config — those changes go through the same `GuardrailsConfig` and persist via the settings KV.

### Multi-tenancy

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `multi_tenancy.enabled` | `THAIRAG__MULTI_TENANCY__ENABLED` | `false` | Enable multi-tenant isolation |
| `multi_tenancy.default_quota_docs` | `THAIRAG__MULTI_TENANCY__DEFAULT_QUOTA_DOCS` | `1000` | Default document quota per tenant |
| `multi_tenancy.default_quota_storage_mb` | `THAIRAG__MULTI_TENANCY__DEFAULT_QUOTA_STORAGE_MB` | `5120` | Default storage quota per tenant (MB) |
| `multi_tenancy.default_quota_users` | `THAIRAG__MULTI_TENANCY__DEFAULT_QUOTA_USERS` | `50` | Default user quota per tenant |

To enable multi-tenancy:

1. Set `THAIRAG__MULTI_TENANCY__ENABLED=true` in your environment or config file.
2. Restart the ThaiRAG service.
3. Provision tenants via the Admin UI (Settings > Multi-tenancy) or the API:

```bash
curl -X POST http://localhost:8080/api/km/tenants \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Acme Corp",
    "slug": "acme",
    "quota": {
      "max_documents": 5000,
      "max_storage_mb": 10240,
      "max_users": 100
    }
  }'
```

Each tenant receives its own isolated KM hierarchy (Org > Dept > Workspace > Documents). Users are assigned to tenants, and all queries are scoped to the tenant boundary. Super admins can manage all tenants; tenant admins can manage only their own.

### Search Analytics

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `search_analytics.enabled` | `THAIRAG__SEARCH_ANALYTICS__ENABLED` | `true` | Enable search analytics event recording |
| `search_analytics.retention_days` | `THAIRAG__SEARCH_ANALYTICS__RETENTION_DAYS` | `90` | Days to retain analytics data before automatic cleanup |

When enabled, every RAG query records an analytics event (fire-and-forget via `tokio::spawn` to avoid impacting response latency). Events capture query text, result count, response time, and whether any results were returned. The Admin UI provides a Search Analytics dashboard showing popular queries, zero-result queries, and summary statistics with date range filtering.

### Personal Memory

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `chat_pipeline.personal_memory_enabled` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_ENABLED` | `false` | Enable per-user memory across sessions |
| `chat_pipeline.personal_memory_max_per_user` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_MAX_PER_USER` | `200` | Maximum memory entries stored per user |
| `chat_pipeline.personal_memory_top_k` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_TOP_K` | `5` | Number of memories retrieved per query |
| `chat_pipeline.personal_memory_decay_factor` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_DECAY_FACTOR` | `0.95` | Relevance decay rate (0.0-1.0) |
| `chat_pipeline.personal_memory_min_relevance` | `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_MIN_RELEVANCE` | `0.1` | Prune memories below this relevance score |

To enable personal memory, set `THAIRAG__CHAT_PIPELINE__PERSONAL_MEMORY_ENABLED=true`. The system automatically extracts user preferences and facts from conversations and stores them per user. On subsequent queries, relevant memories are retrieved and injected into the LLM context for personalized responses. Memories are pruned when they exceed `max_per_user` or fall below `min_relevance` after decay.

### Embedding Fine-tuning

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `embedding_finetune.enabled` | `THAIRAG__EMBEDDING_FINETUNE__ENABLED` | `false` | Enable embedding fine-tuning features |
| `embedding_finetune.backend` | `THAIRAG__EMBEDDING_FINETUNE__BACKEND` | `local` | Fine-tuning backend: `local` or `remote` |

Embedding fine-tuning allows training domain-specific embedding models from your document corpus. When enabled, the Admin UI exposes a Fine-tuning page where you can create training datasets from existing documents, launch fine-tuning jobs, and track their progress. The `local` backend runs fine-tuning on the same machine; `remote` delegates to an external training service.

Fine-tuning is disabled by default because it is compute-intensive and requires sufficient GPU/CPU resources. Enable it only when you have a meaningful document corpus and want to improve retrieval quality for domain-specific terminology.

### Qdrant Dimension Auto-detection

When using Qdrant as the vector store, ThaiRAG automatically detects and handles embedding dimension mismatches. If you switch embedding models (e.g., from `text-embedding-3-small` at 1536 dimensions to `text-embedding-3-large` at 3072 dimensions), the following happens on startup:

1. ThaiRAG queries the existing Qdrant collection for its configured vector dimension.
2. If the dimension does not match the current embedding model's output dimension, ThaiRAG logs a warning and recreates the collection with the correct dimension.
3. All existing vectors are invalidated (they were generated with a different model and are incompatible).
4. If the Tantivy BM25 index contains chunks, those chunks are re-embedded in batches using the new model and re-indexed into Qdrant.

This means switching embedding models is a safe operation -- no manual Qdrant administration is needed -- but be aware that re-embedding a large corpus takes time and incurs API costs if using a hosted embedding provider.

---

## Production Checklist

- [ ] Set `THAIRAG__AUTH__ENABLED=true`
- [ ] Set a strong `THAIRAG__AUTH__JWT_SECRET` (32+ characters)
- [ ] Use PostgreSQL instead of SQLite for the database
- [ ] Use Qdrant instead of in-memory vector store
- [ ] Set `THAIRAG__SERVER__CORS_ORIGINS` to restrict allowed origins
- [ ] Enable MCP if using external connectors (`THAIRAG__MCP__ENABLED=true`)
- [ ] Configure rate limiting appropriately for your traffic
- [ ] Set up Prometheus scraping from `/metrics`
- [ ] Seed a super admin account via env vars
- [ ] Use Docker secrets or a vault for API keys (avoid `.env` in production)
- [ ] Set `RUST_LOG=info` (or `warn` for less output)
- [ ] Mount persistent volumes for postgres-data, qdrant-data, and thairag-data (Tantivy index auto-rebuilds from DB if volume is lost)
- [ ] Set up health check monitoring on `/health?deep=true`
- [ ] Verify SSE streaming works through any reverse proxies/load balancers (test the `/api/km/test-query-stream` endpoint)
- [ ] Configure Chat Pipeline LLM mode (Use Chat LLM / Shared / Per-Agent) via Admin UI Settings
- [ ] Configure Redis for session/cache/job queue if scaling horizontally
- [ ] Set up Grafana dashboards for monitoring
- [ ] Configure OpenTelemetry if using distributed tracing
- [ ] Enable knowledge graph extraction if needed
- [ ] Configure backup schedule

---

## CI/CD

The project includes a GitHub Actions workflow (`.github/workflows/ci.yml`):

1. **Format check** — `cargo fmt --check`
2. **Clippy linting** — `cargo clippy -- -D warnings`
3. **Tests** — `cargo test`
4. **Docker build** — Verifies the Docker image builds successfully

---

## Deployment CLI

ThaiRAG includes a deployment CLI (`thairag`) for operational tasks. The CLI connects to a running ThaiRAG instance and provides commands for health monitoring, configuration inspection, and backup management.

### Health Check

The health endpoint needs no CLI — hit it directly (ideal for load-balancer probes, monitoring, and CI/CD):

```bash
# Basic health (returns status ok/degraded/unhealthy)
curl -fsS "http://localhost:8080/health"

# Deep health — probes all configured providers (embedding, vector store, LLM)
curl -fsS "http://localhost:8080/health?deep=true"
```

Use `?deep=true` in monitoring scripts and CI/CD pipelines to verify all dependencies are reachable. For a CLI summary of the running instance, use `thairag status` (below).

### Status

```bash
# Show service status: uptime, active sessions, document count, index health
thairag status
```

Displays a summary of the running instance including version, tier, uptime, number of active sessions, total documents indexed, and Tantivy/Qdrant index health.

### Configuration

```bash
# Show the resolved provider configuration
thairag config show

# Get a single setting value by dot-separated key (e.g. "llm.model")
thairag config get llm.model
```

`config show` fetches the resolved provider configuration from the running instance. Sensitive values (API keys, JWT secrets, database passwords) are redacted in the output.

### Backup

```bash
# Create a backup (writes a single .zip file)
thairag backup create

# Create a backup with a custom output file
thairag backup create --output /backups/thairag-$(date +%Y%m%d).zip

# Preview what would be included in a backup (no file written)
thairag backup preview
```

A backup is a single `.zip` archive containing settings, users, documents, and org structure. When `--output` is omitted, the file is written to the current directory as `thairag-backup-<timestamp>.zip`. Always create a backup before Docker volume rebuilds or embedding model changes.

### Deploy

```bash
# Generate a docker-compose file for the standard profile in the current directory
thairag deploy

# Generate into a specific output directory
thairag deploy --output ./deploy

# Generate for a specific profile (free, standard, premium)
thairag deploy --profile free
```

The `deploy` command **generates a deployment compose file** (`docker-compose.<profile>.yml`) for the chosen `--profile` into `--output <dir>` (default: current directory). It does not pull images, restart services, or perform rolling updates/rollbacks — after generating the file, copy `.env.example` to `.env`, configure it, then run `docker compose -f docker-compose.<profile>.yml up -d` yourself.

### CI/CD Integration

Use the CLI in CI/CD pipelines for automated verification and operations:

```bash
# Post-deployment verification (deep health probe)
curl -fsS "http://localhost:8080/health?deep=true" || { echo "Deployment health check failed"; exit 1; }

# Scheduled backup (e.g., daily cron job)
thairag backup create --output /backups/thairag-$(date +%Y%m%d).zip

# Inspect resolved provider configuration
thairag config show
```
