# CLAUDE.md

Guidance for working in the ThaiRAG repository.

## What this is

Production RAG platform: a Rust workspace (13 crates, edition 2024) exposing an
OpenAI-compatible API, plus a React/Vite admin UI. Layered crate graph:
core → config/thai/auth → providers/document → search/agent → api. Architecture
detail in `docs/ARCHITECTURE.md`.

## Dev workflow

- **The app runs in Docker, not locally.** Use `./scripts/docker-rebuild.sh
  [service]` to rebuild — it backs up the DB first (always back up before a
  rebuild). Stack: thairag (8080), open-webui (3000, pinned v0.9.6), keycloak
  (9090), postgres (5432), redis, qdrant, admin-ui (8081), native Ollama (11435).
- **Always `cargo fmt`** before committing — CI fails on formatting.
- `cargo clippy` and `cargo test` must be clean.
- E2E: Playwright specs in `admin-ui/e2e/` (`npm run test:e2e`, headed). Some are
  live-stack specs that need the Docker stack up; they are not in the headless CI
  gate.
- **PR workflow, not direct-to-main**: feature branch + `gh pr create`; `main`
  requires a PR.

## Citations (Open WebUI inline source modals)

Streaming chat answers carry clickable citations so users can see the source
behind each claim. The shape is client-specific, chosen in
`crates/thairag-api/src/routes/chat.rs` (`handle_stream`), gated on the
`is_openwebui` flag (set when the request carries the forwarded
`x-openwebui-user-email` header):

- **Open WebUI** → content-bearing `{"event":{"type":"source",...}}` chunks
  (`build_owui_source_events`). The `document` array holds the real retrieved
  snippets and `metadata.source` is a **non-URL doc id** — OWUI v0.9.6's
  middleware dispatches these to its event emitter and renders them as one-click
  **inline** citation modals showing the snippet text (no new tab). A signed
  citation `url` rides along as the modal's "view full" link. Stock OWUI is
  unmodified — all logic is ThaiRAG-side, speaking OWUI's existing protocol.
- **Other clients** → portable OpenAI-standard `delta.annotations[].url_citation`
  plus a plain-text `Sources:` footer. The footer is suppressed for OWUI to avoid
  a duplicate.

The citation `url` points at the public viewer `GET /v1/citation/{doc_id}?token=…`
(`chat::view_citation`), which renders the document's title + converted text as
HTML. The token is a short-lived (24h), single-doc JWT signed with the existing
`jwt_secret` (`JwtService::encode_citation` / `decode_citation` in
`thairag-auth`); a browser click needs no auth header — the token authorizes it.

Config: `chat_pipeline.citation_base_url` (default empty = no link, falls back to
the opaque `thairag:///doc/{id}` scheme). It is a **deploy-time** setting — chat.rs
reads the static config, so set it via env/config and restart (not the hot-reload
settings API). It must be **browser-reachable** (e.g. `http://localhost:8080`
locally; a public hostname in container deploys, not the internal `thairag:8080`).

Coverage: unit tests in `chat.rs` (`citation_tests`) and a live-stack e2e gate
`admin-ui/e2e/owui-citations.spec.ts` that asserts the inline rendering end-to-end.
