# OWUI → chat-ui parity audit & Phase 7 cutover plan

Goal: decommission Open WebUI (OWUI) once the first-party `chat-ui` covers what
real end users rely on. This is the go/no-go checklist + the concrete removal
steps. Built after shipping #245–#254.

## Capability matrix (end-user surface)

| Capability | OWUI today | chat-ui | Status |
|---|---|---|---|
| Streaming chat | ✓ | ✓ (`/api/chat`) | **done** |
| Durable per-user history | ✓ (`webui.db`) | ✓ (Postgres) | **done** |
| Source citations | ✓ (smuggled inline modals) | ✓ native chips + viewer | **done (better)** |
| Inline source images | ✗ (can't cleanly) | ✓ (default-off) | **done** (see G6) |
| File upload | ✓ (full-context) | ✓ per-conversation (#254) | **done** |
| Markdown / code / Thai | ✓ | ✓ | **done** |
| Scope / workspace filter | ✗ | ✓ (#253) | **done (differentiator)** |
| Login | Keycloak **SSO** | native JWT only | **GAP — G1** |
| Stop / regenerate / edit | ✓ | ✗ | **GAP — G2** |
| Mobile / responsive | ✓ | partial | **GAP — G3** |
| Error / interrupt recovery | ✓ | partial | **GAP — G4** |
| Feedback (thumbs) | ✓ (feedback sync) | ✗ | gap — G5 (optional) |
| Conversation rename UI | ✓ | API only, no button | gap — G7 (minor) |
| Model picker | ✓ | single model by design | not needed |
| Admin / user mgmt / settings | ✓ | (admin-ui owns this) | not needed |

## Gaps to close before cutover

- **G1 — OIDC/SSO login (BLOCKER, *if* your OWUI users authenticate via Keycloak).**
  OWUI end users sign in with Keycloak SSO; chat-ui only has native email/password.
  Cutting OWUI without SSO would lock those users out. The backend already has
  OIDC (`crates/thairag-api/src/oidc.rs`, `/api/auth/oauth/{provider}/authorize`
  + `/callback`); chat-ui needs a "Sign in with SSO" button + callback handling.
  Locked decision was "auth configurable (native+OIDC)" — this is that. *Medium.*
  → If real users do NOT use SSO (native only), G1 drops to not-needed.
- **G2 — Stop / regenerate / edit.** Table-stakes chat controls. Stop needs the
  stream's AbortController wired to a button; regenerate re-sends the last user
  turn; edit-and-resend rewrites it. *Small–medium.*
- **G3 — Mobile/responsive.** Sider collapses at `md` but there's no toggle to
  reopen it on mobile; verify composer/bubbles/scroll on a phone viewport. *Small.*
- **G4 — Error/interrupt recovery.** Per the team's "edge-action analysis before
  PR" rule: mid-stream disconnect, refresh during send, double-submit, send to a
  deleted conversation. *Small.*
- **G5 — Feedback thumbs (optional).** `/v1/chat/feedback` exists; OWUI feedback
  was judged low-value at current scale and left off. Skip for cutover; add later
  if wanted.
- **G6 — Inline images live (config).** Works but `inline_images_enabled` is
  default-off and read from *static boot config*. Move it (and `citation_base_url`)
  to the admin-toggleable effective config so it's not deploy-time-only. *Small.*
- **G7 — Rename UI (minor).** Rename endpoint exists; add a sidebar action.

## Go / no-go gate

Cutover when **G1 (if applicable), G2, G3, G4 are done** and a final live
headed-e2e parity pass is green (login → stream → citations → images → upload →
scope → stop/regenerate, on desktop + mobile viewport).

## Phase 7 — decommission steps (after the gate)

1. **Compose:** remove the OWUI service from `docker-compose.test-idp.yml` and the
   commented block in `docker-compose.yml`; drop the `open-webui-data`
   (`webui.db`) volume.
2. **Backend OWUI-only code to delete:**
   - `x-openwebui-user-email` resolution + `is_openwebui` branching in
     `routes/chat.rs`, `routes/v2/v2_chat.rs`, `routes/ws_chat.rs`.
   - `build_owui_source_events` in `chat.rs` (the smuggled-citation shape).
   - `owui_feedback_sync.rs` + its `spawn_owui_feedback_sync` call in `main.rs`.
3. **KEEP (do not remove):**
   - `/v1` OpenAI-compatible surface — external API clients still use it.
   - The citation viewer (`/v1/citation/{doc_id}`) + media route — used by chat-ui.
   - **`oidc.rs` + OAuth routes** — repurposed for chat-ui SSO (G1). *(This revises
     the earlier roadmap note that said remove OIDC; OIDC stays if G1 ships.)*
4. **Docs:** update the CLAUDE.md "Citations" section (the OWUI smuggling
   explanation becomes historical).
5. Update `docs/ARCHITECTURE.md` if it references OWUI as the chat client.

## Recommended sequence

G1 (if SSO) → G2 → G3 + G4 (one hardening PR) → G6 (cheap) → parity e2e pass →
Phase 7 removal PR. G5/G7 anytime or post-cutover.
