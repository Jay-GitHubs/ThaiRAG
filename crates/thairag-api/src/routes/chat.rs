use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_agent::context_compactor::{self, ContextCompactor};
use thairag_agent::conversation_memory::MemoryEntry;
use thairag_agent::personal_memory::PersonalMemoryManager;
use thairag_agent::tool_router::SearchableScope;
use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatChoice, ChatChunkChoice, ChatChunkDelta, ChatCompletionChunk, ChatCompletionRequest,
    ChatCompletionResponse, ChatMessage, ChatUsage, LlmStreamResponse, MetadataCell,
    PersonalMemory, PipelineMetadata, SessionId, UserId,
};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::feedback;
use crate::store::{InferenceLogEntry, LineageRecord, SearchAnalyticsEvent};

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    headers: axum::http::HeaderMap,
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

    // ── Scope resolution ────────────────────────────────────────────
    // For API key auth: check X-OpenWebUI-User-Email header to resolve real user.
    // This allows Open WebUI (with ENABLE_FORWARD_USER_INFO_HEADERS=true) to
    // enforce per-user workspace permissions even through a shared API key.
    let user_id = if claims.sub == "api-key" {
        // Resolve real user from forwarded headers (e.g., Open WebUI with
        // ENABLE_FORWARD_USER_INFO_HEADERS=true). If the user doesn't exist
        // in ThaiRAG yet, auto-create them as a viewer.
        headers
            .get("x-openwebui-user-email")
            .and_then(|v| v.to_str().ok())
            .and_then(|email| {
                match state.km_store.get_user_by_email(email) {
                    Ok(u) => Some(u.user.id),
                    Err(_) => {
                        // Auto-provision: create user from forwarded identity
                        let name = headers
                            .get("x-openwebui-user-name")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or(email);
                        state
                            .km_store
                            .upsert_user_by_email(
                                email.to_string(),
                                name.to_string(),
                                String::new(),
                                false,
                                "viewer".to_string(),
                            )
                            .ok()
                            .map(|u| u.id)
                    }
                }
            })
    } else if claims.sub == "anonymous" {
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
        )
        .await
    }
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
        let title = c.doc_title.as_deref().unwrap_or(&c.doc_id);
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
    let mut llm_resp = if let Some(ref pipeline) = scoped_pipeline {
        pipeline
            .process(
                &augmented_messages,
                &scope,
                &memories,
                &available_scopes,
                Some(progress_tx),
                Some(metadata_cell.clone()),
            )
            .await
            .map_err(ApiError::from)?
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

    let pipeline_handle = tokio::spawn(async move {
        if let Some(ref pipeline) = scoped_pipeline {
            pipeline
                .process_stream(
                    &augmented_messages_clone,
                    &scope_clone,
                    &memories_clone,
                    &available_scopes_clone,
                    Some(progress_tx),
                    Some(metadata_cell_clone),
                )
                .await
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

        // Append source footer for end-user transparency (e.g. Open WebUI).
        // Emitted as a final content chunk so the client renders it inline.
        // Snapshot the metadata before any further await so the MutexGuard
        // never crosses an await point (it isn't Send).
        let footer_meta = metadata_cell.lock().unwrap().clone();
        if let Some(footer) = build_source_footer(
            &footer_meta,
            state.config.chat_pipeline.source_footer_enabled,
            state.config.chat_pipeline.source_footer_max,
            &id,
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
