use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_agent::context_compactor::{self, ContextCompactor};
use thairag_agent::conversation_memory::MemoryEntry;
use thairag_agent::guardrails::{GuardAction, InputGuardrails};
use thairag_agent::personal_memory::PersonalMemoryManager;
use thairag_agent::tool_router::SearchableScope;
use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::permission::AccessScope;
use thairag_core::traits::DocumentProcessor;
use thairag_core::types::{
    Attachment, ChatAnnotation, ChatChoice, ChatChunkChoice, ChatChunkDelta, ChatCompletionChunk,
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatUsage, DocId,
    LlmStreamResponse, MetadataCell, PersonalMemory, PipelineMetadata, SessionAttachment,
    SessionId, UrlCitation, UserId,
};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::feedback;
use crate::store::{InferenceLogEntry, LineageRecord, SearchAnalyticsEvent};

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    // ── Request validation ──────────────────────────────────────────
    if req.messages.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "messages must not be empty".into(),
        )));
    }
    if req.model != "ThaiRAG-1.0" {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "model not found: {}",
            req.model
        ))));
    }

    // LLM01/LLM10: Input size validation
    let max_messages = state.config.server.max_chat_messages;
    if req.messages.len() > max_messages {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "too many messages: {} (max {})",
            req.messages.len(),
            max_messages
        ))));
    }
    let max_msg_len = state.config.server.max_message_length;
    for (i, msg) in req.messages.iter().enumerate() {
        if msg.content.len() > max_msg_len {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "message[{i}] content too long: {} chars (max {max_msg_len})",
                msg.content.len()
            ))));
        }
    }

    // LLM10: Per-user concurrent request limiting
    let _request_guard = state
        .user_request_limiter
        .try_acquire(&claims.sub)
        .map_err(|()| {
            ApiError(ThaiRagError::Validation(
                "Too many concurrent requests. Please wait for your previous request to complete."
                    .into(),
            ))
        })?;

    // LLM10: Per-user token-bucket rate limiting
    if claims.sub != "anonymous" {
        state
            .user_rate_limiter
            .try_acquire(&claims.sub)
            .map_err(|retry_after| {
                ApiError(ThaiRagError::Validation(format!(
                    "User rate limit exceeded. Retry after {:.0} seconds.",
                    retry_after.ceil()
                )))
            })?;
    }

    // ── Session handling ────────────────────────────────────────────
    let session_id = match &req.session_id {
        Some(id_str) => {
            let uuid = id_str.parse::<Uuid>().map_err(|_| {
                ApiError(ThaiRagError::Validation(format!(
                    "invalid session_id: {id_str}"
                )))
            })?;
            Some(SessionId(uuid))
        }
        None => None,
    };

    // Prepend history to messages if session exists
    let full_messages = if let Some(sid) = session_id {
        let mut msgs = state
            .session_store
            .get_history(&sid)
            .await
            .unwrap_or_default();
        msgs.extend(req.messages.clone());
        msgs
    } else {
        req.messages.clone()
    };

    // ── Attachment handling ─────────────────────────────────────────
    // New attachments on this request are decoded, converted, guardrail-
    // checked, and (when a session exists) persisted so follow-up turns can
    // reference them. Absent new attachments, pick up any persisted earlier.
    let attachments: Vec<SessionAttachment> = match req.attachments.as_deref() {
        Some(raw) if !raw.is_empty() => {
            let processed = process_request_attachments(&state, raw)?;
            if let Some(sid) = session_id {
                state.session_store.attach(&sid, processed.clone()).await;
            }
            processed
        }
        _ => {
            if let Some(sid) = session_id {
                state.session_store.get_attachments(&sid).await
            } else {
                Vec::new()
            }
        }
    };

    // ── Scope resolution ────────────────────────────────────────────
    // anonymous (auth disabled) and api-key (machine-to-machine) have no
    // concrete user; a JWT carries the user id in `sub`.
    let user_id = if claims.sub == "anonymous" || claims.sub == "api-key" {
        None
    } else {
        claims.sub.parse::<Uuid>().ok().map(UserId)
    };

    let scope = if let Some(uid) = user_id {
        let ws_ids = state.km_store.get_user_workspace_ids(uid);
        if ws_ids.is_empty() {
            AccessScope::none()
        } else {
            AccessScope::new(ws_ids)
        }
    } else if claims.sub == "anonymous" {
        // Auth disabled: unrestricted for dev/testing convenience
        AccessScope::unrestricted()
    } else if claims.sub == "api-key" {
        // API key without forwarded user email: unrestricted (machine-to-machine)
        AccessScope::unrestricted()
    } else {
        // JWT user whose UUID didn't parse: no access
        AccessScope::none()
    };

    // ── Resolve settings scope for multi-tenant LLM config ─────────
    let settings_scope = scope
        .workspace_ids
        .first()
        .map(|ws_id| state.resolve_scope_for_workspace(*ws_id))
        .unwrap_or(crate::store::SettingsScope::Global);

    // ── Load conversation memories (Feature 1) ─────────────────────
    let memories = load_memories(&state, user_id);

    // ── Context Compaction (Claude Code style) ──────────────────────
    let full_messages = maybe_compact_context(&state, full_messages, session_id, user_id).await;

    // ── Message-count Auto-Summarization ─────────────────────────────
    let full_messages = maybe_auto_summarize(&state, full_messages, session_id, user_id).await;

    // ── Personal Memory Retrieval (Per-User RAG) ────────────────────
    let personal_memories = retrieve_personal_memories(&state, user_id, &full_messages).await;

    // ── Build available scopes for tool router (Feature 3) ─────────
    let available_scopes = build_searchable_scopes(&state, &scope);

    if req.stream {
        handle_stream(
            state,
            req,
            full_messages,
            scope,
            session_id,
            memories,
            available_scopes,
            user_id,
            personal_memories,
            settings_scope,
            attachments,
        )
        .await
    } else {
        handle_non_stream(
            state,
            req,
            full_messages,
            scope,
            session_id,
            memories,
            available_scopes,
            user_id,
            personal_memories,
            settings_scope,
            attachments,
        )
        .await
    }
}

/// Decode, convert, size-check, and guardrail-check the per-request
/// attachments. Returns the processed list, or the first validation/guardrail
/// failure as an `ApiError` (surfaced to the client as a 400).
///
/// This is synchronous: base64 decode, document conversion, and the
/// deterministic guardrail detectors are all CPU-bound.
pub(crate) fn process_request_attachments(
    state: &AppState,
    raw: &[Attachment],
) -> Result<Vec<SessionAttachment>, ApiError> {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let cfg = &state.config.attachments;

    if raw.len() > cfg.max_per_request {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "too many attachments: {} (max {})",
            raw.len(),
            cfg.max_per_request
        ))));
    }

    let guard = InputGuardrails::new(
        crate::routes::settings::get_effective_chat_pipeline(state).guardrails,
    );
    let converter = thairag_document::converter::MarkdownConverter::new();

    let mut total_bytes = 0usize;
    let mut out = Vec::with_capacity(raw.len());

    for (i, a) in raw.iter().enumerate() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(a.data.as_bytes())
            .map_err(|_| {
                ApiError(ThaiRagError::Validation(format!(
                    "attachment[{i}] '{}': invalid base64 data",
                    a.name
                )))
            })?;

        if bytes.len() > cfg.max_bytes_per_attachment {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "attachment[{i}] '{}' too large: {} bytes (max {})",
                a.name,
                bytes.len(),
                cfg.max_bytes_per_attachment
            ))));
        }
        total_bytes += bytes.len();
        if total_bytes > cfg.max_total_bytes {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "attachments total size exceeds {} bytes",
                cfg.max_total_bytes
            ))));
        }

        let t = Instant::now();
        let extracted = converter.convert(&bytes, &a.mime_type);
        let extraction_secs = t.elapsed().as_secs_f64();

        let text = match extracted {
            Ok(text) => text,
            Err(e) => {
                state
                    .metrics
                    .record_attachment(&a.mime_type, "error", extraction_secs);
                return Err(ApiError(ThaiRagError::Validation(format!(
                    "attachment[{i}] '{}': {e}",
                    a.name
                ))));
            }
        };

        // Truncate over-long extractions to the configured char ceiling.
        let mut text: String = text.chars().take(cfg.max_text_chars).collect();

        // Input guardrails on the extracted text — a user must not be able to
        // smuggle PII/secrets/blocked phrases in via a file upload.
        let verdict = guard.check(&text);
        match verdict.action {
            GuardAction::Pass | GuardAction::Regenerate { .. } => {}
            GuardAction::Sanitize(redacted) => text = redacted,
            GuardAction::Block { reason } => {
                state
                    .metrics
                    .record_attachment(&a.mime_type, "error", extraction_secs);
                return Err(ApiError(ThaiRagError::Validation(format!(
                    "attachment[{i}] '{}' rejected by guardrails: {reason}",
                    a.name
                ))));
            }
        }

        let content_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            format!("{:x}", hasher.finalize())
        };

        state
            .metrics
            .record_attachment(&a.mime_type, "success", extraction_secs);
        // Retain raw image bytes only for image uploads when CLIP visual search
        // is enabled, so the attachment can drive image→image KB retrieval.
        let image_bytes =
            if a.mime_type.starts_with("image/") && state.providers().image_embedding.is_some() {
                Some(bytes.clone())
            } else {
                None
            };

        out.push(SessionAttachment {
            name: a.name.clone(),
            mime_type: a.mime_type.clone(),
            text,
            size_bytes: bytes.len(),
            content_hash,
            image_bytes,
        });
    }

    Ok(out)
}

/// Inject personal memory context as a system message at the beginning of the conversation.
pub(crate) fn inject_personal_memory_context(
    mut messages: Vec<ChatMessage>,
    personal_memories: &[PersonalMemory],
) -> Vec<ChatMessage> {
    if let Some(ctx_msg) = PersonalMemoryManager::build_memory_context(personal_memories) {
        messages.insert(0, ctx_msg);
    }
    messages
}

/// Persist cumulative token usage to KV store so it survives restarts.
pub(crate) fn persist_usage(state: &AppState, prompt: u32, completion: u32) {
    let key = "usage:tokens";
    let (prev_prompt, prev_completion) = state
        .km_store
        .get_setting(key)
        .and_then(|v| serde_json::from_str::<(u64, u64)>(&v).ok())
        .unwrap_or((0, 0));
    let new_prompt = prev_prompt + prompt as u64;
    let new_completion = prev_completion + completion as u64;
    if let Ok(json) = serde_json::to_string(&(new_prompt, new_completion)) {
        state.km_store.set_setting(key, &json);
    }
}

/// Build a markdown "Sources" footer from pipeline metadata for end-user
/// transparency (e.g. Open WebUI). Returns None when there's nothing to cite
/// or the feature is disabled.
pub(crate) fn build_source_footer(
    meta: &PipelineMetadata,
    enabled: bool,
    max: usize,
    response_id: &str,
    resolve_title: impl Fn(&str, Option<&str>) -> String,
) -> Option<String> {
    if !enabled || max == 0 || meta.retrieved_chunks.is_empty() {
        return None;
    }
    let mut sources: Vec<&thairag_core::types::RetrievedChunkMeta> = meta
        .retrieved_chunks
        .iter()
        .filter(|c| c.contributed)
        .collect();
    if sources.is_empty() {
        return None;
    }
    sources.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sources.truncate(max);

    let mut out = String::from("\n\n---\n**Sources:**\n");
    for (i, c) in sources.iter().enumerate() {
        let title = resolve_title(&c.doc_id, c.doc_title.as_deref());
        out.push_str(&format!(
            "{}. *{}* — relevance {:.2}\n",
            i + 1,
            title,
            c.score
        ));
    }
    out.push_str(&format!("\n_Response ID: `{response_id}`_"));
    Some(out)
}

/// A single resolved citation source, positionally aligned to the answer's
/// inline `[N]` markers (index 0 → marker `[1]`) when citations are present,
/// or ranked by relevance when the answer carries no markers.
struct CitationSource {
    title: String,
    doc_id: String,
    /// 1-indexed primary page of the cited passage (first page the chunk spans),
    /// when the source format carries pages. Surfaced so users can locate and
    /// trust the citation in the original document.
    page: Option<usize>,
    /// Section/heading the cited passage belongs to, when known.
    section: Option<String>,
    /// A snippet of the cited chunk's text, used by the in-app source viewer to
    /// locate and highlight the passage within the full document.
    snippet: Option<String>,
}

/// Resolve a human-readable document title for native citations: prefer a title
/// already on the chunk, otherwise look the document up by id, finally fall back
/// to the raw id.
fn resolve_doc_title(state: &AppState, doc_id: &str, fallback: Option<&str>) -> String {
    if let Some(t) = fallback
        && !t.is_empty()
    {
        return t.to_string();
    }
    if let Ok(uuid) = doc_id.parse::<Uuid>()
        && let Ok(doc) = state.km_store.get_document(DocId(uuid))
        && !doc.title.is_empty()
    {
        return doc.title;
    }
    doc_id.to_string()
}

/// The 1-indexed primary page a retrieved chunk starts on, when its source
/// format carries page numbers.
fn primary_page(rc: &thairag_core::types::RetrievedChunkMeta) -> Option<usize> {
    rc.page_numbers.as_ref().and_then(|p| p.first().copied())
}

/// Resolve the per-source citation list used to drive native (clickable)
/// citations in compatible clients. Prefers the deterministically-parsed
/// `[N]` markers (so marker order is preserved); falls back to the
/// contributed retrieved chunks ranked by score when the answer has none.
///
/// `resolve_title` maps a `(doc_id, chunk_fallback_title)` pair to a display
/// title — injected so the ordering logic can be unit-tested without an
/// `AppState`/document store.
fn build_citation_sources(
    meta: &PipelineMetadata,
    max: usize,
    resolve_title: impl Fn(&str, Option<&str>) -> String,
) -> Vec<CitationSource> {
    use std::collections::HashMap;

    if !meta.citations.is_empty() {
        let max_marker = meta.citations.iter().map(|c| c.marker).max().unwrap_or(0);
        let mut by_marker: HashMap<u32, &thairag_core::types::Citation> = HashMap::new();
        for c in &meta.citations {
            by_marker.entry(c.marker).or_insert(c);
        }
        let mut out = Vec::with_capacity(max_marker as usize);
        for n in 1..=max_marker {
            if let Some(c) = by_marker.get(&n) {
                // The marker carries no page/section locator, so look the cited
                // chunk up by id in the retrieval to recover its provenance.
                let loc = meta
                    .retrieved_chunks
                    .iter()
                    .find(|r| r.chunk_id == c.chunk_id);
                out.push(CitationSource {
                    title: resolve_title(&c.doc_id, c.doc_title.as_deref()),
                    doc_id: c.doc_id.clone(),
                    page: loc.and_then(primary_page),
                    section: loc.and_then(|r| r.section_title.clone()),
                    snippet: loc.map(|r| r.content_preview.clone()),
                });
            } else if let Some(rc) = meta.retrieved_chunks.iter().find(|r| r.rank == n - 1) {
                // Marker gap: fill positionally from the ranked retrieval so the
                // client's `[N]` → source index mapping stays correct.
                out.push(CitationSource {
                    title: resolve_title(&rc.doc_id, rc.doc_title.as_deref()),
                    doc_id: rc.doc_id.clone(),
                    page: primary_page(rc),
                    section: rc.section_title.clone(),
                    snippet: Some(rc.content_preview.clone()),
                });
            } else {
                out.push(CitationSource {
                    title: format!("Source {n}"),
                    doc_id: String::new(),
                    page: None,
                    section: None,
                    snippet: None,
                });
            }
        }
        out
    } else {
        let mut sources: Vec<&thairag_core::types::RetrievedChunkMeta> = meta
            .retrieved_chunks
            .iter()
            .filter(|c| c.contributed)
            .collect();
        sources.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sources.truncate(max);
        sources
            .into_iter()
            .map(|c| CitationSource {
                title: resolve_title(&c.doc_id, c.doc_title.as_deref()),
                doc_id: c.doc_id.clone(),
                page: primary_page(c),
                section: c.section_title.clone(),
                snippet: Some(c.content_preview.clone()),
            })
            .collect()
    }
}

/// Build the `url` for a citation annotation. When a public base URL is
/// configured and we can mint a signed token, return a browser-openable link to
/// the citation viewer; otherwise fall back to the opaque `thairag:///doc/<id>`
/// identifier (carries the id but is not openable).
fn citation_url(
    state: &AppState,
    base: &str,
    doc_id: &str,
    page: Option<usize>,
    section: Option<&str>,
) -> String {
    if !base.is_empty()
        && let Some(jwt) = state.jwt.as_ref()
        && let Ok(token) = jwt.encode_citation(doc_id, 24)
    {
        return format!(
            "{base}/v1/citation/{doc_id}?token={token}{}",
            provenance_query(page, section),
        );
    }
    format!("thairag:///doc/{doc_id}")
}

/// Build the optional `&page=N&section=...` query suffix for citation viewer
/// links. `page` is 1-indexed (as stored); empty string when neither is set.
fn provenance_query(page: Option<usize>, section: Option<&str>) -> String {
    let mut q = String::new();
    if let Some(p) = page {
        q.push_str(&format!("&page={p}"));
    }
    if let Some(sec) = section.map(str::trim).filter(|s| !s.is_empty()) {
        q.push_str(&format!("&section={}", urlencode(sec)));
    }
    q
}

/// Minimal percent-encoding for query-string values (RFC 3986 unreserved set
/// stays literal; everything else is `%XX`). Avoids pulling in a URL crate for
/// the one place we build a citation link.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render the document's converted text for the citation viewer: prose stays as
/// plain (escaped) text shown pre-wrapped, while reconstructed `<table>` blocks
/// render as real tables. Sanitised with a strict allowlist (table tags +
/// colspan/rowspan only) so any HTML in the *document's own text* — e.g. a
/// stray `<script>` from the source — is stripped. Guaranteed-safe by allowlist
/// rather than by trusting the table's origin.
fn render_citation_html(content: &str) -> String {
    ammonia::Builder::default()
        .tags(std::collections::HashSet::from([
            "table", "thead", "tbody", "tr", "td", "th",
        ]))
        .tag_attributes(std::collections::HashMap::from([
            (
                "td",
                std::collections::HashSet::from(["colspan", "rowspan"]),
            ),
            (
                "th",
                std::collections::HashSet::from(["colspan", "rowspan"]),
            ),
        ]))
        .clean(content)
        .to_string()
}

#[derive(serde::Deserialize)]
pub struct CitationViewQuery {
    token: String,
    /// 1-indexed source page of the cited passage, when known.
    #[serde(default)]
    page: Option<usize>,
    /// Section/heading of the cited passage, when known.
    #[serde(default)]
    section: Option<String>,
}

/// Public, token-gated viewer for a cited source. The signed `token` (minted at
/// chat time, scoped to a single doc, 24h TTL) authorizes the request — no auth
/// header needed, so a citation link clicked in a chat client (e.g. Open WebUI)
/// opens the source directly in the browser. Returns a minimal HTML page with
/// the document title and its converted text.
pub async fn view_citation(
    State(state): State<AppState>,
    Path(doc_id): Path<Uuid>,
    Query(params): Query<CitationViewQuery>,
) -> Response {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Html("<h1>401</h1><p>Invalid or expired citation link.</p>".to_string()),
        )
            .into_response()
    };

    let Some(jwt) = state.jwt.as_ref() else {
        return unauthorized();
    };
    let Ok(granted_doc) = jwt.decode_citation(&params.token) else {
        return unauthorized();
    };
    // The token must grant exactly the doc in the path.
    if granted_doc != doc_id.to_string() {
        return unauthorized();
    }

    let doc_id_typed = DocId(doc_id);
    let Ok(doc) = state.km_store.get_document(doc_id_typed) else {
        return (
            StatusCode::NOT_FOUND,
            Html("<h1>404</h1><p>Document not found.</p>".to_string()),
        )
            .into_response();
    };

    let content = state
        .km_store
        .get_document_content(doc_id_typed)
        .unwrap_or(None)
        .unwrap_or_default();

    // "Cited from: Section X · page N" banner so a reader landing on the viewer
    // sees exactly where in the document the citation was drawn from.
    let mut prov_parts: Vec<String> = Vec::new();
    if let Some(sec) = params
        .section
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        prov_parts.push(format!("Section {}", escape_html(sec)));
    }
    if let Some(p) = params.page {
        prov_parts.push(format!("page {p}"));
    }
    let provenance = if prov_parts.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"prov\">Cited from: {}</div>",
            prov_parts.join(" · ")
        )
    };

    let page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title>\
<style>body{{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;\
max-width:820px;margin:2rem auto;padding:0 1rem;line-height:1.6;color:#1a1a1a}}\
h1{{font-size:1.4rem;border-bottom:1px solid #e0e0e0;padding-bottom:.5rem}}\
.src{{white-space:pre-wrap;word-wrap:break-word;background:#fafafa;\
border:1px solid #eee;border-radius:8px;padding:1rem;font-size:.95rem}}\
.src table{{border-collapse:collapse;margin:.5rem 0;white-space:normal}}\
.src td,.src th{{border:1px solid #ccc;padding:.3rem .5rem;vertical-align:top}}\
.prov{{background:#eef4ff;border:1px solid #cfe0ff;border-radius:8px;\
padding:.5rem .75rem;margin-bottom:1rem;font-size:.9rem;color:#1a3a6b}}\
.meta{{color:#888;font-size:.8rem;margin-bottom:1rem}}</style></head>\
<body><h1>{title}</h1><div class=\"meta\">Cited source · {doc_id}</div>\
{provenance}<div class=\"src\">{content}</div></body></html>",
        title = escape_html(&doc.title),
        doc_id = escape_html(&doc.id.0.to_string()),
        content = render_citation_html(&content),
    );

    Html(page).into_response()
}

/// Load conversation memory entries for a user from the KV store.
pub(crate) fn load_memories(state: &AppState, user_id: Option<UserId>) -> Vec<MemoryEntry> {
    let Some(uid) = user_id else { return vec![] };
    let key = format!("memory:{}", uid.0);
    state
        .km_store
        .get_setting(&key)
        .and_then(|json| serde_json::from_str::<Vec<MemoryEntry>>(&json).ok())
        .unwrap_or_default()
}

/// Save updated memories for a user.
fn save_memories(state: &AppState, user_id: UserId, memories: &[MemoryEntry], max: usize) {
    let mut entries = memories.to_vec();
    // Keep only the most recent N
    if entries.len() > max {
        entries.drain(..entries.len() - max);
    }
    let key = format!("memory:{}", user_id.0);
    if let Ok(json) = serde_json::to_string(&entries) {
        state.km_store.set_setting(&key, &json);
    }
}

/// Check if context compaction is needed and perform it if so.
pub(crate) async fn maybe_compact_context(
    state: &AppState,
    messages: Vec<ChatMessage>,
    session_id: Option<SessionId>,
    user_id: Option<UserId>,
) -> Vec<ChatMessage> {
    let p = state.providers();
    let Some(ref compactor) = p.context_compactor else {
        return messages;
    };
    let Some(uid) = user_id else {
        return messages;
    };
    let Some(sid) = session_id else {
        return messages;
    };

    let chat_config = &p.chat_pipeline_config;
    let context_window = chat_config.model_context_window;
    let threshold = chat_config.compaction_threshold;
    let keep_recent = chat_config.compaction_keep_recent;
    let rag_budget = chat_config.max_context_tokens;

    if !ContextCompactor::needs_compaction(&messages, context_window, threshold, rag_budget) {
        return messages;
    }

    tracing::info!(
        user_id = %uid,
        session_id = %sid,
        msg_count = messages.len(),
        "Context compaction triggered"
    );

    match compactor.compact(&messages, keep_recent, uid).await {
        Ok(result) => {
            if result.messages_compacted == 0 {
                return messages;
            }

            // Store extracted personal memories in background
            if !result.extracted_memories.is_empty()
                && let Some(ref pm) = p.personal_memory_manager
            {
                let pm = Arc::clone(pm);
                let memories = result.extracted_memories.clone();
                tokio::spawn(async move {
                    if let Err(e) = pm.store_memories(&memories).await {
                        tracing::warn!(error = %e, "Failed to store personal memories from compaction");
                    }
                });
            }

            // Build compacted messages
            let recent_start = messages.len().saturating_sub(result.messages_kept);
            let recent = &messages[recent_start..];
            let compacted = ContextCompactor::build_compacted_messages(&result.summary, recent);

            // Update session with compacted history
            state
                .session_store
                .replace_messages(&sid, compacted.clone())
                .await;

            tracing::info!(
                compacted = result.messages_compacted,
                kept = result.messages_kept,
                memories = result.extracted_memories.len(),
                "Context compaction complete"
            );

            compacted
        }
        Err(e) => {
            tracing::warn!(error = %e, "Context compaction failed, using original messages");
            messages
        }
    }
}

/// Check if message-count-based auto-summarization should run and perform it.
/// This summarizes older messages and replaces them with a summary system message,
/// keeping recent messages intact for immediate context.
pub(crate) async fn maybe_auto_summarize(
    state: &AppState,
    messages: Vec<ChatMessage>,
    session_id: Option<SessionId>,
    _user_id: Option<UserId>,
) -> Vec<ChatMessage> {
    let p = state.providers();
    let chat_config = &p.chat_pipeline_config;

    // Check if auto-summarization is enabled
    if !chat_config.auto_summarize {
        return messages;
    }

    let Some(sid) = session_id else {
        return messages;
    };

    let threshold = chat_config.summarize_threshold;
    let keep_recent = chat_config.summarize_keep_recent;

    // Only trigger when message count exceeds threshold
    if messages.len() < threshold {
        return messages;
    }

    // Check if we already summarized at this message count (avoid re-summarizing)
    if let Some((_summary, prev_count)) = state.session_store.get_summary(&sid).await
        && messages.len() <= prev_count + 4
    {
        // Already summarized recently, skip
        return messages;
    }

    // Build the LLM provider for summarization: prefer memory_llm > shared llm > global
    let llm: Arc<dyn thairag_core::traits::LlmProvider> =
        if let Some(ref cfg) = chat_config.memory_llm {
            Arc::from(thairag_provider_llm::create_llm_provider(cfg))
        } else if let Some(ref cfg) = chat_config.llm {
            Arc::from(thairag_provider_llm::create_llm_provider(cfg))
        } else {
            Arc::from(thairag_provider_llm::create_llm_provider(
                &p.providers_config.llm,
            ))
        };

    tracing::info!(
        session_id = %sid,
        msg_count = messages.len(),
        threshold,
        "Auto-summarization triggered"
    );

    // Summarize older messages
    let compact_end = messages.len().saturating_sub(keep_recent);
    if compact_end <= 1 {
        return messages;
    }

    let to_summarize = &messages[..compact_end];
    match context_compactor::summarize_conversation(llm.as_ref(), to_summarize).await {
        Ok(summary) if !summary.is_empty() => {
            let recent = &messages[compact_end..];
            let compacted = ContextCompactor::build_compacted_messages(&summary, recent);

            // Update session store
            state
                .session_store
                .replace_messages(&sid, compacted.clone())
                .await;
            state
                .session_store
                .set_summary(&sid, summary, messages.len())
                .await;

            tracing::info!(
                session_id = %sid,
                summarized = compact_end,
                kept = recent.len(),
                "Auto-summarization complete"
            );
            compacted
        }
        Ok(_) => messages,
        Err(e) => {
            tracing::warn!(error = %e, "Auto-summarization failed, using original messages");
            messages
        }
    }
}

/// Retrieve relevant personal memories for the current query.
pub(crate) async fn retrieve_personal_memories(
    state: &AppState,
    user_id: Option<UserId>,
    messages: &[ChatMessage],
) -> Vec<PersonalMemory> {
    let p = state.providers();
    let Some(ref pm) = p.personal_memory_manager else {
        return vec![];
    };
    let Some(uid) = user_id else {
        return vec![];
    };

    // Use the last user message as the query
    let query = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");

    if query.is_empty() {
        return vec![];
    }

    match pm.retrieve(uid, query).await {
        Ok(memories) => memories,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to retrieve personal memories");
            vec![]
        }
    }
}

/// Build searchable scopes from the user's accessible workspaces.
pub(crate) fn build_searchable_scopes(
    state: &AppState,
    scope: &AccessScope,
) -> Vec<SearchableScope> {
    if scope.is_unrestricted() {
        // For unrestricted access, list all workspaces
        state
            .km_store
            .list_workspaces_all()
            .into_iter()
            .map(|ws| SearchableScope {
                workspace_id: ws.id,
                name: ws.name,
                description: None,
            })
            .collect()
    } else {
        scope
            .workspace_ids
            .iter()
            .filter_map(|ws_id| {
                state
                    .km_store
                    .get_workspace(*ws_id)
                    .ok()
                    .map(|ws| SearchableScope {
                        workspace_id: ws.id,
                        name: ws.name,
                        description: None,
                    })
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_non_stream(
    state: AppState,
    req: ChatCompletionRequest,
    full_messages: Vec<ChatMessage>,
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
    personal_memories: Vec<PersonalMemory>,
    settings_scope: crate::store::SettingsScope,
    attachments: Vec<SessionAttachment>,
) -> Result<Response, ApiError> {
    // Inject personal memory context
    let full_messages = inject_personal_memory_context(full_messages, &personal_memories);

    // Inject golden examples as few-shot demonstrations
    let golden = feedback::load_golden_examples_for_workspace(&state, None);
    let augmented_messages = if golden.is_empty() {
        full_messages.clone()
    } else {
        let examples_text = golden
            .iter()
            .map(|ex| format!("Q: {}\nA: {}", ex.query, ex.answer))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let mut msgs = vec![ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Here are examples of high-quality answers for reference:\n\n{examples_text}\n\n\
                 Use these examples as a guide for style and quality, but answer based on the retrieved context."
            ),
            images: vec![],
        }];
        msgs.extend(full_messages.clone());
        msgs
    };

    let p = state.providers();
    let request_start = Instant::now();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();
    // Computed from the RAW client messages (before memory / golden-example
    // injection) so internal system additions never disarm the empty-context
    // guard — same signal the streaming path threads into process_stream.
    // OWUI traffic is exempt from the non-stream guard entirely: besides chats
    // (whose injected context is detected), OWUI fires auxiliary NON-STREAM
    // task calls (title/tag/follow-up generation) that carry no context and
    // must get plain LLM behavior — a canned refusal would become the chat
    // title. API clients (curl/bench/scripts) keep the guard, fixing the
    // stream-vs-non-stream divergence where stream refused but non-stream
    // answered from general knowledge.
    let has_external_context =
        thairag_agent::chat_pipeline::has_client_supplied_context(&req.messages);
    let mut llm_resp = if let Some(ref pipeline) = scoped_pipeline {
        if attachments.is_empty() {
            pipeline
                .process(
                    &augmented_messages,
                    &scope,
                    &memories,
                    &available_scopes,
                    Some(progress_tx),
                    Some(metadata_cell.clone()),
                    has_external_context,
                )
                .await
                .map_err(ApiError::from)?
        } else {
            pipeline
                .process_with_attachments(
                    &augmented_messages,
                    &attachments,
                    &memories,
                    &scope,
                    Some(progress_tx),
                    Some(metadata_cell.clone()),
                )
                .await
                .map_err(ApiError::from)?
        }
    } else {
        drop(progress_tx);
        p.orchestrator
            .process(&full_messages, &scope)
            .await
            .map_err(ApiError::from)?
    };
    // Collect pipeline stages for the response
    let pipeline_stages: Vec<thairag_core::types::PipelineProgress> =
        std::iter::from_fn(|| progress_rx.try_recv().ok()).collect();

    state.metrics.record_tokens(
        llm_resp.usage.prompt_tokens,
        llm_resp.usage.completion_tokens,
    );
    persist_usage(
        &state,
        llm_resp.usage.prompt_tokens,
        llm_resp.usage.completion_tokens,
    );

    let response_id = format!("chatcmpl-{}", Uuid::new_v4());

    // Append source footer for end-user transparency (e.g. Open WebUI).
    // Done before session save so memory + history retain the citations.
    // Snapshot the metadata so the lock guard never crosses an await.
    let footer_meta = metadata_cell.lock().unwrap().clone();
    if let Some(footer) = build_source_footer(
        &footer_meta,
        state.config.chat_pipeline.source_footer_enabled,
        state.config.chat_pipeline.source_footer_max,
        &response_id,
        |doc_id, fallback| resolve_doc_title(&state, doc_id, fallback),
    ) {
        llm_resp.content.push_str(&footer);
    }

    // Save to session
    if let Some(sid) = session_id
        && let Some(last_user_msg) = req.messages.last().cloned()
    {
        let assistant_msg = ChatMessage {
            role: "assistant".to_string(),
            content: llm_resp.content.clone(),
            images: vec![],
        };
        state
            .session_store
            .append(sid, last_user_msg.clone(), assistant_msg.clone(), user_id)
            .await;

        // Feature 1: Async memory summarization
        if let Some(uid) = user_id {
            maybe_summarize_memory(state.clone(), p.chat_pipeline.clone(), uid, sid, memories);
        }
    }

    let response_length = llm_resp.content.len() as u32;

    // ── Inference Logging + Analytics ─────────────────────────────
    {
        let total_ms = request_start.elapsed().as_millis() as u64;
        let meta = metadata_cell.lock().unwrap().clone();
        let user_query = req
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let pp = state.providers();
        let (llm_kind, llm_model) = resolve_llm_info(&pp);
        let entry = InferenceLogEntry {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            user_id: user_id.map(|u| u.0.to_string()),
            workspace_id: scope.workspace_ids.first().map(|w| w.0.to_string()),
            org_id: None,
            dept_id: None,
            session_id: session_id.map(|s| s.0.to_string()),
            response_id: response_id.clone(),
            query_text: user_query.chars().take(2000).collect(),
            detected_language: meta.language.clone(),
            intent: meta.intent.clone(),
            complexity: meta.complexity.clone(),
            llm_kind,
            llm_model,
            settings_scope: format!("{:?}", settings_scope),
            prompt_tokens: llm_resp.usage.prompt_tokens,
            completion_tokens: llm_resp.usage.completion_tokens,
            estimated_context_tokens: meta.estimated_context_tokens.unwrap_or(0),
            total_ms,
            search_ms: meta.search_ms,
            generation_ms: meta.generation_ms,
            chunks_retrieved: meta.chunks_retrieved,
            avg_chunk_score: meta.avg_chunk_score,
            self_rag_decision: meta.self_rag_decision.clone(),
            self_rag_confidence: meta.self_rag_confidence,
            quality_guard_pass: meta.quality_guard_pass,
            relevance_score: meta.relevance_score,
            hallucination_score: meta.hallucination_score,
            completeness_score: meta.completeness_score,
            pipeline_route: meta.pipeline_route.clone(),
            agents_used: serde_json::to_string(
                &pipeline_stages.iter().map(|s| &s.stage).collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".into()),
            status: "success".into(),
            error_message: None,
            response_length,
            feedback_score: None,
            input_guardrails_pass: meta.input_guardrails_pass,
            output_guardrails_pass: meta.output_guardrails_pass,
            guardrail_violation_codes: meta
                .guardrail_violations
                .iter()
                .map(|v| v.code.as_str())
                .collect::<Vec<_>>()
                .join(","),
        };

        // ── Search Analytics ──
        if let Some(search_ms) = meta.search_ms {
            let result_count = meta.chunks_retrieved.unwrap_or(0);
            let event = SearchAnalyticsEvent {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                query_text: user_query.chars().take(2000).collect(),
                user_id: user_id.map(|u| u.0.to_string()),
                workspace_id: scope.workspace_ids.first().map(|w| w.0.to_string()),
                result_count,
                latency_ms: search_ms,
                zero_results: result_count == 0,
            };
            let store = state.km_store.clone();
            tokio::spawn(async move {
                store.insert_search_event(&event);
            });
        }

        // ── Document Lineage ──
        if !meta.retrieved_chunks.is_empty() {
            let lineage_response_id = response_id.clone();
            let lineage_query = user_query.chars().take(2000).collect::<String>();
            let chunk_metas = meta.retrieved_chunks.clone();
            let store = state.km_store.clone();
            tokio::spawn(async move {
                let now = chrono::Utc::now().to_rfc3339();
                for chunk in &chunk_metas {
                    let record = LineageRecord {
                        id: Uuid::new_v4().to_string(),
                        response_id: lineage_response_id.clone(),
                        timestamp: now.clone(),
                        query_text: lineage_query.clone(),
                        chunk_id: chunk.chunk_id.clone(),
                        doc_id: chunk.doc_id.clone(),
                        doc_title: chunk.doc_title.clone(),
                        chunk_text_preview: chunk.content_preview.clone(),
                        score: chunk.score,
                        rank: chunk.rank,
                        contributed: chunk.contributed,
                    };
                    store.insert_lineage_record(&record);
                }
            });
        }

        let store = state.km_store.clone();
        tokio::spawn(async move {
            store.insert_inference_log(&entry);
        });
    }

    let mut response = serde_json::to_value(ChatCompletionResponse {
        id: response_id,
        object: "chat.completion".to_string(),
        created: Utc::now().timestamp(),
        model: "ThaiRAG-1.0".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: llm_resp.content,
                images: vec![],
            },
            finish_reason: "stop".to_string(),
        }],
        usage: ChatUsage {
            prompt_tokens: llm_resp.usage.prompt_tokens,
            completion_tokens: llm_resp.usage.completion_tokens,
            total_tokens: llm_resp.usage.prompt_tokens + llm_resp.usage.completion_tokens,
            thairag_response_id: None,
        },
    })
    .unwrap();

    if let Some(sid) = session_id {
        response["session_id"] = serde_json::Value::String(sid.to_string());
    }

    if !pipeline_stages.is_empty() {
        response["pipeline_stages"] = serde_json::to_value(&pipeline_stages).unwrap();
    }

    Ok(Json(response).into_response())
}

/// Trigger async memory summarization if enough turns have accumulated.
#[allow(clippy::too_many_arguments)]
fn maybe_summarize_memory(
    state: AppState,
    pipeline: Option<std::sync::Arc<thairag_agent::ChatPipeline>>,
    user_id: UserId,
    session_id: SessionId,
    existing_memories: Vec<MemoryEntry>,
) {
    let Some(pipeline) = pipeline else { return };
    if pipeline.conversation_memory().is_none() {
        return;
    }

    let max_summaries = 10usize;

    tokio::spawn(async move {
        // Only summarize every 5 turns (10 messages)
        let history = state.session_store.get_history(&session_id).await;
        let msg_count = history.as_ref().map(|h| h.len()).unwrap_or(0);
        if msg_count < 10 || !msg_count.is_multiple_of(10) {
            return;
        }

        let messages = history.unwrap_or_default();
        let p = state.providers();
        if let Some(ref pipeline) = p.chat_pipeline
            && let Some(mem) = pipeline.conversation_memory()
        {
            match mem.summarize(&messages).await {
                Ok(entry) => {
                    let mut all = existing_memories;
                    all.push(entry);
                    save_memories(&state, user_id, &all, max_summaries);
                    tracing::debug!(user_id = %user_id.0, "Conversation memory saved");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to summarize conversation for memory");
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn handle_stream(
    state: AppState,
    req: ChatCompletionRequest,
    full_messages: Vec<ChatMessage>,
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
    personal_memories: Vec<PersonalMemory>,
    settings_scope: crate::store::SettingsScope,
    attachments: Vec<SessionAttachment>,
) -> Result<Response, ApiError> {
    let id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = Utc::now().timestamp();
    let model = "ThaiRAG-1.0".to_string();

    // Inject personal memory context
    let full_messages = inject_personal_memory_context(full_messages, &personal_memories);

    // Inject golden examples as few-shot demonstrations
    let golden = feedback::load_golden_examples_for_workspace(&state, None);
    let augmented_messages = if golden.is_empty() {
        full_messages.clone()
    } else {
        let examples_text = golden
            .iter()
            .map(|ex| format!("Q: {}\nA: {}", ex.query, ex.answer))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let mut msgs = vec![ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Here are examples of high-quality answers for reference:\n\n{examples_text}\n\n\
                 Use these examples as a guide for style and quality, but answer based on the retrieved context."
            ),
            images: vec![],
        }];
        msgs.extend(full_messages.clone());
        msgs
    };

    let id_clone = id.clone();
    let model_clone = model.clone();
    let last_user_msg = req.messages.last().cloned();

    // Spawn the pipeline in a background task so the SSE stream can yield
    // progress events in real-time as each agent starts/completes.
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();

    let p = state.providers();
    let request_start = Instant::now();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let metadata_cell_clone = metadata_cell.clone();
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);
    let pipeline_for_memory = scoped_pipeline.clone();

    // Clone what the spawned task needs
    let augmented_messages_clone = augmented_messages.clone();
    let scope_clone = scope.clone();
    let memories_clone = memories.clone();
    let available_scopes_clone = available_scopes.clone();
    // Computed from the RAW request: when the client injects its own context
    // (an OWUI file upload arrives as a <context> block in the user message,
    // or a client sets its own system prompt), the empty-KB short-circuit
    // must not fire — the answer LLM should use that context. Streaming OWUI
    // chats WITHOUT context keep the guard (refusing on an empty KB is the
    // desired RAG behavior in chat).
    let has_external_context =
        thairag_agent::chat_pipeline::has_client_supplied_context(&req.messages);

    let pipeline_handle = tokio::spawn(async move {
        if let Some(ref pipeline) = scoped_pipeline {
            if attachments.is_empty() {
                pipeline
                    .process_stream(
                        &augmented_messages_clone,
                        &scope_clone,
                        &memories_clone,
                        &available_scopes_clone,
                        Some(progress_tx),
                        Some(metadata_cell_clone),
                        has_external_context,
                    )
                    .await
            } else {
                pipeline
                    .process_stream_with_attachments(
                        &augmented_messages_clone,
                        &attachments,
                        &memories_clone,
                        &scope_clone,
                        Some(progress_tx),
                        Some(metadata_cell_clone),
                    )
                    .await
            }
        } else {
            drop(progress_tx);
            p.orchestrator
                .process_stream(&augmented_messages_clone, &scope_clone)
                .await
        }
    });

    let sse_stream = async_stream::stream! {
        // Stream progress events in real-time while pipeline runs in background
        let mut pipeline_handle = pipeline_handle;
        let pipeline_result;
        let mut stage_names: Vec<String> = Vec::new();

        loop {
            tokio::select! {
                evt = progress_rx.recv() => {
                    match evt {
                        Some(progress) => {
                            if progress.status == thairag_core::types::StageStatus::Done
                                || progress.status == thairag_core::types::StageStatus::Error
                            {
                                stage_names.push(progress.stage.clone());
                            }
                            let data = serde_json::to_string(&progress).unwrap();
                            yield Ok::<_, std::convert::Infallible>(
                                Event::default().event("progress").data(data)
                            );
                        }
                        None => {
                            // Channel closed — sender dropped, pipeline must be done or about to be
                        }
                    }
                }
                result = &mut pipeline_handle => {
                    // Drain any remaining progress events
                    while let Ok(evt) = progress_rx.try_recv() {
                        if evt.status == thairag_core::types::StageStatus::Done
                            || evt.status == thairag_core::types::StageStatus::Error
                        {
                            stage_names.push(evt.stage.clone());
                        }
                        let data = serde_json::to_string(&evt).unwrap();
                        yield Ok::<_, std::convert::Infallible>(
                            Event::default().event("progress").data(data)
                        );
                    }
                    pipeline_result = match result {
                        Ok(r) => r,
                        Err(e) => Err(ThaiRagError::LlmProvider(format!("Pipeline task panicked: {e}"))),
                    };
                    break;
                }
            }
        }

        let LlmStreamResponse {
            stream: token_stream,
            usage: usage_cell,
        } = match pipeline_result {
            Ok(resp) => resp,
            Err(e) => {
                let error_data = serde_json::json!({
                    "error": { "message": e.to_string(), "type": "pipeline_error" }
                });
                yield Ok::<_, std::convert::Infallible>(
                    Event::default().data(serde_json::to_string(&error_data).unwrap())
                );
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        // First chunk: role
        let role_chunk = ChatCompletionChunk {
            id: id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model_clone.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatChunkDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                    annotations: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        yield Ok::<_, std::convert::Infallible>(
            Event::default().data(serde_json::to_string(&role_chunk).unwrap())
        );

        // Content chunks
        let mut accumulated_content = String::new();
        let mut token_stream = std::pin::pin!(token_stream);
        while let Some(result) = token_stream.next().await {
            match result {
                Ok(token) => {
                    accumulated_content.push_str(&token);
                    let chunk = ChatCompletionChunk {
                        id: id_clone.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created,
                        model: model_clone.clone(),
                        choices: vec![ChatChunkChoice {
                            index: 0,
                            delta: ChatChunkDelta {
                                role: None,
                                content: Some(token),
                                annotations: None,
                            },
                            finish_reason: None,
                        }],
                        usage: None,
                    };
                    yield Ok(Event::default().data(serde_json::to_string(&chunk).unwrap()));
                }
                Err(e) => {
                    let error_data = serde_json::json!({
                        "error": { "message": e.to_string(), "type": "stream_error" }
                    });
                    yield Ok(Event::default().data(serde_json::to_string(&error_data).unwrap()));
                    return;
                }
            }
        }

        // Snapshot the pipeline metadata once — it drives both the native
        // citations and the plain-text footer. Take the lock before any await
        // so the MutexGuard never crosses an await point (it isn't Send).
        let footer_meta = metadata_cell.lock().unwrap().clone();

        // ── Native, clickable citations ──────────────────────────────────
        // Portable OpenAI-standard `delta.annotations[].url_citation` (a valid
        // chat.completion.chunk), rendered as clickable references by capable
        // clients or safely ignored. The plain-text footer below is the
        // universal fallback.
        let cite_cfg = &state.config.chat_pipeline;
        if cite_cfg.citation_annotations_enabled {
            let sources = build_citation_sources(
                &footer_meta,
                cite_cfg.source_footer_max.max(1),
                |doc_id, fallback| resolve_doc_title(&state, doc_id, fallback),
            );
            if !sources.is_empty() {
                let cite_base = cite_cfg.citation_base_url.trim_end_matches('/');
                let annotations: Vec<ChatAnnotation> = sources
                    .iter()
                    .map(|s| ChatAnnotation {
                        annotation_type: "url_citation".to_string(),
                        url_citation: UrlCitation {
                            url: citation_url(
                                &state,
                                cite_base,
                                &s.doc_id,
                                s.page,
                                s.section.as_deref(),
                            ),
                            title: s.title.clone(),
                        },
                    })
                    .collect();
                let ann_chunk = ChatCompletionChunk {
                    id: id_clone.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model_clone.clone(),
                    choices: vec![ChatChunkChoice {
                        index: 0,
                        delta: ChatChunkDelta {
                            role: None,
                            content: None,
                            annotations: Some(annotations),
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                };
                yield Ok(Event::default().data(serde_json::to_string(&ann_chunk).unwrap()));
            }
        }

        // Append the plain-text source footer for transparency / fallback, so
        // clients without native citation support still render it inline.
        if let Some(footer) = build_source_footer(
            &footer_meta,
            state.config.chat_pipeline.source_footer_enabled,
            state.config.chat_pipeline.source_footer_max,
            &id,
            |doc_id, fallback| resolve_doc_title(&state, doc_id, fallback),
        ) {
            accumulated_content.push_str(&footer);
            let footer_chunk = ChatCompletionChunk {
                id: id_clone.clone(),
                object: "chat.completion.chunk".to_string(),
                created,
                model: model_clone.clone(),
                choices: vec![ChatChunkChoice {
                    index: 0,
                    delta: ChatChunkDelta {
                        role: None,
                        content: Some(footer),
                        annotations: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            };
            yield Ok(Event::default().data(serde_json::to_string(&footer_chunk).unwrap()));
        }

        // Capture response length before content is moved
        let response_length = accumulated_content.len() as u32;

        // Save to session after stream completes
        if let Some(sid) = session_id
            && let Some(ref user_msg) = last_user_msg
        {
            let assistant_msg = ChatMessage {
                role: "assistant".to_string(),
                content: accumulated_content.clone(),
                images: vec![],
            };
            state.session_store.append(sid, user_msg.clone(), assistant_msg, user_id).await;

            // Feature 1: Async memory summarization
            if let Some(uid) = user_id {
                maybe_summarize_memory(
                    state.clone(), pipeline_for_memory, uid, sid, memories,
                );
            }
        }

        // Finish chunk
        let finish_chunk = ChatCompletionChunk {
            id: id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model_clone.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatChunkDelta {
                    role: None,
                    content: None,
                    annotations: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        };
        yield Ok(Event::default().data(serde_json::to_string(&finish_chunk).unwrap()));

        // Usage chunk
        let llm_usage = usage_cell.lock().unwrap().take().unwrap_or_default();
        state.metrics.record_tokens(llm_usage.prompt_tokens, llm_usage.completion_tokens);
        persist_usage(&state, llm_usage.prompt_tokens, llm_usage.completion_tokens);
        let usage_chunk = ChatCompletionChunk {
            id: id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model_clone.clone(),
            choices: vec![],
            usage: Some(ChatUsage {
                prompt_tokens: llm_usage.prompt_tokens,
                completion_tokens: llm_usage.completion_tokens,
                total_tokens: llm_usage.prompt_tokens + llm_usage.completion_tokens,
                thairag_response_id: None,
            }),
        };
        yield Ok(Event::default().data(serde_json::to_string(&usage_chunk).unwrap()));

        // Inference logging + analytics
        {
            let total_ms = request_start.elapsed().as_millis() as u64;
            let meta = metadata_cell.lock().unwrap().clone();
            let pp = state.providers();
            let (llm_kind, llm_model) = resolve_llm_info(&pp);
            let user_query_text: String = last_user_msg
                .as_ref()
                .map(|m| m.content.chars().take(2000).collect())
                .unwrap_or_default();
            let entry = InferenceLogEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                user_id: user_id.map(|u| u.0.to_string()),
                workspace_id: scope.workspace_ids.first().map(|w| w.0.to_string()),
                org_id: None,
                dept_id: None,
                session_id: session_id.map(|s| s.0.to_string()),
                response_id: id.clone(),
                query_text: user_query_text.clone(),
                detected_language: meta.language.clone(),
                intent: meta.intent.clone(),
                complexity: meta.complexity.clone(),
                llm_kind,
                llm_model,
                settings_scope: format!("{:?}", settings_scope),
                prompt_tokens: llm_usage.prompt_tokens,
                completion_tokens: llm_usage.completion_tokens,
                estimated_context_tokens: meta.estimated_context_tokens.unwrap_or(0),
                total_ms,
                search_ms: meta.search_ms,
                generation_ms: meta.generation_ms,
                chunks_retrieved: meta.chunks_retrieved,
                avg_chunk_score: meta.avg_chunk_score,
                self_rag_decision: meta.self_rag_decision.clone(),
                self_rag_confidence: meta.self_rag_confidence,
                quality_guard_pass: meta.quality_guard_pass,
                relevance_score: meta.relevance_score,
                hallucination_score: meta.hallucination_score,
                completeness_score: meta.completeness_score,
                pipeline_route: meta.pipeline_route.clone(),
                agents_used: serde_json::to_string(&stage_names)
                    .unwrap_or_else(|_| "[]".into()),
                status: "success".into(),
                error_message: None,
                response_length,
                feedback_score: None,
                input_guardrails_pass: meta.input_guardrails_pass,
                output_guardrails_pass: meta.output_guardrails_pass,
                guardrail_violation_codes: meta
                    .guardrail_violations
                    .iter()
                    .map(|v| v.code.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            };

            // ── Search Analytics ──
            if let Some(search_ms) = meta.search_ms {
                let result_count = meta.chunks_retrieved.unwrap_or(0);
                let event = SearchAnalyticsEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                    query_text: user_query_text.clone(),
                    user_id: user_id.map(|u| u.0.to_string()),
                    workspace_id: scope.workspace_ids.first().map(|w| w.0.to_string()),
                    result_count,
                    latency_ms: search_ms,
                    zero_results: result_count == 0,
                };
                let store = state.km_store.clone();
                tokio::spawn(async move {
                    store.insert_search_event(&event);
                });
            }

            // ── Document Lineage ──
            if !meta.retrieved_chunks.is_empty() {
                let lineage_response_id = id.clone();
                let lineage_query = user_query_text.clone();
                let chunk_metas = meta.retrieved_chunks.clone();
                let store = state.km_store.clone();
                tokio::spawn(async move {
                    let now = chrono::Utc::now().to_rfc3339();
                    for chunk in &chunk_metas {
                        let record = LineageRecord {
                            id: Uuid::new_v4().to_string(),
                            response_id: lineage_response_id.clone(),
                            timestamp: now.clone(),
                            query_text: lineage_query.clone(),
                            chunk_id: chunk.chunk_id.clone(),
                            doc_id: chunk.doc_id.clone(),
                            doc_title: chunk.doc_title.clone(),
                            chunk_text_preview: chunk.content_preview.clone(),
                            score: chunk.score,
                            rank: chunk.rank,
                            contributed: chunk.contributed,
                        };
                        store.insert_lineage_record(&record);
                    }
                });
            }

            let store = state.km_store.clone();
            tokio::spawn(async move {
                store.insert_inference_log(&entry);
            });
        }

        // [DONE] sentinel
        yield Ok(Event::default().data("[DONE]"));
    };

    let mut response = Sse::new(sse_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(5))
                .text("ping"),
        )
        .into_response();

    // Tell reverse proxies (nginx, Cloudflare, etc.) not to buffer SSE events
    response.headers_mut().insert(
        "X-Accel-Buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );

    Ok(response)
}

// ── First-party chat-UI streaming endpoint ───────────────────────────────

/// Request body for `POST /api/chat/conversations/{id}/messages`.
#[derive(serde::Deserialize)]
pub struct SendMessageRequest {
    /// Defaults to empty — a regenerate request carries no content.
    #[serde(default)]
    pub content: String,
    /// Files attached to this turn (base64). Decoded, converted, and guardrail-
    /// checked, then used as context for the answer and kept (session-scoped) for
    /// follow-up turns in the same conversation. Not added to the permanent KB.
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
    /// Regenerate the last answer: drop the trailing assistant turn and produce a
    /// fresh one for the same last user message. `content` is ignored.
    #[serde(default)]
    pub regenerate: bool,
    /// Edit-and-resend the last user message: drop the trailing assistant turn
    /// *and* the last user message, then answer `content` as the new last turn.
    #[serde(default)]
    pub edit: bool,
}

/// POST /api/chat/conversations/{id}/messages — first-party streaming chat.
///
/// Owner-checks the conversation, loads its stored history, runs the RAG
/// pipeline, streams a clean first-party SSE protocol, and persists the
/// completed turn back to the conversation. Independent of
/// `/v1/chat/completions` (which serves OWUI / OpenAI-compatible clients with
/// their bespoke chunk shapes) — this endpoint owns its own event protocol:
///
/// - `{"type":"progress","stage","status"}` — pipeline stage updates
/// - `{"type":"token","text"}`              — streamed answer tokens
/// - `{"type":"citation","citations":[…]}`  — resolved sources (once, post-answer)
/// - `{"type":"done","message_id","usage"}` — terminal success
/// - `{"type":"error","message"}`           — terminal failure
///
/// followed by a `[DONE]` sentinel. Inline source images are added in a
/// follow-up PR (a future `{"type":"image",…}` event).
/// Request body for image generation.
#[derive(serde::Deserialize)]
pub struct ImagePromptRequest {
    pub prompt: String,
}

/// POST /api/chat/conversations/{id}/images — generate an image from a prompt
/// and persist it as a turn (user prompt + assistant image). Capability-gated:
/// only works when `general_chat.image_generation` is enabled and points at an
/// OpenAI-compatible `/v1/images/generations` endpoint; otherwise returns a
/// clear error so the UI can keep the affordance hidden.
pub async fn generate_conversation_image(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(conversation_id): Path<String>,
    AppJson(req): AppJson<ImagePromptRequest>,
) -> Result<Json<crate::store::MessageRow>, ApiError> {
    let gc = crate::routes::settings::build_effective_general_chat(&state.config, &*state.km_store);
    let img = &gc.image_generation;
    if !img.enabled {
        return Err(ApiError(ThaiRagError::Validation(
            "Image generation is not enabled".into(),
        )));
    }
    if img.model.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "No image-generation model configured".into(),
        )));
    }
    let prompt = req.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "prompt must not be empty".into(),
        )));
    }

    // Require a real signed-in user + ownership of the conversation.
    let _uid = claims.sub.parse::<Uuid>().map_err(|_| {
        ApiError(ThaiRagError::Auth(
            "A signed-in user is required for chat conversations".into(),
        ))
    })?;
    let conv = state
        .km_store
        .get_conversation(&conversation_id)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Conversation not found".into())))?;
    if conv.user_id != claims.sub {
        return Err(ApiError(ThaiRagError::Authorization(
            "You do not have access to this conversation".into(),
        )));
    }

    // Resolve the image endpoint + key: image_generation overrides, else the
    // general_chat LLM, else the main LLM.
    let base = [
        img.base_url.as_str(),
        gc.llm.as_ref().map(|l| l.base_url.as_str()).unwrap_or(""),
        state.config.providers.llm.base_url.as_str(),
    ]
    .into_iter()
    .find(|s| !s.is_empty())
    .unwrap_or("")
    .trim_end_matches('/')
    .to_string();
    if base.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "No base URL configured for image generation".into(),
        )));
    }
    let api_key = gc
        .llm
        .as_ref()
        .map(|l| l.api_key.clone())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| state.config.providers.llm.api_key.clone());

    // OpenAI-compatible image generation; ask for base64 so the result is
    // self-contained (no dependency on a provider-hosted, expiring URL).
    let endpoint = format!("{base}/images/generations");
    let body = serde_json::json!({
        "model": img.model,
        "prompt": prompt,
        "n": 1,
        "response_format": "b64_json",
    });
    let client = reqwest::Client::new();
    let mut rb = client.post(&endpoint).json(&body);
    if !api_key.is_empty() {
        rb = rb.header("Authorization", format!("Bearer {api_key}"));
    }
    let resp = rb.send().await.map_err(|e| {
        ApiError(ThaiRagError::Internal(format!(
            "image generation request failed: {e}"
        )))
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        return Err(ApiError(ThaiRagError::Internal(format!(
            "image generation returned HTTP {status}: {}",
            detail.chars().take(200).collect::<String>()
        ))));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| {
        ApiError(ThaiRagError::Internal(format!(
            "failed to parse image response: {e}"
        )))
    })?;
    let b64 = json["data"][0]["b64_json"].as_str().ok_or_else(|| {
        ApiError(ThaiRagError::Internal(
            "image response had no b64_json data".into(),
        ))
    })?;
    let data_url = format!("data:image/png;base64,{b64}");

    // Persist as a turn. The image rides in the `images` field (not content), so
    // it is rendered by the UI but NOT replayed into the LLM context on later
    // turns (which a giant data URL in content would otherwise bloat).
    let image = crate::chat_history::PersistedImage {
        image_id: Uuid::new_v4().to_string(),
        url: data_url,
        page: None,
    };
    let row = crate::chat_history::persist_turn(
        &state.km_store,
        &conversation_id,
        &prompt,
        "",
        &[],
        std::slice::from_ref(&image),
        &crate::chat_history::PersistedTokenStats::default(),
    )?;
    Ok(Json(row))
}

pub async fn stream_conversation_message(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(conversation_id): Path<String>,
    AppJson(req): AppJson<SendMessageRequest>,
) -> Result<Response, ApiError> {
    // ── Validation ──────────────────────────────────────────────────
    let content = req.content.trim().to_string();
    if !req.regenerate && content.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "message content must not be empty".into(),
        )));
    }
    let max_msg_len = state.config.server.max_message_length;
    if content.len() > max_msg_len {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "message content too long: {} chars (max {max_msg_len})",
            content.len()
        ))));
    }

    // ── Require a real signed-in user (conversations are user-owned) ──
    let uid = claims.sub.parse::<Uuid>().map(UserId).map_err(|_| {
        ApiError(ThaiRagError::Auth(
            "A signed-in user is required for chat conversations".into(),
        ))
    })?;
    let user_id_str = claims.sub.clone();

    // ── Per-user concurrency + token-bucket rate limiting ──
    let _request_guard = state
        .user_request_limiter
        .try_acquire(&claims.sub)
        .map_err(|()| {
            ApiError(ThaiRagError::Validation(
                "Too many concurrent requests. Please wait for your previous request to complete."
                    .into(),
            ))
        })?;
    state
        .user_rate_limiter
        .try_acquire(&claims.sub)
        .map_err(|retry_after| {
            ApiError(ThaiRagError::Validation(format!(
                "User rate limit exceeded. Retry after {:.0} seconds.",
                retry_after.ceil()
            )))
        })?;

    // ── Owner-checked history load (404/403 without leaking existence) ──
    let history = crate::chat_history::load_history(
        &state.km_store,
        &conversation_id,
        &user_id_str,
        crate::chat_history::DEFAULT_HISTORY_LIMIT,
    )
    .map_err(|e| match e {
        crate::chat_history::ConversationAccess::NotFound => {
            ApiError(ThaiRagError::NotFound("Conversation not found".into()))
        }
        crate::chat_history::ConversationAccess::Forbidden => ApiError(
            ThaiRagError::Authorization("You do not have access to this conversation".into()),
        ),
    })?;

    // Rows that an edit/regenerate replaces. We do NOT delete them up front:
    // deletion is deferred until the new answer is ready to persist, so a stream
    // that drops mid-way (e.g. a gateway flake — mid-stream is not retried)
    // leaves the original turn intact instead of silently losing it on reload.
    let mut defer_delete_ids: Vec<String> = Vec::new();
    let full_messages = if req.edit {
        // Edit-and-resend: replace the trailing assistant turn *and* the old user
        // message, then answer `content` as a fresh last turn. Persisted like a
        // normal send (is_regenerate stays false) so the edited prompt and its new
        // answer both land in history, replacing the original pair.
        let mut rows = state.km_store.list_messages(&conversation_id);
        if rows.last().map(|m| m.role.as_str()) == Some("assistant") {
            defer_delete_ids.push(rows.pop().expect("checked non-empty").id);
        }
        if rows.last().map(|m| m.role.as_str()) != Some("user") {
            return Err(ApiError(ThaiRagError::Validation("nothing to edit".into())));
        }
        defer_delete_ids.push(rows.pop().expect("checked non-empty").id);
        if rows.len() > crate::chat_history::DEFAULT_HISTORY_LIMIT {
            rows = rows.split_off(rows.len() - crate::chat_history::DEFAULT_HISTORY_LIMIT);
        }
        let mut msgs = crate::chat_history::rows_to_chat_messages(&rows);
        msgs.push(ChatMessage {
            role: "user".to_string(),
            content: content.clone(),
            images: vec![],
        });
        msgs
    } else if req.regenerate {
        // Replace the last answer: drop the trailing assistant turn, then re-run
        // generation for the last user message already in history.
        let mut rows = state.km_store.list_messages(&conversation_id);
        if rows.last().map(|m| m.role.as_str()) == Some("assistant") {
            defer_delete_ids.push(rows.pop().expect("checked non-empty").id);
        }
        if rows.last().map(|m| m.role.as_str()) != Some("user") {
            return Err(ApiError(ThaiRagError::Validation(
                "nothing to regenerate".into(),
            )));
        }
        if rows.len() > crate::chat_history::DEFAULT_HISTORY_LIMIT {
            rows = rows.split_off(rows.len() - crate::chat_history::DEFAULT_HISTORY_LIMIT);
        }
        crate::chat_history::rows_to_chat_messages(&rows)
    } else {
        let mut h = history;
        h.push(ChatMessage {
            role: "user".to_string(),
            content: content.clone(),
            images: vec![],
        });
        h
    };

    // ── Scope + settings resolution (the user's workspace permissions) ──
    // A conversation can be pinned to a single workspace (scope selector); when
    // it is — and the user actually has access to that workspace — retrieval is
    // hard-filtered to it. This is the "one product per scope" lever that avoids
    // near-clone cross-contamination. An unknown/forbidden pin is ignored (falls
    // back to all the user's workspaces), so the pin can never widen access.
    let ws_ids = state.km_store.get_user_workspace_ids(uid);
    let conv_row = state.km_store.get_conversation(&conversation_id);
    // General (non-RAG) mode: a plain assistant that bypasses retrieval/scope
    // entirely. Honoured per conversation (set at creation).
    let is_general = conv_row.as_ref().map(|c| c.mode.as_str()) == Some("general");
    let pinned_ws = conv_row
        .and_then(|c| c.workspace_scope)
        .and_then(|s| s.parse::<Uuid>().ok())
        .map(thairag_core::types::WorkspaceId);
    let scope = match pinned_ws {
        Some(w) if ws_ids.contains(&w) => AccessScope::new(vec![w]),
        _ if ws_ids.is_empty() => AccessScope::none(),
        _ => AccessScope::new(ws_ids),
    };
    let settings_scope = scope
        .workspace_ids
        .first()
        .map(|ws_id| state.resolve_scope_for_workspace(*ws_id))
        .unwrap_or(crate::store::SettingsScope::Global);

    // ── Per-conversation attachments ────────────────────────────────
    // New attachments are decoded/converted/guardrail-checked and stashed in the
    // (ephemeral) session keyed by the conversation id, so follow-up turns reuse
    // them — mirroring the /v1 attachment behavior. They are context for the
    // answer, not added to the permanent KB.
    let attach_sid = SessionId(conversation_id.parse::<Uuid>().unwrap_or_default());
    let attachments: Vec<SessionAttachment> = match req.attachments.as_deref() {
        Some(raw) if !raw.is_empty() => {
            let processed = process_request_attachments(&state, raw)?;
            state
                .session_store
                .attach(&attach_sid, processed.clone())
                .await;
            processed
        }
        _ => state.session_store.get_attachments(&attach_sid).await,
    };

    // ── Memories / personal memory / searchable scopes (reuse /v1 setup) ──
    let memories = load_memories(&state, Some(uid));
    let personal_memories = retrieve_personal_memories(&state, Some(uid), &full_messages).await;
    let available_scopes = build_searchable_scopes(&state, &scope);
    let full_messages = inject_personal_memory_context(full_messages, &personal_memories);

    // ── Spawn the pipeline so progress can stream in real-time ──
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();
    let p = state.providers();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let metadata_cell_clone = metadata_cell.clone();
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);

    // General mode: build a plain LLM (the dedicated general_chat model, or the
    // main chat LLM if none) + system prompt. No retrieval/scope/citations.
    // Read the effective config so admin edits apply without a restart.
    let general_chat_cfg =
        crate::routes::settings::build_effective_general_chat(&state.config, &*state.km_store);
    let general_llm = if is_general {
        let cfg = general_chat_cfg
            .llm
            .clone()
            .unwrap_or_else(|| state.config.providers.llm.clone());
        Some(thairag_provider_llm::create_llm_provider(&cfg))
    } else {
        None
    };
    let general_system_prompt = general_chat_cfg.system_prompt.clone();

    let pipeline_handle = tokio::spawn(async move {
        if let Some(general_llm) = general_llm {
            // ── Non-RAG general chat: system prompt + history, straight to the LLM ──
            drop(progress_tx);
            let mut msgs = Vec::with_capacity(full_messages.len() + 1);
            msgs.push(ChatMessage {
                role: "system".to_string(),
                content: general_system_prompt,
                images: vec![],
            });
            msgs.extend(full_messages.clone());
            general_llm.generate_stream(&msgs, None).await
        } else if let Some(ref pipeline) = scoped_pipeline {
            if attachments.is_empty() {
                pipeline
                    .process_stream(
                        &full_messages,
                        &scope,
                        &memories,
                        &available_scopes,
                        Some(progress_tx),
                        Some(metadata_cell_clone),
                        // No client-injected <context>; real attachments go below.
                        false,
                    )
                    .await
            } else {
                pipeline
                    .process_stream_with_attachments(
                        &full_messages,
                        &attachments,
                        &memories,
                        &scope,
                        Some(progress_tx),
                        Some(metadata_cell_clone),
                    )
                    .await
            }
        } else {
            drop(progress_tx);
            p.orchestrator.process_stream(&full_messages, &scope).await
        }
    });

    // Read the EFFECTIVE (scope-merged) chat-pipeline config so citation/inline-
    // image settings are admin-toggleable at runtime per scope, not just at boot.
    // Read out here so the (move) stream closure doesn't hold `&state`.
    let eff_cp = crate::routes::settings::get_effective_chat_pipeline_scoped(
        &state.config,
        &*state.km_store,
        &settings_scope,
    );
    let cite_enabled = eff_cp.citation_annotations_enabled;
    let source_max = eff_cp.source_footer_max.max(1);
    let cite_base = eff_cp.citation_base_url.trim_end_matches('/').to_string();
    let inline_images_enabled = eff_cp.inline_images_enabled;
    let inline_images_max = eff_cp.inline_images_max;
    let is_regenerate = req.regenerate;
    let conv_id = conversation_id.clone();

    let sse_stream = async_stream::stream! {
        // 1) Drain progress events until the pipeline task resolves.
        let mut pipeline_handle = pipeline_handle;
        let pipeline_result;
        loop {
            tokio::select! {
                evt = progress_rx.recv() => {
                    if let Some(progress) = evt {
                        let data = serde_json::json!({
                            "type": "progress",
                            "stage": progress.stage,
                            "status": format!("{:?}", progress.status),
                        });
                        yield Ok::<_, std::convert::Infallible>(
                            Event::default().data(data.to_string())
                        );
                    }
                }
                result = &mut pipeline_handle => {
                    while let Ok(evt) = progress_rx.try_recv() {
                        let data = serde_json::json!({
                            "type": "progress",
                            "stage": evt.stage,
                            "status": format!("{:?}", evt.status),
                        });
                        yield Ok::<_, std::convert::Infallible>(
                            Event::default().data(data.to_string())
                        );
                    }
                    pipeline_result = match result {
                        Ok(r) => r,
                        Err(e) => Err(ThaiRagError::LlmProvider(
                            format!("Pipeline task panicked: {e}")
                        )),
                    };
                    break;
                }
            }
        }

        let LlmStreamResponse { stream: token_stream, usage: usage_cell } = match pipeline_result {
            Ok(resp) => resp,
            Err(e) => {
                let data = serde_json::json!({"type":"error","message": e.to_string()});
                yield Ok(Event::default().data(data.to_string()));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        // 2) Token events.
        let mut accumulated = String::new();
        let mut token_stream = std::pin::pin!(token_stream);
        let mut stream_failed = false;
        while let Some(result) = token_stream.next().await {
            match result {
                Ok(token) => {
                    accumulated.push_str(&token);
                    let data = serde_json::json!({"type":"token","text": token});
                    yield Ok(Event::default().data(data.to_string()));
                }
                Err(e) => {
                    let data = serde_json::json!({"type":"error","message": e.to_string()});
                    yield Ok(Event::default().data(data.to_string()));
                    stream_failed = true;
                    break;
                }
            }
        }
        if stream_failed {
            // A mid-stream failure leaves the turn unsaved (the user keeps their
            // text client-side); surface terminal sentinel and stop.
            yield Ok(Event::default().data("[DONE]"));
            return;
        }

        // 3) Citations — resolved once, in the clean first-party shape.
        let footer_meta = metadata_cell.lock().unwrap().clone();
        // A refusal cites nothing and must surface no sources: the no-context
        // guard records no citations, and without this the image step below would
        // fall back to ALL retrieved (irrelevant) chunks — showing source
        // thumbnails on a "no relevant information" answer. Detected via the
        // no-answer metadata state (guard refusal) or the answer text (model
        // refusal).
        let is_refusal_answer = (footer_meta.confidence.is_none()
            && footer_meta.confidence_summary.is_some())
            || thairag_agent::citation_parser::is_refusal(&accumulated);
        let mut persisted_citations: Vec<crate::chat_history::PersistedCitation> = Vec::new();
        if cite_enabled && !is_refusal_answer {
            let sources = build_citation_sources(
                &footer_meta,
                source_max,
                |doc_id, fallback| resolve_doc_title(&state, doc_id, fallback),
            );
            persisted_citations = sources
                .iter()
                .map(|s| crate::chat_history::PersistedCitation {
                    doc_id: s.doc_id.clone(),
                    title: s.title.clone(),
                    page: s.page,
                    section: s.section.clone(),
                    url: if cite_base.is_empty() {
                        None
                    } else {
                        Some(citation_url(&state, &cite_base, &s.doc_id, s.page, s.section.as_deref()))
                    },
                    snippet: s.snippet.clone(),
                })
                .collect();
            if !persisted_citations.is_empty() {
                let data = serde_json::json!({
                    "type": "citation",
                    "citations": persisted_citations,
                });
                yield Ok(Event::default().data(data.to_string()));
            }
        }

        // 3b) Inline source images — the source images of the chunks the answer
        // actually cited (falls back to all retrieved chunks only when there are
        // no structured citations), so images track the citations, not raw
        // retrieval. Gated (default off); needs a browser-reachable base + key.
        let image_chunks: Vec<thairag_core::types::RetrievedChunkMeta> =
            if is_refusal_answer {
                // Refusal → no sources at all (don't fall back to retrieval).
                Vec::new()
            } else if footer_meta.citations.is_empty() {
                footer_meta.retrieved_chunks.clone()
            } else {
                let cited: std::collections::HashSet<&str> = footer_meta
                    .citations
                    .iter()
                    .map(|c| c.chunk_id.as_str())
                    .collect();
                footer_meta
                    .retrieved_chunks
                    .iter()
                    .filter(|r| cited.contains(r.chunk_id.as_str()))
                    .cloned()
                    .collect()
            };
        let mut persisted_images: Vec<crate::chat_history::PersistedImage> = Vec::new();
        if inline_images_enabled
            && !cite_base.is_empty()
            && let Some(jwt) = state.jwt.as_deref()
        {
            for (img_id, page) in
                resolve_inline_images(&*state.km_store, &image_chunks, inline_images_max)
            {
                // Sign a single-image, time-limited token (reusing the citation
                // token model with the image id as subject) so a browser <img>
                // can load it from the public media route without auth headers.
                if let Ok(token) = jwt.encode_citation(&img_id.0.to_string(), 24) {
                    persisted_images.push(crate::chat_history::PersistedImage {
                        image_id: img_id.0.to_string(),
                        url: format!("{cite_base}/api/chat/media/{}?token={token}", img_id.0),
                        page,
                    });
                }
            }
            if !persisted_images.is_empty() {
                let data = serde_json::json!({"type": "image", "images": persisted_images});
                yield Ok(Event::default().data(data.to_string()));
            }
        }

        // 4) Token accounting (metrics + observability), mirroring /v1.
        let llm_usage = usage_cell.lock().unwrap().take().unwrap_or_default();
        state.metrics.record_tokens(llm_usage.prompt_tokens, llm_usage.completion_tokens);
        persist_usage(&state, llm_usage.prompt_tokens, llm_usage.completion_tokens);

        // 5) Persist the turn. Regenerate replaces only the assistant turn (the
        // user message already exists + the old answer was deleted above); a
        // normal turn persists both the user prompt and the assistant reply.
        let token_stats = crate::chat_history::PersistedTokenStats {
            prompt_tokens: llm_usage.prompt_tokens,
            completion_tokens: llm_usage.completion_tokens,
        };
        // Deferred swap: now that the answer is ready, delete the rows the
        // edit/regenerate replaces, then persist the new turn. Done here (not up
        // front) so a failed/aborted stream never loses the original.
        for old_id in &defer_delete_ids {
            if let Err(e) = state.km_store.delete_message(old_id) {
                tracing::warn!(error = %e, old_id, "failed to delete replaced message");
            }
        }
        let persist_result = if is_regenerate {
            crate::chat_history::persist_assistant(
                &state.km_store,
                &conv_id,
                &accumulated,
                &persisted_citations,
                &persisted_images,
                &token_stats,
            )
        } else {
            crate::chat_history::persist_turn(
                &state.km_store,
                &conv_id,
                &content,
                &accumulated,
                &persisted_citations,
                &persisted_images,
                &token_stats,
            )
        };
        let message_id = match persist_result {
            Ok(row) => row.id,
            Err(e) => {
                let data = serde_json::json!({
                    "type": "error",
                    "message": format!("failed to save message: {e}"),
                });
                yield Ok(Event::default().data(data.to_string()));
                String::new()
            }
        };

        // 6) Terminal success.
        let done = serde_json::json!({
            "type": "done",
            "message_id": message_id,
            "usage": {
                "prompt_tokens": llm_usage.prompt_tokens,
                "completion_tokens": llm_usage.completion_tokens,
            },
            "confidence": footer_meta.confidence,
            "confidence_summary": footer_meta.confidence_summary,
            "confidence_factors": footer_meta.confidence_factors,
        });
        yield Ok(Event::default().data(done.to_string()));
        yield Ok(Event::default().data("[DONE]"));
    };

    let mut response = Sse::new(sse_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(5))
                .text("ping"),
        )
        .into_response();
    response.headers_mut().insert(
        "X-Accel-Buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    Ok(response)
}

/// Resolve the source images to surface inline, deduped by image and capped.
///
/// A vision-derived chunk uses its own `image_blob_id` directly. A text-layer
/// chunk has no image of its own, so it's linked to its **page-render** image by
/// page number — that's what lets text-layer PDFs still show the source page.
/// Per-doc page→image maps are built lazily so we hit the store at most once per
/// cited document.
/// The 1-indexed page a chunk came from. Prefers the structured `page_numbers`,
/// but falls back to the enricher's `section_title` "Page N" convention — which
/// is how the AI chunker actually records pages in practice (it rarely populates
/// `page_numbers`). Without this fallback, page→image linkage never fires.
fn chunk_page(c: &thairag_core::types::RetrievedChunkMeta) -> Option<usize> {
    if let Some(p) = c.page_numbers.as_ref().and_then(|p| p.first().copied()) {
        return Some(p);
    }
    let s = c.section_title.as_deref()?.trim();
    let rest = s
        .strip_prefix("Page ")
        .or_else(|| s.strip_prefix("page "))
        .or_else(|| s.strip_prefix("หน้า "))?;
    rest.trim().parse::<usize>().ok()
}

fn resolve_inline_images(
    store: &dyn crate::store::KmStoreTrait,
    chunks: &[thairag_core::types::RetrievedChunkMeta],
    max: usize,
) -> Vec<(thairag_core::types::ImageId, Option<usize>)> {
    use std::collections::{HashMap, HashSet};
    let mut seen: HashSet<thairag_core::types::ImageId> = HashSet::new();
    let mut page_maps: HashMap<String, HashMap<u32, thairag_core::types::ImageId>> = HashMap::new();
    let mut out = Vec::new();

    for c in chunks {
        let first_page = chunk_page(c);
        let img = if let Some(id) = c.image_blob_id {
            Some(id)
        } else if let Some(page) = first_page {
            let map = page_maps.entry(c.doc_id.clone()).or_insert_with(|| {
                let mut m = HashMap::new();
                if let Ok(uuid) = c.doc_id.parse()
                    && let Ok(blobs) =
                        store.list_image_blobs_for_doc(thairag_core::types::DocId(uuid))
                {
                    for b in blobs {
                        if b.source == crate::store::ImageSource::PdfPageRender
                            && let Some(pn) = b.page_num
                        {
                            m.entry(pn).or_insert(b.image_id);
                        }
                    }
                }
                m
            });
            map.get(&(page as u32)).copied()
        } else {
            None
        };

        if let Some(id) = img
            && seen.insert(id)
        {
            out.push((id, first_page));
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Query params for the token-gated media route.
#[derive(serde::Deserialize)]
pub struct MediaQuery {
    pub token: String,
}

/// GET /api/chat/media/{image_id}?token=… — serve a source image by id.
///
/// Public (no auth header): the signed, time-limited `token` authorizes a
/// single image so a browser `<img>` in a chat answer can load it directly.
/// Mirrors the citation viewer's token model (`JwtService::encode_citation`),
/// using the image id as the token subject.
pub async fn serve_media(
    State(state): State<AppState>,
    Path(image_id): Path<String>,
    Query(params): Query<MediaQuery>,
) -> Result<Response, ApiError> {
    let jwt = state
        .jwt
        .as_deref()
        .ok_or_else(|| ApiError(ThaiRagError::Auth("media tokens are not configured".into())))?;
    let authorized = jwt
        .decode_citation(&params.token)
        .map_err(|_| ApiError(ThaiRagError::Auth("invalid or expired media token".into())))?;
    if authorized != image_id {
        return Err(ApiError(ThaiRagError::Authorization(
            "token does not authorize this image".into(),
        )));
    }
    let img_uuid = image_id
        .parse::<Uuid>()
        .map_err(|_| ApiError(ThaiRagError::Validation("invalid image id".into())))?;
    let record = state
        .km_store
        .get_image_blob(thairag_core::types::ImageId(img_uuid))
        .ok()
        .flatten()
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("image not found".into())))?;

    let mut response = Response::new(axum::body::Body::from(record.bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_str(&record.mime)
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(response)
}

/// Extract the LLM kind/model from the provider config.
fn resolve_llm_info(p: &crate::app_state::ProviderBundle) -> (String, String) {
    if let Some(ref rg) = p.chat_pipeline_config.response_generator_llm {
        (format!("{:?}", rg.kind).to_lowercase(), rg.model.clone())
    } else if let Some(ref shared) = p.chat_pipeline_config.llm {
        (
            format!("{:?}", shared.kind).to_lowercase(),
            shared.model.clone(),
        )
    } else {
        (
            format!("{:?}", p.providers_config.llm.kind).to_lowercase(),
            p.providers_config.llm.model.clone(),
        )
    }
}

// ── Session Summary Endpoints ────────────────────────────────────────

/// GET /api/chat/sessions/:session_id/summary
/// Returns the current conversation summary for a session.
pub async fn get_session_summary(
    State(state): State<AppState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    let uuid = session_id.parse::<Uuid>().map_err(|_| {
        ApiError(ThaiRagError::Validation(format!(
            "invalid session_id: {session_id}"
        )))
    })?;
    let sid = SessionId(uuid);

    let msg_count = state.session_store.message_count(&sid).await;
    if msg_count == 0 {
        return Err(ApiError(ThaiRagError::Validation(
            "session not found".into(),
        )));
    }

    let (summary, summary_message_count) = state
        .session_store
        .get_summary(&sid)
        .await
        .unwrap_or_else(|| (String::new(), 0));

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "summary": summary,
        "summary_message_count": summary_message_count,
        "current_message_count": msg_count,
    }))
    .into_response())
}

/// POST /api/chat/sessions/:session_id/summarize
/// Manually trigger summarization of a session's conversation history.
pub async fn summarize_session(
    State(state): State<AppState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    let uuid = session_id.parse::<Uuid>().map_err(|_| {
        ApiError(ThaiRagError::Validation(format!(
            "invalid session_id: {session_id}"
        )))
    })?;
    let sid = SessionId(uuid);

    let messages = state
        .session_store
        .get_history(&sid)
        .await
        .ok_or_else(|| ApiError(ThaiRagError::Validation("session not found".into())))?;

    if messages.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "session has no messages".into(),
        )));
    }

    // Build LLM provider for summarization
    let p = state.providers();
    let chat_config = &p.chat_pipeline_config;
    let llm: Arc<dyn thairag_core::traits::LlmProvider> =
        if let Some(ref cfg) = chat_config.memory_llm {
            Arc::from(thairag_provider_llm::create_llm_provider(cfg))
        } else if let Some(ref cfg) = chat_config.llm {
            Arc::from(thairag_provider_llm::create_llm_provider(cfg))
        } else {
            Arc::from(thairag_provider_llm::create_llm_provider(
                &p.providers_config.llm,
            ))
        };

    let keep_recent = chat_config.summarize_keep_recent;
    let compact_end = messages.len().saturating_sub(keep_recent);

    // If there are very few messages, summarize all of them without compacting
    let (summary, did_compact) = if compact_end <= 1 {
        let summary = context_compactor::summarize_conversation(llm.as_ref(), &messages)
            .await
            .map_err(|e| ApiError(ThaiRagError::LlmProvider(e.to_string())))?;
        (summary, false)
    } else {
        let to_summarize = &messages[..compact_end];
        let summary = context_compactor::summarize_conversation(llm.as_ref(), to_summarize)
            .await
            .map_err(|e| ApiError(ThaiRagError::LlmProvider(e.to_string())))?;

        if !summary.is_empty() {
            // Compact the session: replace old messages with summary + keep recent
            let recent = &messages[compact_end..];
            let compacted = ContextCompactor::build_compacted_messages(&summary, recent);
            state.session_store.replace_messages(&sid, compacted).await;
        }
        (summary, true)
    };

    // Store the summary
    state
        .session_store
        .set_summary(&sid, summary.clone(), messages.len())
        .await;

    let new_msg_count = state.session_store.message_count(&sid).await;

    tracing::info!(
        session_id = %sid,
        original_messages = messages.len(),
        new_messages = new_msg_count,
        compacted = did_compact,
        "Manual session summarization complete"
    );

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "summary": summary,
        "messages_before": messages.len(),
        "messages_after": new_msg_count,
        "compacted": did_compact,
    }))
    .into_response())
}

#[cfg(test)]
mod citation_tests {
    use super::*;
    use thairag_core::types::{Citation, RetrievedChunkMeta};

    #[test]
    fn citation_html_renders_tables_but_strips_scripts() {
        let content = "Intro prose.\n\
            <table><tr><td colspan=\"2\">หัวข้อ</td></tr>\
            <tr><td>ก</td><td>๑๒๓</td></tr></table>\n\
            <script>alert('xss')</script> tail";
        let out = render_citation_html(content);
        // Reconstructed table survives with its span + Thai numerals intact.
        assert!(out.contains("<table>"), "table dropped: {out}");
        assert!(out.contains("colspan=\"2\""), "span dropped: {out}");
        assert!(out.contains("๑๒๓"));
        // Any HTML from the document's own text is stripped (no script tag).
        assert!(!out.contains("<script"), "script survived: {out}");
        // Prose text is preserved.
        assert!(out.contains("Intro prose."));
    }

    fn chunk(id: &str, preview: &str, score: f32, rank: u32) -> RetrievedChunkMeta {
        RetrievedChunkMeta {
            chunk_id: id.to_string(),
            doc_id: format!("doc-{id}"),
            doc_title: None,
            content_preview: preview.to_string(),
            score,
            rank,
            contributed: true,
            ..Default::default()
        }
    }

    fn chunk_loc(
        id: &str,
        preview: &str,
        rank: u32,
        page: Option<usize>,
        section: Option<&str>,
    ) -> RetrievedChunkMeta {
        RetrievedChunkMeta {
            page_numbers: page.map(|p| vec![p]),
            section_title: section.map(str::to_string),
            ..chunk(id, preview, 0.9, rank)
        }
    }

    #[test]
    fn resolve_inline_images_direct_and_page_linked() {
        use crate::store::{ImageBlobRecord, ImageSource, KmStoreTrait, memory::MemoryKmStore};
        use thairag_core::types::{DocId, ImageId, WorkspaceId};

        let store = MemoryKmStore::new();
        let img_direct = ImageId::new();
        let img_page = ImageId::new();
        let doc = DocId(Uuid::new_v4());
        let doc_str = doc.0.to_string();

        // A page-render image for page 6 of `doc`.
        store
            .save_image_blob(ImageBlobRecord {
                image_id: img_page,
                doc_id: doc,
                workspace_id: WorkspaceId(Uuid::new_v4()),
                bytes: vec![0u8; 4],
                mime: "image/png".into(),
                width: None,
                height: None,
                page_num: Some(6),
                source: ImageSource::PdfPageRender,
            })
            .unwrap();

        // A page-render image for page 3 — linked via the "Page N" section_title.
        let img_page3 = ImageId::new();
        store
            .save_image_blob(ImageBlobRecord {
                image_id: img_page3,
                doc_id: doc,
                workspace_id: WorkspaceId(Uuid::new_v4()),
                bytes: vec![0u8; 4],
                mime: "image/png".into(),
                width: None,
                height: None,
                page_num: Some(3),
                source: ImageSource::PdfPageRender,
            })
            .unwrap();

        let direct = RetrievedChunkMeta {
            image_blob_id: Some(img_direct),
            page_numbers: Some(vec![2]),
            ..Default::default()
        };
        // Text-layer chunk: no image of its own, but page 6 links to the render.
        let text_layer = RetrievedChunkMeta {
            doc_id: doc_str.clone(),
            image_blob_id: None,
            page_numbers: Some(vec![6]),
            ..Default::default()
        };
        // Vision chunk recording its page in section_title ("Page 3") rather than
        // page_numbers — the real-world case. Must still link to the render.
        let section_chunk = RetrievedChunkMeta {
            doc_id: doc_str,
            image_blob_id: None,
            page_numbers: None,
            section_title: Some("Page 3".to_string()),
            ..Default::default()
        };
        let no_image = RetrievedChunkMeta {
            image_blob_id: None,
            page_numbers: None,
            ..Default::default()
        };

        // Direct image used as-is; text-layer chunk linked by page_numbers;
        // section_chunk linked by "Page 3"; the page-less chunk is skipped.
        assert_eq!(
            resolve_inline_images(
                &store,
                &[direct.clone(), text_layer.clone(), section_chunk, no_image],
                10
            ),
            vec![
                (img_direct, Some(2)),
                (img_page, Some(6)),
                (img_page3, Some(3))
            ]
        );
        // Cap respected.
        assert_eq!(
            resolve_inline_images(&store, &[direct, text_layer], 1),
            vec![(img_direct, Some(2))]
        );
        // Empty input → empty.
        assert!(resolve_inline_images(&store, &[], 10).is_empty());
    }

    fn cite(marker: u32, chunk_id: &str, title: &str, score: f32) -> Citation {
        Citation {
            claim: "claim".to_string(),
            marker,
            chunk_id: chunk_id.to_string(),
            doc_id: format!("doc-{chunk_id}"),
            doc_title: Some(title.to_string()),
            score,
        }
    }

    // Title resolver that just echoes the chunk fallback (or the doc id).
    fn echo_title(doc_id: &str, fallback: Option<&str>) -> String {
        fallback
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .unwrap_or_else(|| doc_id.to_string())
    }

    #[test]
    fn markers_drive_order_regardless_of_insertion_order() {
        let meta = PipelineMetadata {
            retrieved_chunks: vec![
                chunk("a", "preview A", 0.9, 0),
                chunk("b", "preview B", 0.8, 1),
            ],
            citations: vec![
                // Deliberately out of order: marker 2 first, marker 1 second.
                cite(2, "b", "Title B", 0.8),
                cite(1, "a", "Title A", 0.9),
            ],
            ..Default::default()
        };

        let sources = build_citation_sources(&meta, 10, echo_title);

        assert_eq!(sources.len(), 2);
        // Index 0 is marker [1] regardless of citation insertion order.
        assert_eq!(sources[0].title, "Title A");
        assert_eq!(sources[0].doc_id, "doc-a");
        assert_eq!(sources[1].title, "Title B");
        assert_eq!(sources[1].doc_id, "doc-b");
    }

    #[test]
    fn marker_gap_is_filled_positionally_from_ranked_chunks() {
        let meta = PipelineMetadata {
            retrieved_chunks: vec![
                chunk("a", "preview A", 0.9, 0),
                chunk("b", "preview B", 0.8, 1),
            ],
            // Answer cited only [2]; slot [1] must still be present and aligned.
            citations: vec![cite(2, "b", "Title B", 0.8)],
            ..Default::default()
        };

        let sources = build_citation_sources(&meta, 10, echo_title);

        assert_eq!(sources.len(), 2);
        // Slot [1] filled from the rank-0 retrieved chunk.
        assert_eq!(sources[0].doc_id, "doc-a");
        assert_eq!(sources[1].title, "Title B");
    }

    #[test]
    fn no_markers_falls_back_to_score_ranked_contributed_chunks() {
        let meta = PipelineMetadata {
            retrieved_chunks: vec![
                chunk("a", "preview A", 0.5, 0),
                chunk("b", "preview B", 0.9, 1),
            ],
            citations: vec![],
            ..Default::default()
        };

        let sources = build_citation_sources(&meta, 10, echo_title);

        assert_eq!(sources.len(), 2);
        // Sorted by score descending: B (0.9) before A (0.5).
        assert_eq!(sources[0].doc_id, "doc-b");
        assert_eq!(sources[1].doc_id, "doc-a");
    }

    #[test]
    fn no_markers_respects_max_truncation() {
        let meta = PipelineMetadata {
            retrieved_chunks: vec![
                chunk("a", "preview A", 0.5, 0),
                chunk("b", "preview B", 0.9, 1),
                chunk("c", "preview C", 0.7, 2),
            ],
            citations: vec![],
            ..Default::default()
        };

        let sources = build_citation_sources(&meta, 2, echo_title);

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].doc_id, "doc-b");
        assert_eq!(sources[1].doc_id, "doc-c");
    }

    #[test]
    fn citation_source_carries_page_and_section_from_marker_chunk() {
        let meta = PipelineMetadata {
            retrieved_chunks: vec![chunk_loc("a", "snippet", 0, Some(7), Some("Prohibited"))],
            citations: vec![cite(1, "a", "Title A", 0.9)],
            ..Default::default()
        };
        let sources = build_citation_sources(&meta, 10, echo_title);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].page, Some(7));
        assert_eq!(sources[0].section.as_deref(), Some("Prohibited"));
    }

    #[test]
    fn citation_source_provenance_in_no_marker_fallback() {
        let meta = PipelineMetadata {
            retrieved_chunks: vec![chunk_loc("a", "snippet", 0, Some(3), Some("Intro"))],
            citations: vec![],
            ..Default::default()
        };
        let sources = build_citation_sources(&meta, 10, echo_title);
        assert_eq!(sources[0].page, Some(3));
        assert_eq!(sources[0].section.as_deref(), Some("Intro"));
    }

    #[test]
    fn provenance_query_encodes_page_and_section() {
        assert_eq!(provenance_query(None, None), "");
        assert_eq!(provenance_query(Some(5), None), "&page=5");
        // Section is percent-encoded (the space becomes %20).
        let q = provenance_query(Some(2), Some("Risk A"));
        assert_eq!(q, "&page=2&section=Risk%20A");
        // Blank section is dropped.
        assert_eq!(provenance_query(None, Some("  ")), "");
    }
}
