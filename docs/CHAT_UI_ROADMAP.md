# ThaiRAG first-party Chat UI — implementation roadmap

Goal: a focused, ChatGPT-like end-user chat frontend built on the ThaiRAG
backend/core, replacing Open WebUI (OWUI). OWUI runs in parallel until a parity
gate passes, then is decommissioned.

## Decisions (locked)

- **Separate app `chat-ui/`** — new top-level dir mirroring `admin-ui/`. Reuse the
  stack: React 18 + Vite 6 + TS + TanStack Query + react-router 6 + Playwright.
  Reuse `admin-ui/src/api/client.ts` axios+interceptor pattern and the nginx SSE
  config. Own port (8082), independent deploy. End users vs admins = different
  audiences.
- **New first-party endpoint surface under `/api/chat`**, NOT `/v1`. Keep
  `/v1/chat/completions` clean as the OpenAI-compat surface so OWUI and external
  clients keep working untouched during the whole transition.
- **Auth: configurable (native JWT + OIDC)** — native `/api/auth/login` by
  default, optional Keycloak/OIDC for enterprise tenants. Both supported.
- **File upload in v1** — per-conversation upload flows into the existing ingest
  pipeline.
- **Run OWUI in parallel** until parity, then remove in one cleanup PR.

## Phase 1 — Conversation persistence (backend, the blocker)

Status: PR-1 merged (#245 — tables, store CRUD across 3 backends, `/api/chat/
conversations` routes with per-user ACL). PR-2 (this) adds the `chat_history`
service glue: owner-checked load of stored history into pipeline `ChatMessage`s
+ `persist_turn` (user + assistant with citations/token-stats JSON). It is
consumed by the Phase 2 `/api/chat` streaming endpoint and deliberately does
**not** touch the `/v1` SessionStore path.


Chat history today is ephemeral (in-memory DashMap / 1h Redis TTL); zero
conversation tables in Postgres; no session-listing API; no per-user isolation.
This blocks a real product.

- New tables (idempotent, both Postgres + SQLite backends):
  - `conversations` — id, user_id (FK users), title, workspace_scope (nullable),
    created_at, updated_at, archived_at.
  - `messages` — id, conversation_id (FK cascade), role, content, citations
    (jsonb/text), images (jsonb/text), token_stats (jsonb/text), seq, created_at.
- Store methods + REST routes:
  - `GET/POST /api/chat/conversations`, `GET/PATCH/DELETE
    /api/chat/conversations/{id}`, `GET /api/chat/conversations/{id}/messages`.
  - **Per-user ACL on every route via `claims.sub`** (real gap today). Non-owner → 403.
- Context assembly: load last N messages for conversation_id (owner-checked) to
  build pipeline context; leave existing SessionStore intact for `/v1`.
- Exit: persists across docker-rebuild; ACL tests; store unit tests.

## Phase 2 — First-party streaming chat endpoint + protocol

Status: PR 2a (this) shipped the `POST /api/chat/conversations/{id}/messages`
SSE endpoint — owner-checked, loads history via `chat_history::load_history`,
runs the pipeline, streams a clean first-party protocol (`progress` / `token` /
`citation` / `done` / `error` + `[DONE]`), and persists the turn via
`chat_history::persist_turn`. New handler reuses the `/v1` setup helpers but has
its own SSE loop — `/v1` is untouched (OWUI/OpenAI clients unaffected).
PR 2b (this) shipped inline source images: `image_blob_id` threaded into
`RetrievedChunkMeta`; token-gated `GET /api/chat/media/{image_id}?token=…`
(reuses the citation-token model); the stream emits `{"type":"image",…}` events
(deduped, capped) and persists them to `messages.images`; gated by
`chat_pipeline.inline_images_enabled` (default off) + `inline_images_max`
(default 4), requires `citation_base_url`. Phase 2 complete.


- `POST /api/chat/conversations/{id}/messages` (SSE): persists user turn → streams
  → persists assistant turn (content + citations + images).
- Clean event protocol: `token`, `citation` {doc_id,title,snippet,page,section,url},
  `image` {image_id,url,page,caption}, `progress`, `done` (usage/tokens).
- Tokenized media route `GET /api/chat/media/{image_id}?token=…` — clone the
  citation-viewer JWT pattern (`JwtService::encode_citation`). Bytes already in
  `document_image_blobs`; chunks carry `image_blob_id`.
- Image relevance gating (config flag, default-off): only page-render/embedded
  image chunks, deduped by page.

## Phase 3 — Chat UI MVP

Status: PR-3a (this) scaffolds the standalone `chat-ui/` app (React 18 + Vite 6 +
TS + antd + TanStack Query, port 8082, Dockerfile + nginx + compose service).
Native JWT login (reuses the backend `/api/auth/login`), conversation sidebar
(list/create/select/delete + auto-title from first message), and the streaming
chat loop consuming the first-party SSE protocol (`token`/`citation`/`image`/
`done`) with markdown + Thai rendering, inline citation chips, and inline source
images. Live e2e against the Docker stack is the remaining verification (Phase 6).


- Scaffold `chat-ui/` from admin-ui tooling. Auth reuse. SSE chat loop (template:
  `admin-ui/src/api/testQuery.ts`). Markdown + code + Thai rendering (add a real
  markdown renderer — admin-ui has none). History sidebar (Phase 1 endpoints).
  Stop/regenerate/copy.
- Exit: multi-turn conversation persists + reloads; headed Playwright spec.

## Phase 4 — Differentiators

- Native inline citations (render `citation` events, ThaiRAG-styled).
- Inline source images (render `image` events inline — the OWUI blocker).
- Scope/workspace selector baked into chat (hard filter; supports the near-clone
  "one product per scope" guidance).
- Exit: image-bearing manual answer shows screenshots inline; live-stack e2e.

## Phase 5 — Attachments & ingestion-from-chat (v1 scope)

- Per-conversation file upload → existing ingest pipeline. Progress UI.

## Phase 6 — Hardening & parity gate

- Mobile/responsive, error/timeout/interrupt recovery (edge-action analysis).
- Dockerfile + nginx (clone admin-ui, port 8082) + compose service.
- Parity checklist = OWUI removal trigger: streaming chat, durable per-user
  history, citations, images, scope selector, login (native+OIDC), mobile,
  file upload.

## Phase 7 — OWUI decommission (only after parity)

- Remove OWUI service from compose + `webui.db` volume.
- Remove OWUI-only backend code: `x-openwebui-user-email` resolution +
  `is_openwebui` branching in `chat.rs`/`v2_chat.rs`/`ws_chat.rs`,
  `owui_feedback_sync.rs`, Keycloak/OIDC bits used only by OWUI. Keep `/v1`
  OpenAI-compat surface + citation viewer. Update CLAUDE.md citation section.

## Sequencing

1→2→3→4 strictly ordered. 5 parallelizable. 6–7 gated on parity. ~7–8 PRs to MVP
(end of Phase 3); Phases 1–2 carry the most risk (new backend surface).
