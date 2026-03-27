# ThaiRAG

Production-ready Retrieval-Augmented Generation platform with Thai language support, hierarchical knowledge management, and a full-featured admin UI.

## Features

- **Hybrid Search** — Vector similarity + BM25 full-text search (disk-persisted Tantivy with auto-recovery) with Reciprocal Rank Fusion and optional reranking
- **Thai NLP** — Built-in Thai word segmentation via `nlpo3` for accurate tokenization
- **OpenAI-Compatible API** — Drop-in replacement at `/v1/chat/completions` and `/v1/models`, works with Open WebUI and any OpenAI-compatible client
- **Hierarchical Knowledge Management** — Organization → Department → Workspace → Documents with scoped permissions
- **Multi-Format Documents** — PDF, DOCX, XLSX, HTML, Markdown, CSV, plain text with automatic chunking
- **Multi-Agent Chat Pipeline** — Configurable LLM assignment per agent (Use Chat LLM / Shared / Per-Agent modes) with fallback chain
- **Streaming Responses** — Server-Sent Events with real-time token usage reporting
- **Feedback-Driven Tuning** — Document boost/penalty, golden examples, adaptive retrieval parameters based on user feedback
- **MCP Connectors** — Connect to external data sources (Confluence, Notion, GitHub, Slack, Google Drive, OneDrive, PostgreSQL, and more) via the Model Context Protocol with automatic sync scheduling, retry logic, and webhook notifications
- **Live Source Retrieval** — When the knowledge base has no relevant results, automatically fetch content from active MCP connectors (OneDrive, web pages, Slack, etc.) in real time — no pre-embedding required
- **Config Snapshots** — Save and restore full configuration with embedding fingerprint tracking for safe rollbacks
- **Live Pipeline Stages** — Real-time SSE progress showing agent names and tasks during queries
- **Chat Persistence** — Test chat history preserved across page navigation via sessionStorage
- **Collapsible Settings UI** — All settings sections are collapsible for a cleaner interface
- **Embedding Protection** — Warns before destructive embedding model changes, auto-saves snapshot before applying
- **Plugin System** — DocumentPlugin / SearchPlugin / ChunkPlugin interfaces with built-in plugins and runtime registration
- **Multi-Modal RAG** — Image vision description and table extraction from PDFs via vision-capable LLMs
- **API Versioning** — V1 OpenAI-compatible endpoint + V2 with metadata, sources, and intent in responses
- **WebSocket Chat** — Real-time bidirectional chat at `/ws/chat` alongside SSE streaming
- **Conversation Auto-Summarization** — Automatic context compaction when conversation history grows long
- **Vector DB Migration** — Hot-swap provider switching with live data migration between vector databases
- **API Key Authentication** — `trag_` prefixed keys, SHA-256 hashed storage, `X-API-Key` header support
- **Knowledge Graph** — LLM-based entity and relation extraction with queryable graph endpoint
- **A/B Testing** — Framework for comparing retrieval strategies, prompt variants, and model configurations
- **Search Quality Evaluation** — RAGAS-based metrics for measuring retrieval and generation quality
- **Backup & Restore** — Full platform backup and restore via admin API
- **Webhooks** — Event-driven outbound notifications for document ingestion, sync completion, and more
- **Document Versioning** — Version history with diff support for updated documents
- **Batch Document Upload** — Upload multiple documents in a single request with background processing
- **Fine-Grained ACLs** — Workspace-level and document-level access control lists
- **Background Job Queue** — Async job processing with SSE streaming for progress updates
- **Redis Backends** — Optional Redis for session storage, embedding cache, and job queue (horizontal scaling)
- **Advanced RAG** — Self-RAG, Corrective RAG, Speculative RAG, Map-Reduce RAG, RAPTOR, ColBERT reranking, Active Learning, Context Compaction, Personal Memory
- **Admin UI** — React + Ant Design dashboard for managing the entire platform (19+ pages) with pipeline stages visualization and config snapshots management
- **Dark Mode + i18n** — Light/dark theme toggle with Thai and English localization
- **Mobile Responsive UI** — Admin UI adapts to mobile and tablet screen sizes
- **Rate Limiting Dashboard** — Real-time rate limit analytics and per-client usage stats
- **Identity Provider Support** — Local auth (Argon2 + JWT) with OIDC/OAuth2/SAML/LDAP management
- **Production Hardened** — Rate limiting, CSRF protection, OWASP security headers, Prometheus metrics, audit logging, brute-force protection

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Admin UI (React)                       │
│              localhost:8081 (via Docker)                  │
└──────────────────────┬──────────────────────────────────┘
                       │ REST / SSE / WebSocket
┌──────────────────────▼──────────────────────────────────┐
│              thairag-api (Axum server)                    │
│                  localhost:8080                           │
│  ┌────────────┬───────────┬──────────┬────────────────┐  │
│  │   Auth     │   Chat    │    KM    │   Settings     │  │
│  │  Routes    │  Routes   │  Routes  │   Routes       │  │
│  └────────────┴───────────┴──────────┴────────────────┘  │
│  ┌────────────────────────────────────────────────────┐  │
│  │              Middleware Stack                       │  │
│  │  CORS · Rate Limit · Auth · CSRF · Metrics         │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────┬──────────────────────────────────┘
                       │
         ┌─────────────┴─────────────┐
         │                           │
┌────────▼────────┐       ┌──────────▼──────────┐
│ thairag-agent   │       │ Redis (optional)     │
│ Orchestrator    │       │ Sessions · Cache     │
│ Intent + RAG    │       │ Job Queue            │
└────────┬────────┘       └─────────────────────┘
         │
┌────────▼──────────────────────────────────────┐
│              thairag-search (Hybrid Search)    │
│      Vector + BM25 → RRF Fusion → Reranking   │
└───┬──────────┬────────────┬────────┬──────────┘
    │          │            │        │
┌───▼────────┐ ┌───▼──────┐ ┌──▼───────────────┐ ┌───▼────────┐
│    LLM     │ │Embedding │ │    VectorDB      │ │  Reranker  │
│ Claude     │ │ OpenAI   │ │ Qdrant           │ │ Cohere     │
│ Ollama     │ │ FastEmbed│ │ ChromaDB         │ │ Jina       │
│ OpenAI     │ │ Cohere   │ │ Milvus           │ │ Passthru   │
│ Gemini     │ │ Ollama   │ │ Weaviate         │ └────────────┘
│ OAI-Compat │ └──────────┘ │ PGVector         │
└────────────┘              │ Pinecone         │
                            │ In-Memory        │
                            └──────────────────┘

External Services (Docker Compose):
  PostgreSQL · Qdrant · Redis · Prometheus · Grafana · Keycloak
```

**15 Rust crates** organized in a layered dependency graph:

| Layer | Crates | Purpose |
|-------|--------|---------|
| Core | `thairag-core` | Error types, ID newtypes, traits, domain models |
| Foundation | `thairag-config`, `thairag-thai`, `thairag-auth` | Configuration, Thai NLP, JWT authentication |
| Providers | `thairag-provider-{llm,embedding,vectordb,search,reranker}` | Pluggable provider abstractions |
| Infrastructure | `thairag-provider-redis` | Redis backends for sessions, embedding cache, job queue |
| Processing | `thairag-document`, `thairag-search` | Document conversion/chunking, hybrid search |
| Integration | `thairag-mcp` | MCP client, sync engine, scheduler, webhooks |
| Intelligence | `thairag-agent` | Orchestrator with intent classification + RAG |
| Server | `thairag-api` | Axum HTTP server, routes, middleware, stores |

## Quick Start

### Option 1: Docker Compose (Recommended)

```bash
# 1. Clone and configure
git clone <repo-url> && cd thairag
cp .env.example .env  # Edit with your API keys

# 2a. Core services (API + Admin UI + PostgreSQL + Qdrant)
docker compose up --build -d

# 2b. Full stack with Keycloak (OIDC) + Open WebUI
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up --build -d

# 3. Access
#    API:        http://localhost:8080
#    Admin UI:   http://localhost:8081
#    Keycloak:   http://localhost:9090  (full stack only)
#    Open WebUI: http://localhost:3000  (full stack only)
```

### Option 2: Local Development

```bash
# Prerequisites: Rust 1.88+, Node.js 20+

# 1. Start the API server
THAIRAG_TIER=free cargo run -p thairag-api

# 2. Start the Admin UI (in another terminal)
cd admin-ui && npm install && npm run dev

# 3. Access
#    API:      http://localhost:8080
#    Admin UI: http://localhost:5173
```

### First Login

The first user to register automatically becomes a super admin. You can also seed an admin account via environment variables:

```bash
THAIRAG__ADMIN__EMAIL=admin@example.com
THAIRAG__ADMIN__PASSWORD=YourSecurePassword123
```

## Configuration

ThaiRAG uses a layered configuration system:

```
config/default.toml          ← Base defaults
config/tiers/{tier}.toml     ← Tier overrides (free, standard, premium)
config/local.toml            ← Local overrides (git-ignored)
Environment variables        ← Final override (THAIRAG__ prefix)
```

Select a tier via `THAIRAG_TIER` environment variable:

| Tier | LLM | Embeddings | Vector DB | Reranker | Backends |
|------|-----|-----------|-----------|----------|----------|
| **free** | Ollama (llama3.2) | FastEmbed (BGE) | In-Memory | Passthrough | In-memory |
| **standard** | Claude Sonnet | OpenAI (small) | Qdrant | Cohere v3.0 | Redis |
| **premium** | Claude Sonnet | OpenAI (large) | Qdrant | Cohere v3.5 | Redis |

### Key Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `THAIRAG_TIER` | Config tier | `free` |
| `THAIRAG__SERVER__PORT` | HTTP port | `8080` |
| `THAIRAG__AUTH__ENABLED` | Enable JWT auth | `false` |
| `THAIRAG__AUTH__JWT_SECRET` | JWT signing secret | `dev-secret-change-me` |
| `THAIRAG__PROVIDERS__LLM__API_KEY` | LLM API key | — |
| `THAIRAG__PROVIDERS__EMBEDDING__API_KEY` | Embedding API key | — |
| `THAIRAG__PROVIDERS__RERANKER__API_KEY` | Reranker API key | — |
| `THAIRAG__DATABASE__URL` | PostgreSQL URL | — (SQLite if empty) |
| `THAIRAG__ADMIN__EMAIL` | Seed super admin email | — |
| `THAIRAG__ADMIN__PASSWORD` | Seed super admin password | — |

## API Overview

ThaiRAG exposes an OpenAI-compatible API plus KM management endpoints:

```
# OpenAI-compatible (V1)
GET  /v1/models                          # List models
POST /v1/chat/completions                # Chat (streaming + non-streaming)
POST /v1/chat/feedback                   # Submit feedback

# API V2 (with metadata, sources & intent)
GET  /v2/models                          # V2 models list
POST /v2/chat/completions                # V2 chat with metadata & sources
POST /v2/search                          # Direct search endpoint
GET  /api/version                        # API version info

# WebSocket
WS   /ws/chat                            # WebSocket chat

# Health & Metrics
GET  /health                             # Health check (?deep=true for provider probes)
GET  /metrics                            # Prometheus metrics

# Auth
POST /api/auth/register                  # Register
POST /api/auth/login                     # Login (returns JWT)
GET  /api/auth/providers                 # List enabled identity providers
GET|POST /api/auth/api-keys              # Manage API keys (trag_ prefix)
DELETE   /api/auth/api-keys/{id}         # Revoke API key

# Knowledge Management (all under /api/km, auth required)
GET|POST    /orgs                        # Organizations
GET|POST    /orgs/{id}/depts             # Departments
GET|POST    /orgs/{id}/depts/{id}/workspaces  # Workspaces
GET|POST    /workspaces/{id}/documents   # Documents
POST        /workspaces/{id}/documents/upload       # File upload
POST        /workspaces/{id}/documents/batch-upload # Batch upload
POST        /workspaces/{id}/test-query             # Test search + RAG
POST        /workspaces/{id}/test-query-stream      # SSE streaming test query
GET         /workspaces/{id}/knowledge-graph        # Knowledge graph
GET         /workspaces/{id}/jobs                   # List background jobs
GET         /workspaces/{id}/jobs/stream            # SSE job progress stream

# Settings (super admin)
GET|PUT     /settings/providers          # Provider configuration
GET|PUT     /settings/document           # Document processing config
GET|PUT     /settings/chat-pipeline      # Chat pipeline config
GET|POST    /settings/identity-providers # Identity provider management
GET|PUT     /settings/feedback/*         # Feedback & tuning
GET         /settings/audit-log          # Audit log
POST        /settings/snapshots          # Create config snapshot
GET         /settings/snapshots          # List config snapshots
DELETE      /settings/snapshots          # Delete a snapshot
POST        /settings/snapshots/{id}/restore  # Restore config snapshot

# Webhooks
GET|POST /webhooks                       # Manage webhooks
DELETE   /webhooks/{id}                  # Delete webhook

# MCP Connectors (super admin)
GET|POST    /connectors                  # List / create connectors
GET         /connectors/templates        # List connector templates
POST        /connectors/from-template    # Create from template
GET|PUT|DEL /connectors/{id}             # Get / update / delete
POST        /connectors/{id}/sync        # Trigger sync
POST        /connectors/{id}/pause       # Pause connector
POST        /connectors/{id}/resume      # Resume connector
GET         /connectors/{id}/sync-runs   # Sync history
POST        /connectors/{id}/test        # Test connection

# Enterprise Admin (super admin)
POST /admin/backup                       # Create platform backup
POST /admin/restore                      # Restore from backup
POST /admin/vector-migration/start       # Start vector DB migration
GET  /admin/rate-limits/stats            # Rate limit analytics

# Search Quality & A/B Testing
GET|POST /eval/query-sets                # Evaluation query sets (RAGAS)
GET|POST /ab-tests                       # A/B testing configurations

# Plugins
GET  /plugins                            # List registered plugins
```

See [docs/API_REFERENCE.md](docs/API_REFERENCE.md) for complete endpoint documentation.

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture Guide](docs/ARCHITECTURE.md) | Crate dependency graph, data flow, pipeline design |
| [Admin UI Guide](docs/ADMIN_UI_GUIDE.md) | Complete manual for all 19+ admin pages |
| [Deployment Guide](docs/DEPLOYMENT_GUIDE.md) | Docker, configuration, production setup |
| [API Reference](docs/API_REFERENCE.md) | All endpoints with request/response schemas |
| [Integration Guide](docs/INTEGRATION_GUIDE.md) | Open WebUI, OIDC/SSO, external systems |
| [Scaling Guide](docs/scaling.md) | Horizontal scaling with Redis, load balancing |

## Testing

```bash
# Backend tests (326+ tests)
cargo test

# Admin UI type check
cd admin-ui && npx tsc --noEmit

# Playwright e2e tests (48+ tests)
cd admin-ui && npx playwright test
```

## License

Copyright (C) 2026 Anuwat Yodngoen <jdevspecialist@gmail.com>

This program is free software: you can redistribute it and/or modify it under the terms of the **GNU Affero General Public License v3.0** as published by the Free Software Foundation.

See [LICENSE](LICENSE) for the full license text.

For commercial licensing inquiries, contact: jdevspecialist@gmail.com
