# ThaiRAG

Production-ready Retrieval-Augmented Generation platform with Thai language support, hierarchical knowledge management, and a full-featured admin UI.

## Features

- **Hybrid Search** — Vector similarity + BM25 full-text search with Reciprocal Rank Fusion and optional reranking
- **Thai NLP** — Built-in Thai word segmentation via `nlpo3` for accurate tokenization
- **OpenAI-Compatible API** — Drop-in replacement at `/v1/chat/completions` and `/v1/models`, works with Open WebUI and any OpenAI-compatible client
- **Hierarchical Knowledge Management** — Organization → Department → Workspace → Documents with scoped permissions
- **Multi-Format Documents** — PDF, DOCX, XLSX, HTML, Markdown, CSV, plain text with automatic chunking
- **Streaming Responses** — Server-Sent Events with real-time token usage reporting
- **Feedback-Driven Tuning** — Document boost/penalty, golden examples, adaptive retrieval parameters based on user feedback
- **Admin UI** — React + Ant Design dashboard for managing the entire platform (11 pages)
- **Identity Provider Support** — Local auth (Argon2 + JWT) with OIDC/OAuth2/SAML/LDAP management
- **Production Hardened** — Rate limiting, CSRF protection, OWASP security headers, Prometheus metrics, audit logging, brute-force protection

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Admin UI (React)                       │
│              localhost:8081 (via Docker)                  │
└──────────────────────┬──────────────────────────────────┘
                       │ REST / SSE
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
┌──────────────────────▼──────────────────────────────────┐
│              thairag-agent (Orchestrator)                 │
│         Intent classification + RAG pipeline             │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│              thairag-search (Hybrid Search)               │
│      Vector + BM25 → RRF Fusion → Reranking              │
└───┬──────────┬───────────┬──────────┬───────────────────┘
    │          │           │          │
┌───▼───┐ ┌───▼────┐ ┌────▼───┐ ┌───▼──────┐
│  LLM  │ │Embedding│ │VectorDB│ │ Reranker │
│Claude │ │ OpenAI  │ │ Qdrant │ │ Cohere   │
│Ollama │ │FastEmbed│ │InMemory│ │Passthru  │
│OpenAI │ │         │ │        │ │          │
└───────┘ └────────┘ └────────┘ └──────────┘
```

**13 Rust crates** organized in a layered dependency graph:

| Layer | Crates | Purpose |
|-------|--------|---------|
| Core | `thairag-core` | Error types, ID newtypes, traits, domain models |
| Foundation | `thairag-config`, `thairag-thai`, `thairag-auth` | Configuration, Thai NLP, JWT authentication |
| Providers | `thairag-provider-{llm,embedding,vectordb,search,reranker}` | Pluggable provider abstractions |
| Processing | `thairag-document`, `thairag-search` | Document conversion/chunking, hybrid search |
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

| Tier | LLM | Embeddings | Vector DB | Reranker |
|------|-----|-----------|-----------|----------|
| **free** | Ollama (llama3.2) | FastEmbed (BGE) | In-Memory | Passthrough |
| **standard** | Claude Sonnet | OpenAI (small) | Qdrant | Cohere v3.0 |
| **premium** | Claude Sonnet | OpenAI (large) | Qdrant | Cohere v3.5 |

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
# OpenAI-compatible
GET  /v1/models                          # List models
POST /v1/chat/completions                # Chat (streaming + non-streaming)
POST /v1/chat/feedback                   # Submit feedback

# Health & Metrics
GET  /health                             # Health check (?deep=true for provider probes)
GET  /metrics                            # Prometheus metrics

# Auth
POST /api/auth/register                  # Register
POST /api/auth/login                     # Login (returns JWT)
GET  /api/auth/providers                 # List enabled identity providers

# Knowledge Management (all under /api/km, auth required)
GET|POST    /orgs                        # Organizations
GET|POST    /orgs/{id}/depts             # Departments
GET|POST    /orgs/{id}/depts/{id}/workspaces  # Workspaces
GET|POST    /workspaces/{id}/documents   # Documents
POST        /workspaces/{id}/documents/upload  # File upload
POST        /workspaces/{id}/test-query  # Test search + RAG

# Settings (super admin)
GET|PUT     /settings/providers          # Provider configuration
GET|PUT     /settings/document           # Document processing config
GET|PUT     /settings/chat-pipeline      # Chat pipeline config
GET|POST    /settings/identity-providers # Identity provider management
GET|PUT     /settings/feedback/*         # Feedback & tuning
GET         /settings/audit-log          # Audit log
```

See [docs/API_REFERENCE.md](docs/API_REFERENCE.md) for complete endpoint documentation.

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture Guide](docs/ARCHITECTURE.md) | Crate dependency graph, data flow, pipeline design |
| [Admin UI Guide](docs/ADMIN_UI_GUIDE.md) | Complete manual for all 11 admin pages |
| [Deployment Guide](docs/DEPLOYMENT_GUIDE.md) | Docker, configuration, production setup |
| [API Reference](docs/API_REFERENCE.md) | All endpoints with request/response schemas |
| [Integration Guide](docs/INTEGRATION_GUIDE.md) | Open WebUI, OIDC/SSO, external systems |

## Testing

```bash
# Backend tests (198 tests)
cargo test

# Admin UI type check
cd admin-ui && npx tsc --noEmit

# Playwright e2e tests (48 tests)
cd admin-ui && npx playwright test
```

## License

Copyright (C) 2026 Anuwat Yodngoen <jdevspecialist@gmail.com>

This program is free software: you can redistribute it and/or modify it under the terms of the **GNU Affero General Public License v3.0** as published by the Free Software Foundation.

See [LICENSE](LICENSE) for the full license text.

For commercial licensing inquiries, contact: jdevspecialist@gmail.com
