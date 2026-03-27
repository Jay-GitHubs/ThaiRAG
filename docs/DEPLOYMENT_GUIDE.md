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

Images are pulled from GitHub Container Registry by default:
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

The `docker-compose.yml` defines these services:

| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| `thairag` | Built from Dockerfile | 8080 | ThaiRAG API server |
| `admin-ui` | Built from admin-ui/Dockerfile | 8081 | Admin dashboard |
| `postgres` | postgres:16-alpine | 5432 | Database |
| `qdrant` | qdrant/qdrant:latest | 6333, 6334 | Vector database |
| `redis` | redis:7-alpine | 6379 | Session store, embedding cache, job queue |
| `prometheus` | prom/prometheus | 9090 | Metrics collection |
| `grafana` | grafana/grafana | 3001 | Dashboards & visualization |
| `ollama` | ollama/ollama (commented) | 11434 | Local LLM (free tier) |
| `open-webui` | open-webui (commented) | 3000 | Chat interface (optional) |

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

# Full stack with Keycloak (OIDC) + Open WebUI
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up --build -d
```

3. **Verify:**

```bash
# Health check
curl http://localhost:8080/health?deep=true

# Admin UI
open http://localhost:8081

# If using full stack:
# Keycloak:   http://localhost:9090  (admin / admin)
# Open WebUI: http://localhost:3000  (login via Keycloak SSO)
```

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
THAIRAG__PROVIDERS__LLM__BASE_URL=http://host.docker.internal:11434
```

### Adding Open WebUI

Uncomment the `open-webui` service in `docker-compose.yml`. Configure it to point at ThaiRAG's OpenAI-compatible API:

```yaml
open-webui:
  image: ghcr.io/open-webui/open-webui:v0.8.10
  ports:
    - "3000:8080"
  environment:
    OPENAI_API_BASE_URLS: "http://thairag:8080/v1"
    OPENAI_API_KEYS: "sk-thairag-openwebui"
    # Forward real user identity to ThaiRAG for per-user permission enforcement
    ENABLE_FORWARD_USER_INFO_HEADERS: "true"
    # Increase timeout for multi-agent pipeline (can take 60+ seconds)
    AIOHTTP_CLIENT_TIMEOUT: "600"
  depends_on:
    - thairag
```

See [Integration Guide](INTEGRATION_GUIDE.md) for detailed Open WebUI setup including per-user permission enforcement.

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
| `providers.llm.kind` | `THAIRAG__PROVIDERS__LLM__KIND` | `ollama`, `claude`, `openai` |
| `providers.llm.model` | `THAIRAG__PROVIDERS__LLM__MODEL` | Model name |
| `providers.llm.api_key` | `THAIRAG__PROVIDERS__LLM__API_KEY` | API key |
| `providers.llm.base_url` | `THAIRAG__PROVIDERS__LLM__BASE_URL` | Base URL (Ollama/OpenAI) |
| `providers.embedding.kind` | `THAIRAG__PROVIDERS__EMBEDDING__KIND` | `fastembed`, `openai` |
| `providers.embedding.model` | `THAIRAG__PROVIDERS__EMBEDDING__MODEL` | Model name |
| `providers.embedding.dimension` | `THAIRAG__PROVIDERS__EMBEDDING__DIMENSION` | Vector dimension |
| `providers.vector_store.kind` | `THAIRAG__PROVIDERS__VECTOR_STORE__KIND` | `in_memory`, `qdrant` |
| `providers.vector_store.url` | `THAIRAG__PROVIDERS__VECTOR_STORE__URL` | Qdrant gRPC URL |
| `providers.reranker.kind` | `THAIRAG__PROVIDERS__RERANKER__KIND` | `passthrough`, `cohere` |

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
| `mcp.max_resource_size_bytes` | `THAIRAG__MCP__MAX_RESOURCE_SIZE_BYTES` | `10485760` | Max resource size (10MB) |
| `mcp.sync_retry_max_attempts` | `THAIRAG__MCP__SYNC_RETRY_MAX_ATTEMPTS` | `3` | Retry attempts on sync failure |
| `mcp.sync_retry_base_delay_secs` | `THAIRAG__MCP__SYNC_RETRY_BASE_DELAY_SECS` | `2` | Base delay for exponential backoff |
| `mcp.sync_retry_max_delay_secs` | `THAIRAG__MCP__SYNC_RETRY_MAX_DELAY_SECS` | `60` | Max retry delay |

### Document Processing

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `document.max_chunk_size` | `THAIRAG__DOCUMENT__MAX_CHUNK_SIZE` | `512` | Chunk size in tokens |
| `document.chunk_overlap` | `THAIRAG__DOCUMENT__CHUNK_OVERLAP` | `64` | Overlap between chunks |

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
| `embedding_cache.ttl_secs` | `THAIRAG__EMBEDDING_CACHE__TTL_SECS` | `86400` | Cache entry TTL in seconds |

### Job Queue

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `job_queue.backend` | `THAIRAG__JOB_QUEUE__BACKEND` | `memory` | Backend: `memory` or `redis` |
| `job_queue.retention_secs` | `THAIRAG__JOB_QUEUE__RETENTION_SECS` | `86400` | How long to retain completed job records |

### Redis

| Key | Env Override | Default | Description |
|-----|-------------|---------|-------------|
| `redis.url` | `THAIRAG__REDIS__URL` | `redis://localhost:6379` | Redis connection URL |

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
| `plugins.enabled_plugins` | `THAIRAG__PLUGINS__ENABLED_PLUGINS` | `[]` | List of enabled plugin names |

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
