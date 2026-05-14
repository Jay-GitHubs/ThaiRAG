# Architecture Guide

## Crate Dependency Graph

ThaiRAG is a Rust workspace with 16 crates organized in strict layers. Each layer depends only on layers below it.

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

> **Note:** `thairag-mcp` (MCP Integration) sits alongside `thairag-agent`, providing MCP client connectivity, sync orchestration, and webhook notifications. `thairag-provider-redis` also sits at this level, providing Redis-backed implementations of `SessionStoreTrait`, `EmbeddingCache`, and `JobQueue` for horizontal scaling. `thairag-cli` is a standalone binary crate that communicates with the API server over HTTP and has no direct dependency on internal crates.

## Crate Details

### thairag-core
Foundation crate with zero external service dependencies.
- **ID newtypes**: `OrgId`, `DeptId`, `WorkspaceId`, `DocumentId`, `ChunkId`, `UserId`, `IdpId`, `MemoryId`, `JobId`, `WebhookId`, `ApiKeyId`, `AbTestId`, `EvalSetId`, `EntityId`, `BackupId` — all UUID-based with the `define_id!` macro
- **Domain models**: `Organization`, `Department`, `Workspace`, `Document`, `TextChunk`, `User`, `IdentityProvider`, `Job`, `WebhookEvent`, `ApiKey`, `Entity`, `Relation`, `BackupManifest`, `ExtractedTable`, `ImageMetadata`
- **Traits**: `LlmProvider`, `EmbeddingProvider`, `VectorStore`, `SearchEngine`, `Reranker`, `SessionStoreTrait`, `EmbeddingCache`, `JobQueue`, `DocumentPlugin`, `SearchPlugin`, `ChunkPlugin`, `VectorStoreExport`, `SearchPluginEngine` (lets retrieval-side code apply enabled SearchPlugins without depending on the API crate's concrete registry), `GuardrailMetricsRecorder` (lets the streaming output guardrail record Prometheus counters without depending on the API crate's `MetricsState`) — all async trait-based
- **Error types**: `ThaiRagError` enum covering validation, auth, not-found, provider errors
- **Permission model**: `AccessScope` with workspace-level scoping

### thairag-config
Layered configuration via the `config` crate:
1. `config/default.toml` — base defaults
2. `config/tiers/{THAIRAG_TIER}.toml` — tier overrides
3. `config/local.toml` — local overrides (git-ignored)
4. Environment variables — `THAIRAG__` prefix with `__` separator

Key config sections: `server`, `auth`, `database`, `providers` (llm, embedding, vector_store, text_search, reranker), `search`, `document`, `session`, `embedding_cache`, `job_queue`, `redis`, `otel` (OpenTelemetry), `knowledge_graph`, `plugins`, `chat_pipeline` (with advanced RAG sub-configs: `self_rag`, `graph_rag`, `crag`, `speculative_rag`, `map_reduce`, `raptor`, `colbert`, `active_learning`, `context_compaction`, `personal_memory`, `multimodal`, `compression`, `auto_summarize`, `live_retrieval`, `conversation_memory`, `retrieval_refinement`, `tool_use`, `adaptive_threshold`).

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
| `thairag-provider-llm` | `LlmProvider` | Claude, OpenAI, Ollama, Gemini, OpenAI-Compatible |
| `thairag-provider-embedding` | `EmbeddingProvider` | OpenAI, FastEmbed, Cohere, Ollama |
| `thairag-provider-vectordb` | `VectorStore`, `PersonalMemoryStore` | Qdrant, InMemory, ChromaDB, Milvus, Weaviate, PGVector, Pinecone |
| `thairag-provider-search` | `SearchEngine` | Tantivy BM25 (disk-persisted via MmapDirectory) |
| `thairag-provider-reranker` | `Reranker` | Cohere, Passthrough, Jina |
| `thairag-provider-redis` | `SessionStoreTrait`, `EmbeddingCache`, `JobQueue` | Redis (via `redis::aio::ConnectionManager`) |

All providers are instantiated via factory functions based on config, enabling runtime provider selection.

### thairag-provider-redis
Redis implementations for horizontal scaling:
- **SessionStoreTrait** — Stores chat session history in Redis, enabling multiple API instances to share session state
- **EmbeddingCache** — Caches embedding vectors by content hash, reducing duplicate embedding API calls across restarts and instances
- **JobQueue** — Redis-backed async job queue for background tasks (sync runs, batch ingestion, re-indexing)
- Connection pooling via `redis::aio::ConnectionManager` with automatic reconnection

### thairag-document
Document ingestion pipeline:
1. **Format detection** — MIME type validation (PDF, DOCX, XLSX, HTML, Markdown, CSV, plain text)
2. **Conversion** — Format-specific extractors (pdf-extract, docx-rs, calamine, scraper)
3. **Chunking** — Configurable chunk size and overlap, preserving metadata (page numbers, section titles)
4. **Embedding** — Chunks are embedded via the configured `EmbeddingProvider`
5. **Persistence** — Chunks are saved to the `document_chunks` DB table (for Tantivy rebuild on restart)
6. **Indexing** — Stored in both VectorDB (semantic search) and Tantivy BM25 (disk-based via MmapDirectory)

### thairag-search
Hybrid search engine:
1. **Vector search** — Embedding query → top-K from VectorDB
2. **BM25 search** — Tokenized query → top-K from Tantivy (disk-persisted, auto-rebuilt from `document_chunks` table on startup if empty)
3. **RRF Fusion** — Reciprocal Rank Fusion merges results with configurable weights (`vector_weight`, `text_weight`) and RRF parameter `k` (default 60)
4. **Reranking** — Optional cross-encoder reranking (Cohere or passthrough)

### thairag-agent
RAG orchestrator with multi-agent pipeline:
- **Intent classification** — Determines if a query needs retrieval or is a direct question
- **Pipeline processing** — Search → Context assembly → LLM generation
- **Chat pipeline** — Configurable multi-agent pipeline with system prompt, guardrails, and pre/post processors
- **LLM mode** — Three modes for assigning LLMs to pipeline agents:
  - **Use Chat LLM** — All agents use the main LLM Provider directly
  - **Shared** — All agents share a separate dedicated chat LLM
  - **Per-Agent** — Each agent (Query Analyzer, Retriever, Response Generator, etc.) can have its own LLM, with fallback to shared → main LLM Provider
- **Context compaction** — Automatic summarization of older messages when conversation approaches the model's context window limit (Claude Code-style). Uses Thai-aware token estimation (~2 chars/token for Thai, ~4 chars/token for English)
- **Personal memory** — Per-user memory extraction and retrieval. Extracts typed memories (preference, fact, decision, conversation, correction) from compacted conversations. Stores in vector DB with relevance decay over time
- **Live source retrieval** — When the knowledge base returns no relevant results (empty context or avg relevance < 0.15), the pipeline automatically fetches content from active MCP connectors in real time. Uses `LiveRetrieval` agent to connect to configured connectors (OneDrive, web fetch, Slack, etc.) in parallel, read resources, and build a `CuratedContext` for the response generator. If more connectors than `max_connectors` are available, an LLM selects the most relevant ones
- **Advanced RAG strategies** — Self-RAG (iterative self-critique and retrieval), Corrective RAG (CRAG, query correction on low confidence), Speculative RAG (draft-then-verify), Map-Reduce RAG (parallel chunk summarization), RAPTOR hierarchical summaries, ColBERT late-interaction reranking, Graph RAG (entity/relation extraction and graph traversal), contextual compression, multimodal RAG (image + table extraction), active learning (uncertainty sampling for feedback prioritization), conversation memory (cross-session user preference tracking), retrieval refinement (iterative query expansion), agentic tool use (LLM-driven function calling), adaptive quality thresholds (per-workspace auto-tuning), auto-summarization of long documents
- **Guardrails module** (`guardrails/`) — Deterministic content safety with four submodules:
  - `detectors/` — Pure functions over text returning `Vec<Violation>`. Closed `ViolationCode` enum (Thai-ID with mod-11 checksum, Thai phone, email, credit card with Luhn, AWS key / JWT / GitHub PAT / generic-API-key secrets, prompt-injection / jailbreak regex set in English + Thai, operator blocklist via combined `(?i)` regex).
  - `input.rs` — `InputGuardrails::check(query)` runs detectors and produces `Pass` / `Sanitize` / `Block` actions per `policy.input_on_violation`. Critical-severity violations (Thai ID, credit card, AWS key) always block. Refusal reason is generic (codes are logged, never returned to caller).
  - `output.rs` — `OutputGuardrails::check(response)` for non-streaming responses with `redact` / `block` / `regenerate` policies, bounded by `policy.max_response_chars` to cap CPU on long outputs. `OutputGuardrails::sanitize(text, violations)` is the inline-redaction primitive shared with the streaming path.
  - `streaming.rs` — `wrap_stream_with_holdback` implements real-prevention sliding-window output filtering. Each inner chunk appends to a buffer of `policy.streaming_window_chars` (default 256); detectors run on the whole buffer per chunk; matches are redacted in place (inline `[REDACTED]`) before the chars age out of the window and flush to the client. Design rationale and trade-offs are in `docs/STREAMING_GUARDRAILS_DESIGN.md`.
- **Pipeline-level plugin hooks** — `ChatPipeline::run_search` applies `SearchPluginEngine::apply_pre_search` to every query (primary, rewriter sub-queries, HyDE) and `apply_post_search` to the deduplicated result set. The engine is supplied via `ChatPipeline::with_search_plugin_engine(...)`; the API crate's `PluginRegistry` is the concrete impl.

### thairag-mcp
MCP (Model Context Protocol) integration for connecting to external data sources:
- **RmcpClient**: MCP client supporting stdio (child process) and streamable HTTP transports via the `rmcp` SDK
- **SyncEngine**: Orchestrates sync runs — connect, discover resources, content-hash for deduplication, ingest via document pipeline, track state. Includes retry with exponential backoff
- **Live retrieval support**: `RmcpClient` is also used by the `thairag-agent::LiveRetrieval` agent for query-time fetching from connectors when the knowledge base has no results
- **SyncScheduler**: Background cron-based scheduler using `CancellationToken` for graceful shutdown
- **Webhook**: POST notifications to configured URLs after sync completion/failure
- Dependencies: `rmcp`, `sha2`, `cron`, `tokio-util`

### thairag-api
Axum HTTP server with:
- **Routes**: Auth, Chat (V1 OpenAI-compatible + V2 with metadata + WebSocket at `/ws/chat`), KM hierarchy CRUD (including super-admin `POST /api/km/users` for local user creation), Documents (versioning, batch, ACL), Settings, Health, Feedback, Webhooks, Backup/Restore, Vector Migration, Rate Limit Stats, Jobs, Evaluation, A/B Tests, **Plugins** (`GET /api/km/plugins`, `POST /plugins/{name}/{enable,disable}`), **Guardrails** (`GET /api/km/guardrails/{stats,violations}`, `POST /api/km/guardrails/preview`), Knowledge Graph, API Keys, Vault
- **Stores**: SQLite (default), PostgreSQL, In-Memory — implementing `KmStoreTrait`. Session and cache stores optionally backed by Redis via `thairag-provider-redis`
- **Middleware stack**: Request ID → Tracing → Security Headers → CORS → Metrics → Rate Limiting → Auth → CSRF
- **Session management**: DashMap-based (default) or Redis-backed (for multi-instance deployments), with 50-message cap and 1-hour auto-cleanup. Supports context compaction (replacing old messages with summaries)
- **Plugin registry**: Loads `DocumentPlugin`, `SearchPlugin`, and `ChunkPlugin` implementations at startup, persists enable-state to the KV store (`plugins.enabled` setting), and implements the `SearchPluginEngine` trait so the chat pipeline's retrieval calls fire SearchPlugin pre/post hooks too. Built-ins: `metadata-strip` (DocumentPlugin), `query-expansion` (SearchPlugin), `summary-chunk` (ChunkPlugin)
- **Embedding cache**: Optional in-process or Redis-backed cache keyed by content hash, reducing redundant embedding API calls
- **`ProviderBundleBuilder`**: Fluent builder for the `ProviderBundle` (the hot-swappable provider state), replacing the previous ten-argument constructor. Required inputs go to `new()`; optional pieces (`km_store`, `vault`, `embedding_cache`, `plugin_engine`, `guardrail_metrics`) are set via `with_*` methods. Used by the main constructor, scoped-pipeline rebuilds, dynamic provider reloads, and the vector-migration switch path.
- **Metrics**: Prometheus counters/histograms for HTTP requests, LLM tokens, active sessions, MCP sync stats, and `guardrail_streaming_redactions_total{code, stage}` for the streaming output guardrail. `MetricsState` implements `thairag_core::traits::GuardrailMetricsRecorder` so the agent crate can record without depending on `thairag-api`.

### thairag-cli
Command-line interface for managing the ThaiRAG platform remotely:
- **Standalone binary** — Communicates with the API server over HTTP (configurable via `--url` or `THAIRAG_URL` env var)
- **Authentication** — Supports API key auth via `--api-key` or `THAIRAG_API_KEY` env var
- **Subcommands** — `health` (with `--deep` flag), `status`, and management commands for KM hierarchy, documents, and system operations
- Dependencies: `clap`, `colored`, `reqwest`, `serde_json`

## Data Flow

### Chat Request Flow

The V1 API (`POST /v1/chat/completions`) is OpenAI-compatible and returns only the generated message. The V2 API (`POST /v2/chat/completions`) returns additional metadata: `search_sources` (chunks used), `intent` (classified query type), and `processing_time_ms`. WebSocket chat at `/ws/chat` uses a JSON protocol for bidirectional streaming without SSE.

```
Client POST /v1/chat/completions  (or /v2/chat/completions for metadata)
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
    ├─ Save to document_chunks table (for Tantivy rebuild on restart)
    ├─ EmbeddingProvider::embed(chunk.content)
    ├─ VectorStore::upsert(chunk_id, embedding)
    └─ Tantivy::index(chunk_id, content)  [disk-persisted via MmapDirectory]
```

### Test Query SSE Streaming Flow

```
Client GET /api/km/test-query-stream?q=...
    │
    ▼
[Auth] → test_query::stream()
    │
    ├─ Create tokio::mpsc channel for PipelineProgress events
    │
    ├─ Spawn pipeline task ──────────────────────────────────┐
    │                                                        │
    ▼                                                        ▼
SSE response (Content-Type: text/event-stream)     Pipeline stages execute:
    │                                                ├─ Query Analysis  (started → completed)
    ├─ event: pipeline_progress                      ├─ Retrieval       (started → completed)
    │  data: {"stage":"query_analysis","status":     ├─ Reranking       (started → completed)
    │         "started"}                             ├─ Context Assembly (started → completed)
    │                                                └─ Response Gen    (started → completed)
    ├─ event: pipeline_progress                              │
    │  data: {"stage":"query_analysis","status":             │
    │         "completed","duration_ms":123}                  │
    │  ...                                                   │
    ├─ event: result                                         │
    │  data: {full test-query response}              ◄───────┘
    │
    └─ stream closed
```

Each pipeline stage sends `started` and `completed` events through the `tokio::mpsc` channel. The frontend renders these in real-time, showing which stage is currently executing and timing information.

### Config Snapshots

Config snapshots allow saving and restoring complete system configuration. Snapshots are stored in the existing `settings` KV table using a `snapshot.{uuid}` key prefix, so no schema migration is needed.

```
POST /api/km/settings/snapshots          → Create snapshot (captures all current settings)
GET  /api/km/settings/snapshots          → List all snapshots
POST /api/km/settings/snapshots/restore  → Restore a snapshot by ID
DELETE /api/km/settings/snapshots/{id}   → Delete a snapshot
```

### Embedding Fingerprint Tracking

The system tracks the current embedding configuration via an `_embedding_fingerprint` key in the `settings` KV table. The fingerprint format is `{kind}:{model}:{dimension}` (e.g., `fastembed:BAAI/bge-small-en-v1.5:384`). This allows detecting when the embedding model changes, which would invalidate existing vector data.

### Tantivy Index Recovery

On startup, if the Tantivy index is empty but the database has stored chunks, the server automatically rebuilds the index in batches of 500. This handles:
- First start with a fresh Docker volume but existing database
- Index corruption or accidental volume deletion
- Container restarts with stale writer lock files (auto-cleaned)

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
- `documents` — Document metadata and original content
- `document_chunks` — Stored chunks with content, used for Tantivy BM25 index rebuild on startup (FK to `documents` with `ON DELETE CASCADE`)
- `document_versions` — Version history for documents with diff metadata
- `permissions` — Scoped access control (org/dept/workspace level)
- `workspace_acls` — Fine-grained workspace-level ACL entries
- `document_acls` — Fine-grained document-level ACL entries
- `identity_providers` — External IdP configuration
- `api_keys` — Hashed API keys (SHA-256, `trag_` prefix) with scope and expiry
- `webhooks` — Registered webhook endpoints and event subscriptions
- `jobs` — Background job records with status, type, and result payload
- `inference_logs` — Per-request LLM call logs for evaluation and cost tracking
- `settings` — KV store for runtime configuration, feedback data, tuning parameters
- `audit_log` — Security audit trail
- `search_analytics_events` — Per-query search telemetry (latency, result count, zero-result flag)
- `lineage_records` — Response-to-chunk-to-document attribution chain
- `personal_memories` — DB-backed personal memory rows with type, importance, and relevance scores
- `tenants` — Multi-tenant definitions with plan and active status
- `tenant_org_mapping` — Maps organizations to tenants for data isolation
- `custom_roles` — RBAC v2 custom role definitions with granular permissions
- `prompt_templates` — Prompt marketplace templates with ratings and versioning
- `training_datasets`, `training_pairs`, `finetune_jobs` — Embedding fine-tuning pipeline tables
- `document_comments`, `document_annotations`, `document_reviews` — Document collaboration
- `regression_runs` — Search quality regression test results

**Redis** serves as a complementary store (when configured) for: chat sessions, embedding vector cache (keyed by content hash), and the async job queue. Redis state is ephemeral and automatically rebuilt from the primary database on reconnect.

## Security Model

### Authentication
- Local auth: Argon2 password hashing + JWT tokens (configurable expiry)
- API key auth: `trag_`-prefixed keys stored as SHA-256 hashes; passed via `X-API-Key` header; support scopes (read-only, workspace-scoped, etc.) and optional expiry
- External: OIDC/OAuth2/SAML/LDAP via identity provider management (protocol flows are stubbed)
- First registered user becomes super admin
- Optional admin seeding via environment variables

### Authorization
- Role hierarchy: `super_admin` > `admin` > `editor` > `viewer`
- Workspace-scoped permissions — users only access documents in their assigned workspaces
- **Fine-grained ACLs** — Workspace-level ACL entries (`workspace_acls`) and document-level ACL entries (`document_acls`) allow granting or denying access at individual resource granularity, overriding role defaults
- Super admins bypass all permission checks
- **Open WebUI identity passthrough** — When Open WebUI sets `ENABLE_FORWARD_USER_INFO_HEADERS=true`, ThaiRAG resolves real user identity from `X-OpenWebUI-User-Email` header and applies per-user workspace permissions even through the shared API key
- **Permission revocation** — On workspace access revocation, server-side sessions and personal memories for the affected user are cleared to prevent stale context leaks

### OWASP Hardening
- **A01 Broken Access Control**: Role-based route guards, workspace scoping, fine-grained ACLs
- **A02 Cryptographic Failures**: Argon2 password hashing, JWT with configurable secret, SHA-256 API key hashing
- **A04 Insecure Design**: CSRF protection on state-changing endpoints
- **A05 Security Misconfiguration**: Security response headers (CSP, X-Frame-Options, nosniff, XSS protection)
- **A07 Authentication Failures**: Brute-force protection (configurable max attempts + lockout), password complexity requirements
- **A08 Software Integrity**: Request ID tracing, structured logging
- **A09 Logging & Monitoring**: Audit log (actions include: login, logout, api_key_created, api_key_revoked, document_uploaded, document_deleted, workspace_acl_changed, backup_created, backup_restored, webhook_triggered, job_queued, eval_run), Prometheus metrics, structured tracing

## Observability

### Prometheus Metrics (`GET /metrics`)
- `http_requests_total{method, path, status}` — Request counter
- `http_request_duration_seconds{method, path}` — Latency histogram
- `llm_tokens_total{type}` — Token usage (prompt/completion)
- `active_sessions_total` — Current active chat sessions
- `mcp_sync_runs_total{connector, status}` / `mcp_sync_items_total{connector, action}` / `mcp_sync_duration_seconds{connector}` — Connector sync stats
- `guardrail_streaming_redactions_total{code, stage}` — Streaming output guardrail redactions. `code` is a value from the closed `ViolationCode` enum (Thai-ID / email / Luhn-validated credit-card / AWS-key / JWT / GitHub-PAT / generic-API-key / blocklist / etc.); `stage` is `output`. Cardinality is bounded by the enum so this counter is safe to scrape at any rate.

Prometheus scrapes ThaiRAG directly at `/metrics`. In scaled deployments (multiple replicas behind a load balancer), use Docker DNS service discovery — Prometheus can be configured with `dns_sd_configs` targeting the service name to discover all replicas automatically.

Grafana dashboards (served on port 3001 when included in the Docker Compose stack) provide pre-built panels for request throughput, latency percentiles, LLM token spend, session counts, and search quality metrics.

### OpenTelemetry
When `otel.enabled = true`, ThaiRAG exports distributed traces and metrics via OTLP (gRPC or HTTP) to a configured collector endpoint. Trace context propagates through all pipeline stages (intent classification → retrieval → reranking → generation), enabling end-to-end latency attribution. The `otel.endpoint` config key (or `THAIRAG__OTEL__ENDPOINT` env var) points to the OTLP receiver.

### Structured Logging
JSON-formatted logs with tracing spans:
```
{"timestamp":"...","level":"INFO","span":{"method":"POST","uri":"/v1/chat/completions","request_id":"..."},"message":"response","status":"200","latency_ms":1234}
```

Configure log level via `RUST_LOG` environment variable.

## Phase 6: Analytics, Governance, and Platform Features

### Search Analytics Pipeline

Every chat request records search telemetry via a fire-and-forget pattern. After the RAG pipeline completes (in both streaming and non-streaming code paths), `chat.rs` constructs a `SearchAnalyticsEvent` capturing the query text (truncated to 2000 chars), user ID, workspace ID, result count, search latency in milliseconds, and a zero-results flag. The event is written to the `search_analytics_events` table via `tokio::spawn`, so the response is never blocked by analytics persistence.

The store exposes several query methods on this data:
- `list_search_events` with time-range, workspace, user, and zero-results-only filtering
- `get_popular_queries` returning top queries by frequency with average result count and latency
- `get_search_analytics_summary` producing aggregate metrics: total searches, zero-result count, average latency, average results, and searches-per-day time series

### Document Lineage Tracking

After each chat response is generated, the system records which document chunks contributed to it. For every chunk in `meta.retrieved_chunks`, a `LineageRecord` is inserted (also via `tokio::spawn` fire-and-forget) linking:

```
response_id  --->  chunk_id  --->  doc_id
     │                │              │
     │                │              └─ doc_title (human-readable)
     │                └─ chunk_text_preview, score, rank
     └─ query_text, timestamp, contributed (bool)
```

The `contributed` flag distinguishes chunks that were actually included in the LLM context from those that were retrieved but filtered out by reranking or score thresholds. Two query directions are supported:
- `get_lineage_for_response(response_id)` — "What sources backed this answer?"
- `get_lineage_for_document(doc_id, limit)` — "Which queries have cited this document?"

### Multi-tenancy Architecture

Multi-tenancy adds a layer above the existing organization hierarchy. A `Tenant` has an ID, name, plan (e.g., "free", "standard", "enterprise"), and active flag. The `tenant_org_mapping` table maps organizations to tenants, enforcing that each org belongs to at most one tenant.

**Quota enforcement** is defined per tenant via `TenantQuota`:
- `max_documents` (default 1,000)
- `max_storage_bytes` (default 10 GB)
- `max_queries_per_day` (default 10,000)
- `max_users` (default 50)
- `max_workspaces` (default 20)

Current usage is tracked via `TenantUsage` (documents, storage, daily queries, users, workspaces). The store provides `get_tenant_usage` to compute live counts and `get_tenant_quota` / `set_tenant_quota` for configuration. Data isolation is achieved by joining through `tenant_org_mapping` — queries for tenant-scoped data resolve the tenant's org IDs first, then filter all downstream tables (workspaces, documents, chunks) by those orgs.

### RBAC v2 Custom Roles

Phase 6 extends the fixed role hierarchy (`viewer` < `editor` < `admin` < `super_admin`) with user-defined custom roles. A `CustomRole` consists of:
- **name** and **description** — Human-readable identifiers
- **permissions** — A list of `RolePermission` entries, each specifying a **resource** (e.g., `"documents"`, `"workspaces"`, `"settings"`, `"analytics"`) and a set of **actions** (e.g., `["read", "write", "delete"]`)
- **is_system** — Flag distinguishing built-in system roles (which cannot be deleted) from user-created roles

Custom roles are managed via `insert_custom_role`, `get_custom_role`, `list_custom_roles`, `update_custom_role`, and `delete_custom_role` on `KmStoreTrait`. They complement rather than replace the existing role hierarchy — a user can be assigned a custom role that grants specific resource-action combinations not covered by the fixed tiers.

### Qdrant Dimension Auto-detection

The `QdrantVectorStore` in `thairag-provider-vectordb` uses lazy collection initialization with automatic dimension mismatch detection. On the first `upsert` call, `ensure_collection(dimension)` is invoked:

1. If the collection does not exist, it is created with the given dimension and Cosine distance.
2. If the collection exists, the current vector dimension is read from `collection_info`. If the existing dimension differs from the requested dimension (which happens when the user switches embedding models), the collection is **deleted and recreated** with the new dimension. A warning is logged indicating that existing vectors are incompatible.
3. An `AtomicBool` (`collection_ready`) short-circuits subsequent checks after the first successful initialization.

On `delete_all`, the `collection_ready` flag is reset to `false`, so the next upsert will re-check and potentially create a fresh collection with whatever dimension the new embedding model produces.

### Streaming Reranking

The `Reranker` trait in `thairag-core` exposes a `rerank_stream` method alongside the standard `rerank`:

```rust
async fn rerank_stream(
    &self,
    query: &str,
    results: Vec<SearchResult>,
) -> Result<Vec<SearchResult>>
```

The default implementation delegates to `rerank()`, so existing providers (Cohere, Jina, Passthrough) work without modification. The streaming variant is used by the test-query SSE pipeline (`test_query::stream`), where reranking results are emitted progressively as `pipeline_progress` events. This allows the admin UI to show reranked results appearing incrementally rather than waiting for the full batch.

### Agent Memory Persistence

Personal memories extracted during context compaction are persisted to the `personal_memories` database table via `KmStoreTrait`. Each `PersonalMemoryRow` stores:
- **user_id** — Owner of the memory
- **memory_type** — One of `preference`, `fact`, `decision`, `conversation`, `correction`
- **summary** — The extracted memory text
- **topics** — JSON array of topic tags
- **importance** — Float weight (0.0 to 1.0)
- **relevance_score** — Decays over time; used to rank memories when injecting into context
- **created_at** / **last_accessed_at** — Timestamps for decay calculation

During chat, the system embeds the user's query and performs a vector similarity search against the user's memory store (via `PersonalMemoryStore` trait on the VectorDB provider). Top-K results are injected as a system context message. The DB-backed storage ensures memories survive container restarts, complementing the vector store which holds the embeddings for similarity search.

Store operations: `insert_personal_memory`, `list_personal_memories` (with limit), `delete_personal_memory`, `delete_all_personal_memories` (used on permission revocation), and `count_personal_memories`.

### Prompt Marketplace

The prompt marketplace provides a shared repository of reusable prompt templates. A `PromptTemplate` includes:
- **name**, **description**, **category** — For discovery and filtering
- **content** — The actual prompt text with `{{variable}}` placeholders
- **variables** — List of expected variable names
- **author_id** / **author_name** — Creator attribution
- **version** — Monotonically increasing version number
- **is_public** — Whether the template is visible to all users or only the author
- **rating_avg** / **rating_count** — Aggregate user ratings

Key operations on `KmStoreTrait`:
- **CRUD** — `insert_prompt_template`, `get_prompt_template`, `update_prompt_template`, `delete_prompt_template`
- **Discovery** — `list_prompt_templates` with `PromptTemplateFilter` supporting category, free-text search, public/private, author, and pagination
- **Rating** — `rate_prompt_template` accepts a `PromptRating` (template_id, user_id, rating 1-5) and updates the template's aggregate scores
- **Forking** — `fork_prompt_template` creates a copy under a new author, enabling users to customize community templates while preserving attribution to the original

### Embedding Fine-tuning Pipeline

The embedding fine-tuning pipeline allows creating domain-specific embedding models from user feedback and curated query-document pairs. The lifecycle involves three entities:

1. **Training Datasets** (`TrainingDataset`) — Named collections with a description and pair count. Created via `insert_training_dataset`, listed, retrieved, or deleted.

2. **Training Pairs** (`TrainingPair`) — Individual query-to-document relevance examples within a dataset:
   - `query` — The search query text
   - `positive_doc` — A document excerpt that is relevant to the query
   - `negative_doc` (optional) — A document excerpt that is not relevant (for contrastive learning)

3. **Fine-tune Jobs** (`FinetuneJob`) — Execution records tracking:
   - `dataset_id` — Which dataset to train on
   - `base_model` — The starting embedding model
   - `status` — Lifecycle state: `pending` -> `running` -> `completed` | `failed`
   - `metrics` — JSON blob with training metrics (loss, accuracy, etc.) populated on completion
   - `output_model_path` — Path to the resulting fine-tuned model

Job status transitions are managed via `update_finetune_job_status`. The admin UI can list all jobs, inspect metrics, and configure the output model as the active embedding provider.
