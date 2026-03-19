# Architecture Guide

## Crate Dependency Graph

ThaiRAG is a Rust workspace with 14 crates organized in strict layers. Each layer depends only on layers below it.

```
                    ┌──────────────┐
                    │ thairag-api  │  Axum server, routes, stores, middleware
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ thairag-agent│  Orchestrator, intent classification, RAG
                    └──────┬───────┘
                           │
              ┌────────────▼────────────┐
              │     thairag-search      │  Hybrid search, RRF fusion
              └────────────┬────────────┘
                           │
    ┌──────────┬───────────┼───────────┬──────────────┐
    │          │           │           │              │
┌───▼───┐ ┌───▼────┐ ┌────▼───┐ ┌────▼────┐ ┌───────▼───────┐
│  LLM  │ │Embedding│ │VectorDB│ │  Search │ │   Reranker    │
│Provider│ │Provider │ │Provider│ │Provider │ │   Provider    │
└───┬───┘ └───┬────┘ └────┬───┘ └────┬────┘ └───────┬───────┘
    │         │           │          │               │
    └─────────┴───────────┴──────────┴───────────────┘
                           │
              ┌────────────▼────────────┐
              │   thairag-document      │  Conversion, chunking pipeline
              └────────────┬────────────┘
                           │
    ┌──────────────────────┼──────────────────────────┐
    │                      │                          │
┌───▼─────────┐   ┌───────▼────────┐   ┌─────────────▼──┐
│ thairag-auth│   │ thairag-config │   │  thairag-thai  │
│ JWT + RBAC  │   │ Layered config │   │  Thai NLP      │
└───┬─────────┘   └───────┬────────┘   └─────────────┬──┘
    │                     │                          │
    └─────────────────────┼──────────────────────────┘
                          │
                   ┌──────▼───────┐
                   │ thairag-core │  Error types, traits, models, IDs
                   └──────────────┘
```

> **Note:** `thairag-mcp` (MCP Integration) sits alongside `thairag-agent`, providing MCP client connectivity, sync orchestration, and webhook notifications.

## Crate Details

### thairag-core
Foundation crate with zero external service dependencies.
- **ID newtypes**: `OrgId`, `DeptId`, `WorkspaceId`, `DocumentId`, `ChunkId`, `UserId`, `IdpId`, `MemoryId` — all UUID-based with the `define_id!` macro
- **Domain models**: `Organization`, `Department`, `Workspace`, `Document`, `TextChunk`, `User`, `IdentityProvider`
- **Traits**: `LlmProvider`, `EmbeddingProvider`, `VectorStore`, `SearchEngine`, `Reranker` — all async trait-based
- **Error types**: `ThaiRagError` enum covering validation, auth, not-found, provider errors
- **Permission model**: `AccessScope` with workspace-level scoping

### thairag-config
Layered configuration via the `config` crate:
1. `config/default.toml` — base defaults
2. `config/tiers/{THAIRAG_TIER}.toml` — tier overrides
3. `config/local.toml` — local overrides (git-ignored)
4. Environment variables — `THAIRAG__` prefix with `__` separator

Key config sections: `server`, `auth`, `database`, `providers` (llm, embedding, vector_store, text_search, reranker), `search`, `document`.

### thairag-auth
JWT-based authentication middleware for Axum:
- `AuthClaims` struct (sub, email, role, exp)
- `auth_layer` middleware function — extracts and validates JWT from `Authorization: Bearer` header
- Password hashing via Argon2
- Role-based access: `viewer`, `editor`, `admin`, `super_admin`

### thairag-thai
Thai language processing:
- Word segmentation using `nlpo3` with the default dictionary
- Used by BM25 indexing for proper Thai tokenization

### Provider Crates

Each provider crate implements one or more trait from `thairag-core`:

| Crate | Trait | Implementations |
|-------|-------|-----------------|
| `thairag-provider-llm` | `LlmProvider` | Claude, OpenAI, Ollama |
| `thairag-provider-embedding` | `EmbeddingProvider` | OpenAI, FastEmbed |
| `thairag-provider-vectordb` | `VectorStore`, `PersonalMemoryStore` | Qdrant, InMemory |
| `thairag-provider-search` | `SearchEngine` | Tantivy (BM25) |
| `thairag-provider-reranker` | `Reranker` | Cohere, Passthrough |

All providers are instantiated via factory functions based on config, enabling runtime provider selection.

### thairag-document
Document ingestion pipeline:
1. **Format detection** — MIME type validation (PDF, DOCX, XLSX, HTML, Markdown, CSV, plain text)
2. **Conversion** — Format-specific extractors (pdf-extract, docx-rs, calamine, scraper)
3. **Chunking** — Configurable chunk size and overlap, preserving metadata (page numbers, section titles)
4. **Embedding** — Chunks are embedded via the configured `EmbeddingProvider`
5. **Indexing** — Stored in both VectorDB (semantic search) and Tantivy (BM25)

### thairag-search
Hybrid search engine:
1. **Vector search** — Embedding query → top-K from VectorDB
2. **BM25 search** — Tokenized query → top-K from Tantivy
3. **RRF Fusion** — Reciprocal Rank Fusion merges results with configurable weights (`vector_weight`, `text_weight`) and RRF parameter `k` (default 60)
4. **Reranking** — Optional cross-encoder reranking (Cohere or passthrough)

### thairag-agent
RAG orchestrator:
- **Intent classification** — Determines if a query needs retrieval or is a direct question
- **Pipeline processing** — Search → Context assembly → LLM generation
- **Chat pipeline** — Optional configurable pipeline with system prompt, guardrails, and pre/post processors
- **Context compaction** — Automatic summarization of older messages when conversation approaches the model's context window limit (Claude Code-style). Uses Thai-aware token estimation (~2 chars/token for Thai, ~4 chars/token for English)
- **Personal memory** — Per-user memory extraction and retrieval. Extracts typed memories (preference, fact, decision, conversation, correction) from compacted conversations. Stores in vector DB with relevance decay over time

### thairag-mcp
MCP (Model Context Protocol) integration for connecting to external data sources:
- **RmcpClient**: MCP client supporting stdio (child process) and streamable HTTP transports via the `rmcp` SDK
- **SyncEngine**: Orchestrates sync runs — connect, discover resources, content-hash for deduplication, ingest via document pipeline, track state. Includes retry with exponential backoff.
- **SyncScheduler**: Background cron-based scheduler using `CancellationToken` for graceful shutdown
- **Webhook**: POST notifications to configured URLs after sync completion/failure
- Dependencies: `rmcp`, `sha2`, `cron`, `tokio-util`

### thairag-api
Axum HTTP server with:
- **Routes**: Auth, Chat (OpenAI-compatible), KM hierarchy CRUD, Documents, Settings, Health, Feedback
- **Stores**: SQLite (default), PostgreSQL, In-Memory — implementing `KmStoreTrait`
- **Middleware stack**: Request ID → Tracing → Security Headers → CORS → Metrics → Rate Limiting → Auth → CSRF
- **Session management**: DashMap-based with 50-message cap and 1-hour auto-cleanup. Supports context compaction (replacing old messages with summaries)
- **Metrics**: Prometheus counters/histograms for HTTP requests, LLM tokens, active sessions

## Data Flow

### Chat Request Flow

```
Client POST /v1/chat/completions
    │
    ▼
[Rate Limit] → [Auth Middleware] → [CSRF Guard]
    │
    ▼
chat::chat_completions()
    │
    ├─ Context compaction (if enabled & near context limit)
    │     ├─ Estimate tokens (Thai-aware)
    │     ├─ If > threshold: summarize older messages via LLM
    │     ├─ Extract personal memories (preference/fact/decision/etc.)
    │     └─ Replace session history with summary + recent messages
    │
    ├─ Retrieve personal memories (if enabled)
    │     ├─ Embed user's query
    │     ├─ Vector search user's memory store (top_k)
    │     └─ Inject as system context message
    │
    ├─ Load golden examples from feedback system
    ├─ Prepend as system message (few-shot)
    │
    ▼
Orchestrator::process()
    │
    ├─ Intent classification
    ├─ SearchEngine::search(query)
    │     ├─ VectorStore::search() (semantic)
    │     ├─ Tantivy BM25 search
    │     └─ RRF fusion + reranking
    │
    ├─ Context assembly (retrieved chunks → prompt)
    │
    └─ LlmProvider::generate()
         │
         ├─ Non-streaming: returns complete response
         └─ Streaming: SSE with content chunks + usage stats
```

### Document Ingestion Flow

```
Client POST /api/km/workspaces/{id}/documents/upload
    │
    ▼
[Auth] → documents::upload_document()
    │
    ├─ MIME type validation
    ├─ Format conversion (PDF/DOCX/XLSX/HTML → text)
    ├─ Chunking (configurable size + overlap)
    │
    ▼
For each chunk:
    ├─ EmbeddingProvider::embed(chunk.content)
    ├─ VectorStore::upsert(chunk_id, embedding)
    └─ Tantivy::index(chunk_id, content)
```

> **MCP Ingestion:** When MCP connectors are enabled, `thairag-mcp`'s SyncEngine discovers resources from external MCP servers and ingests them through the same document pipeline (format conversion → chunking → embedding → indexing). Content-hash deduplication prevents re-ingesting unchanged resources.

### Feedback-Driven Tuning Flow

```
User rates response (thumbs up/down)
    │
    ▼
POST /v1/chat/feedback
    │
    ├─ Store feedback entry (query, answer, chunks, scores, workspace_id)
    ├─ Recompute document boost map
    │     └─ Per-document positive_rate → boost multiplier [0.5, 1.5]
    ├─ Update adaptive quality threshold
    │
    ▼
Next query applies:
    ├─ Document boosts → multiplied onto search scores, re-sorted
    ├─ Golden examples → injected as few-shot system messages
    └─ Retrieval params → top_k, min_score_threshold from tuning page
```

## Database Architecture

ThaiRAG supports three storage backends:

| Backend | Use Case | Config |
|---------|----------|--------|
| **SQLite** | Development, single-node | Default when `database.url` is empty |
| **PostgreSQL** | Production, multi-node | Set `database.url` to a PostgreSQL connection string |
| **In-Memory** | Testing | Used in unit tests |

All backends implement `KmStoreTrait` with identical behavior. Key tables:
- `users` — Authentication with Argon2 password hashes
- `organizations`, `departments`, `workspaces` — KM hierarchy
- `documents`, `chunks` — Document storage with metadata
- `permissions` — Scoped access control (org/dept/workspace level)
- `identity_providers` — External IdP configuration
- `settings` — KV store for runtime configuration, feedback data, tuning parameters
- `audit_log` — Security audit trail

## Security Model

### Authentication
- Local auth: Argon2 password hashing + JWT tokens (configurable expiry)
- External: OIDC/OAuth2/SAML/LDAP via identity provider management (protocol flows are stubbed)
- First registered user becomes super admin
- Optional admin seeding via environment variables

### Authorization
- Role hierarchy: `super_admin` > `admin` > `editor` > `viewer`
- Workspace-scoped permissions — users only access documents in their assigned workspaces
- Super admins bypass all permission checks
- **Open WebUI identity passthrough** — When Open WebUI sets `ENABLE_FORWARD_USER_INFO_HEADERS=true`, ThaiRAG resolves real user identity from `X-OpenWebUI-User-Email` header and applies per-user workspace permissions even through the shared API key
- **Permission revocation** — On workspace access revocation, server-side sessions and personal memories for the affected user are cleared to prevent stale context leaks

### OWASP Hardening
- **A01 Broken Access Control**: Role-based route guards, workspace scoping
- **A02 Cryptographic Failures**: Argon2 password hashing, JWT with configurable secret
- **A04 Insecure Design**: CSRF protection on state-changing endpoints
- **A05 Security Misconfiguration**: Security response headers (CSP, X-Frame-Options, nosniff, XSS protection)
- **A07 Authentication Failures**: Brute-force protection (configurable max attempts + lockout), password complexity requirements
- **A08 Software Integrity**: Request ID tracing, structured logging
- **A09 Logging & Monitoring**: Audit log, Prometheus metrics, structured tracing

## Observability

### Prometheus Metrics (`GET /metrics`)
- `http_requests_total{method, path, status}` — Request counter
- `http_request_duration_seconds{method, path}` — Latency histogram
- `llm_tokens_total{type}` — Token usage (prompt/completion)
- `active_sessions_total` — Current active chat sessions

### Structured Logging
JSON-formatted logs with tracing spans:
```
{"timestamp":"...","level":"INFO","span":{"method":"POST","uri":"/v1/chat/completions","request_id":"..."},"message":"response","status":"200","latency_ms":1234}
```

Configure log level via `RUST_LOG` environment variable.
