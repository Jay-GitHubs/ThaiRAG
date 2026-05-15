# Per-request attachments: design

**Status:** Accepted — four scoping decisions locked in 2026-05-15. PR-1 (backend) and PR-2 (admin UI) are unblocked; vision OCR / citations / per-tenant quotas remain as future PRs.

**Owner:** TBD
**Last updated:** 2026-05-15

---

## 1. Problem

ThaiRAG currently supports two ways for chat content to reach the LLM:

- **Embedded KB** — ingest once, retrieve top-K relevant chunks per request via hybrid search.
- **Live retrieval** — fan out to MCP connectors at chat time when KB coverage is poor; transient, not persisted.

It does **not** have a first-class "drop a document and ask about it in this conversation" pathway — the claude.ai / ChatGPT pattern. Today this is only possible by having the client paste the extracted text into `messages[].content` itself, which has four practical problems:

1. The client has to extract text from PDF / DOCX / XLSX / HTML itself (or the user copy-pastes).
2. The pipeline still runs hybrid search and possibly live retrieval — wasted work, wasted tokens on per-agent LLM calls (rewriter, analyzer, etc.).
3. Inference logs store the full pasted text (PDPA + storage concern at scale).
4. Multi-turn follow-up questions require the client to re-paste the entire document.

We want this third pathway as a first-class feature.

## 2. Threat-model / out-of-scope clarifications

In scope:
- Single-conversation, per-session attachment: "for this chat, use these documents."
- Format support matching the existing upload pipeline (PDF, DOCX, XLSX, HTML, Markdown, CSV, plain text).
- Guardrails enforcement on attachment content (operator policy still applies).

Out of scope (future PRs):
- Vision OCR for images embedded in PDFs, screenshots, etc. The existing `ChatMessage.images` field handles native image attachments; OCR-from-PDF is a separate problem.
- Citation support (the LLM annotating which attachment a claim came from). Anthropic's API has a `citations` feature that could plug in; see §8.
- Per-tenant quota enforcement (max attachments per month, total size). Trivial to add; deferred until usage data informs the right defaults.
- Persistent attachments shared across sessions. The current Documents path covers that — attachments here are deliberately transient.

## 3. Design decisions

These four were resolved before implementation began. The recommended path was chosen in each case.

### 3.1 Multi-turn behavior → **persist in session**

Attachments uploaded on turn 1 of a conversation remain available for follow-up questions on turn 2, 3, etc., without the client re-sending them.

- Mechanism: extend the existing `SessionStore` to hold a list of `SessionAttachment` records keyed by `session_id`, populated when an attachment-bearing chat request arrives. Subsequent requests in the same session see the previous attachments injected into context.
- Lifetime: the standard session 1-hour idle expiry already handles eviction. Explicit `DELETE /api/km/sessions/{sid}/attachments` is a stretch goal, not PR-1 scope.
- Memory cost: bounded by per-tenant attachment limits (see §6). At default limits, a 1-hour-old session holds ≤ ~2 MB of attachment text. Negligible per-replica.

### 3.2 Admin UI surface → **API + UI shipped together**

PR-1 ships the backend; PR-2 ships a paperclip button on the Test Chat page plus a chip-style indicator showing which attachments are active in the current session. Both target the same release. External API clients (Open WebUI, custom integrations) can use the API the moment PR-1 lands; the UI is the canonical surface for super-admin and operator validation.

### 3.3 Guardrails on attachment content → **yes, run input guardrails**

Attachment text passes through the same input-guardrail detectors as the user prompt. Rationale: a user could otherwise smuggle PII / secrets / blocked phrases into the chat context by uploading a file instead of typing them. The detector pass on attachment text shares the existing `InputGuardrails::check` plumbing — no new detector code is needed, just an extra invocation per attachment.

- Cost: O(attachment_size × num_detectors). Each detector is a single regex pass. For a 100 KB attachment this is sub-50ms even on free-tier hardware.
- Verdict handling: if an attachment fires `Block`, the whole request is refused (matches today's behavior when the prompt itself triggers `Block`). If `Sanitize`, the redacted attachment text is what reaches the LLM.

### 3.4 Citation support → **deferred to a later PR**

PR-1 ships the "drop-doc-ask-question" flow without citation annotations. The LLM may informally reference *"according to the document you provided"* via prompting, but there's no structured per-claim citation.

Rationale: citations are a real feature on their own with non-trivial UX implications (do we render span highlights? do we link back to the attachment chunk?). The current `Anthropic` provider could leverage the `citations` API feature when we get to it, but bundling it into PR-1 would double the scope. Tracked separately.

## 4. Request schema

The chat completions endpoint gains an optional `attachments` field. The wire format follows the same shape as `ChatMessage.images` (base64 + MIME) — this is intentional symmetry.

```jsonc
POST /v1/chat/completions
{
  "model": "ThaiRAG-1.0",
  "messages": [{ "role": "user", "content": "Summarize section 3" }],
  "stream": true,
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "attachments": [
    {
      "name": "Q3-Report.pdf",
      "mime_type": "application/pdf",
      "data": "JVBERi0xLjQK..."
    },
    {
      "name": "FY2025-Plan.docx",
      "mime_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      "data": "UEsDBBQACAg..."
    }
  ]
}
```

Field is optional; absent means today's behavior. The field is declared on both `/v1/chat/completions` and `/v2/chat/completions`.

For non-base64 transport (eyeing future multipart support), a streaming-friendly upload variant — `POST /v1/chat/completions/multipart` — is sketched in §8 but not in PR-1.

## 5. Pipeline behavior

When `attachments` is present (length ≥ 1), the chat pipeline routes differently:

```
Request arrives with attachments
        │
        ▼
   Validate attachments (size, count, mime — §6)
        │
        ▼
   For each attachment:
     thairag_document::convert(bytes, mime_type) → text
        │
        ▼
   For each extracted text:
     InputGuardrails::check(text)
        ├─ Block  → refuse whole request (refusal stream)
        ├─ Sanitize → use redacted text
        └─ Pass   → use as-is
        │
        ▼
   Persist to SessionStore::attach(session_id, [SessionAttachment])
        │
        ▼
   Build augmented messages:
     system: "You have been given the following document(s). Use them to answer."
     system: "[Document: Q3-Report.pdf]\n<extracted text>\n"
     system: "[Document: FY2025-Plan.docx]\n<extracted text>\n"
     ... user messages from request ...
        │
        ▼
   SKIP hybrid search (no embedded KB lookup)
   SKIP live retrieval (no MCP fan-out)
   SKIP query rewriter / analyzer (they'd waste tokens on per-agent calls)
        │
        ▼
   LlmProvider::generate / generate_stream
        │
        ▼
   Streaming output guardrails (unchanged — sliding-window hold-back)
        │
        ▼
   Response → client
```

On **follow-up turns in the same session** without re-sent attachments:
- The pipeline checks `SessionStore::get_attachments(session_id)` at the start of processing.
- If non-empty, attachments are injected into the system context the same way, KB / live paths are still skipped.
- The user can clear attachments by either explicit `DELETE` (future) or session expiry.

If `attachments` is **non-empty on a follow-up turn**, the new set replaces the session's prior attachments — this matches user intent (*"now ask about this new doc"*) and avoids unbounded session growth.

## 6. Limits and validation

Reject requests that exceed these:

| Limit | Default | Configurable via |
|---|---|---|
| Max attachments per request | 5 | `attachments.max_per_request` |
| Max bytes per attachment | 5 MB | `attachments.max_bytes_per_attachment` |
| Max total bytes per request | 15 MB | `attachments.max_total_bytes` |
| Max extracted text chars (post-conversion) | 200 000 | `attachments.max_text_chars` |
| Max attachments retained in a session | 10 (oldest evicted) | `attachments.max_session_attachments` |
| Allowed MIME types | The DocumentPipeline's existing supported list | `document.allowed_mime_types` (reuses existing config) |

Limits are enforced server-side. Validation errors return `400` with the specific reason; this is observable in the audit log and counted in `http_requests_total{status="400"}`.

## 7. Phasing

### PR-1 — Backend (no UI yet)

Files touched (sketch):

- `crates/thairag-config/src/schema.rs` — new `AttachmentsConfig` struct with the §6 fields.
- `crates/thairag-core/src/types.rs` — new `Attachment` and `SessionAttachment` types; add `attachments: Option<Vec<Attachment>>` to `ChatCompletionRequest`.
- `crates/thairag-core/src/traits.rs` — extend `SessionStoreTrait` with `attach`, `get_attachments`, `clear_attachments`.
- `crates/thairag-api/src/session.rs` + `crates/thairag-provider-redis/src/session.rs` — implement the new trait methods on both in-memory and Redis stores.
- `crates/thairag-agent/src/chat_pipeline.rs` — new `process_with_attachments` branch in the main `process` / `process_stream` methods. Skip search/orchestrator when attachments are active.
- `crates/thairag-api/src/routes/chat.rs` + `v2_chat.rs` — accept the new field, run document conversion via `thairag_document::DocumentPipeline::convert`, run input guardrails per attachment, persist to session.
- `crates/thairag-api/src/metrics.rs` — new `attachments_total{mime}` counter, `attachment_extraction_duration_seconds{mime}` histogram. Bounded label space.

Tests:
- Happy-path single attachment, PDF and DOCX.
- Multi-attachment request.
- Multi-turn: attach on turn 1, ask about it on turn 2 without re-sending.
- Replace-on-resend: attach A on turn 1, attach B on turn 2 — B replaces A.
- Guardrail Block on attachment text refuses whole request.
- Guardrail Sanitize on attachment text replaces only the offending span.
- Size / count / mime-type validation errors.
- Inference log records metadata only (size, mime, hash, count) — never the extracted text.

### PR-2 — Admin UI

- `admin-ui/src/api/attachments.ts` — base64 encode + POST helper.
- `admin-ui/src/pages/TestChatPage.tsx` — paperclip button next to the input, file picker, chip-style indicator for the currently-attached files.
- Inference-log page already exists; verify it surfaces attachment-bearing requests with a small chip.

### Later PRs (not committed to a date)

- **Vision OCR pipeline**: for PDFs with embedded images / scanned PDFs. Reuse the existing `multimodal_rag.rs` machinery.
- **Citation support**: leverage Anthropic's `citations` API feature for the Claude provider; format as inline footnotes for other providers. Needs its own UX call (highlight spans? hover cards? footnote table?).
- **Per-tenant quotas**: monthly attachment count + total bytes ceilings. Surfaced on the Usage admin page.
- **Multipart upload variant**: `POST /v1/chat/completions/multipart` so large attachments don't have to be base64-encoded inline.

## 8. Open questions remaining

None blocking PR-1. These are explicitly future-PR questions:

- **Citation UX** when we get to it: inline footnotes vs hover cards vs a sidebar reference panel.
- **Vision attachments interaction**: today `ChatMessage.images` handles vision content. When a user uploads a PDF *and* an image, do we treat them as one combined attachment set, or two parallel streams (text via `attachments`, image via `images`)?
- **Cross-session attachment sharing**: nice-to-have for power users ("save this attachment for next time"). Equivalent feature exists as Documents today, so probably won't add.

## 9. Out of scope for this design

- Anything related to large-corpus retrieval — that's the existing embedded-KB pathway, untouched.
- Anything related to streaming-only sources — that's live retrieval, untouched.
- Document persistence at rest — by design, attachments here are transient session-scoped.
