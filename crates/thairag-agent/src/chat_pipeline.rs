use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use thairag_config::schema::ChatPipelineConfig;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::permission::AccessScope;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{
    ChatMessage, DocId, LlmResponse, LlmStreamResponse, LlmUsage, MetadataCell, PipelineMetadata,
    PipelineProgress, ProgressSender, QueryIntent, SearchQuery, StageStatus, VisionMessage,
};
use thairag_search::HybridSearchEngine;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use crate::active_learning::ActiveLearning;
use crate::colbert_reranker::ColbertReranker;
use crate::context_curator::{self, ContextCurator, CuratedContext};
use crate::contextual_compression::ContextualCompression;
use crate::conversation_memory::{ConversationMemory, MemoryEntry};
use crate::corrective_rag::{ContextAction, CorrectiveRag};
use crate::graph_rag::{GraphRag, KnowledgeGraph};
use crate::guardrails::{GuardAction, InputGuardrails, OutputGuardrails, violations_to_meta};
use crate::language_adapter::LanguageAdapter;
use crate::live_retrieval::LiveRetrieval;
use crate::map_reduce::MapReduceRag;
use crate::multimodal_rag::MultimodalRag;
use crate::pipeline_orchestrator::{PipelineOrchestrator, PipelineRoute, heuristic_decide};
use crate::quality_guard::QualityGuard;
use crate::query_analyzer::{self, QueryAnalysis, QueryAnalyzer, QueryLanguage};
use crate::query_rewriter::{self, QueryRewriter, RewrittenQueries};
use crate::ragas_eval::RagasEvaluator;
use crate::raptor::Raptor;
use crate::response_generator::ResponseGenerator;
use crate::self_rag::{RetrievalDecision, SelfRag};
use crate::speculative_rag::SpeculativeRag;
use crate::tool_router::{SearchableScope, ToolRouter};

/// Closure that resolves MCP connector configs for a given access scope.
type ConnectorProvider =
    Arc<dyn Fn(&AccessScope) -> Vec<thairag_core::types::McpConnectorConfig> + Send + Sync>;

/// Per-request LLM call budget. Shared across pipeline stages to enforce
/// `max_llm_calls_per_request` and skip optional agents when budget runs low.
#[derive(Clone)]
struct LlmBudget(Arc<AtomicU32>);

impl LlmBudget {
    fn new(max_calls: u32) -> Self {
        Self(Arc::new(AtomicU32::new(max_calls)))
    }

    /// Try to spend 1 LLM call. Returns true if budget was available.
    fn try_spend(&self) -> bool {
        loop {
            let current = self.0.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .0
                .compare_exchange(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Check remaining budget without spending.
    fn remaining(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Returns true if any message in the slice contains image attachments.
fn has_vision_content(messages: &[ChatMessage]) -> bool {
    messages.iter().any(|m| !m.images.is_empty())
}

/// Converts ChatMessages to VisionMessages for the vision LLM API.
fn to_vision_messages(messages: &[ChatMessage]) -> Vec<VisionMessage> {
    messages
        .iter()
        .map(|m| VisionMessage {
            role: m.role.clone(),
            text: m.content.clone(),
            images: m.images.clone(),
        })
        .collect()
}

/// The full multi-agent chat pipeline.
pub struct ChatPipeline {
    // Agents (None = disabled, use fallback)
    query_analyzer: Option<QueryAnalyzer>,
    query_rewriter: Option<QueryRewriter>,
    context_curator: Option<ContextCurator>,
    response_generator: ResponseGenerator,
    quality_guard: Option<Arc<QualityGuard>>,
    language_adapter: Option<LanguageAdapter>,
    pipeline_orchestrator: Option<PipelineOrchestrator>,
    conversation_memory: Option<ConversationMemory>,
    tool_router: Option<ToolRouter>,
    // Next-gen features
    self_rag: Option<SelfRag>,
    graph_rag: Option<GraphRag>,
    corrective_rag: Option<CorrectiveRag>,
    speculative_rag: Option<SpeculativeRag>,
    map_reduce: Option<MapReduceRag>,
    ragas_evaluator: Option<Arc<RagasEvaluator>>,
    // Final 5 features
    contextual_compression: Option<ContextualCompression>,
    multimodal_rag: Option<MultimodalRag>,
    raptor: Option<Raptor>,
    colbert_reranker: Option<ColbertReranker>,
    active_learning: Option<Arc<ActiveLearning>>,
    /// Live source retrieval from MCP connectors.
    live_retrieval: Option<LiveRetrieval>,
    /// Provider of MCP connector configs for a given access scope.
    connector_provider: Option<ConnectorProvider>,
    /// Input-side guardrails (PII / secrets / injection / blocklist on the user query).
    input_guardrails: Option<Arc<InputGuardrails>>,
    /// Output-side guardrails (PII / secrets / blocklist on the model response).
    output_guardrails: Option<Arc<OutputGuardrails>>,
    /// In-memory knowledge graph built from document entity extraction.
    knowledge_graph: Arc<std::sync::RwLock<KnowledgeGraph>>,
    // Infrastructure
    main_llm: Arc<dyn LlmProvider>,
    search_engine: Arc<HybridSearchEngine>,
    config: ChatPipelineConfig,
    /// Adaptive quality threshold (encoded as u32 bits of f32). Updated externally.
    adaptive_threshold: Arc<AtomicU32>,
    /// Prompt registry for externalized agent system prompts.
    prompts: Arc<PromptRegistry>,
    /// Optional resolver: DocId → document title (for richer LLM context).
    doc_title_resolver: Option<Arc<dyn Fn(DocId) -> Option<String> + Send + Sync>>,
}

impl ChatPipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        main_llm: Arc<dyn LlmProvider>,
        search_engine: Arc<HybridSearchEngine>,
        query_analyzer: Option<QueryAnalyzer>,
        query_rewriter: Option<QueryRewriter>,
        context_curator: Option<ContextCurator>,
        response_generator: ResponseGenerator,
        quality_guard: Option<Arc<QualityGuard>>,
        language_adapter: Option<LanguageAdapter>,
        pipeline_orchestrator: Option<PipelineOrchestrator>,
        conversation_memory: Option<ConversationMemory>,
        tool_router: Option<ToolRouter>,
        self_rag: Option<SelfRag>,
        graph_rag: Option<GraphRag>,
        corrective_rag: Option<CorrectiveRag>,
        speculative_rag: Option<SpeculativeRag>,
        map_reduce: Option<MapReduceRag>,
        ragas_evaluator: Option<Arc<RagasEvaluator>>,
        contextual_compression: Option<ContextualCompression>,
        multimodal_rag: Option<MultimodalRag>,
        raptor: Option<Raptor>,
        colbert_reranker: Option<ColbertReranker>,
        active_learning: Option<Arc<ActiveLearning>>,
        live_retrieval: Option<LiveRetrieval>,
        connector_provider: Option<ConnectorProvider>,
        input_guardrails: Option<Arc<InputGuardrails>>,
        output_guardrails: Option<Arc<OutputGuardrails>>,
        config: ChatPipelineConfig,
        prompts: Arc<PromptRegistry>,
        doc_title_resolver: Option<Arc<dyn Fn(DocId) -> Option<String> + Send + Sync>>,
    ) -> Self {
        let threshold_bits = config.quality_guard_threshold.to_bits();
        Self {
            query_analyzer,
            query_rewriter,
            context_curator,
            response_generator,
            quality_guard,
            language_adapter,
            pipeline_orchestrator,
            conversation_memory,
            tool_router,
            self_rag,
            graph_rag,
            corrective_rag,
            speculative_rag,
            map_reduce,
            ragas_evaluator,
            contextual_compression,
            multimodal_rag,
            raptor,
            colbert_reranker,
            active_learning,
            live_retrieval,
            connector_provider,
            input_guardrails,
            output_guardrails,
            knowledge_graph: Arc::new(std::sync::RwLock::new(KnowledgeGraph::default())),
            main_llm,
            search_engine,
            config,
            adaptive_threshold: Arc::new(AtomicU32::new(threshold_bits)),
            prompts,
            doc_title_resolver,
        }
    }

    /// Get the shared adaptive threshold handle (for external updates from feedback system).
    pub fn adaptive_threshold_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.adaptive_threshold)
    }

    /// Emit a pipeline progress event (no-op if sender is None).
    /// Model info is automatically included on `Started` events for LLM-using stages.
    fn emit_progress(
        &self,
        tx: &Option<ProgressSender>,
        stage: &str,
        status: StageStatus,
        duration_ms: Option<u64>,
    ) {
        if let Some(tx) = tx {
            let model = if status == StageStatus::Started {
                self.model_for_stage(stage)
            } else {
                None
            };
            let _ = tx.send(PipelineProgress {
                stage: stage.to_string(),
                status,
                duration_ms,
                model,
            });
        }
    }

    /// Resolve the LLM model name for a given pipeline stage.
    fn model_for_stage(&self, stage: &str) -> Option<String> {
        let cfg = &self.config;
        let per_agent = match stage {
            "query_analyzer" => cfg.query_analyzer_llm.as_ref(),
            "query_rewriter" => cfg.query_rewriter_llm.as_ref(),
            "context_curator" => cfg.context_curator_llm.as_ref(),
            "response_generator" => cfg.response_generator_llm.as_ref(),
            "quality_guard" => cfg.quality_guard_llm.as_ref(),
            "language_adapter" => cfg.language_adapter_llm.as_ref(),
            "pipeline_orchestrator" => cfg.orchestrator_llm.as_ref(),
            "live_retrieval" => cfg.live_retrieval_llm.as_ref(),
            "speculative_rag" => cfg.speculative_rag_llm.as_ref(),
            "self_rag_gate"
            | "corrective_rag"
            | "graph_rag"
            | "map_reduce"
            | "contextual_compression" => cfg.llm.as_ref(),
            // Non-LLM stages (search, reranker, etc.)
            _ => return None,
        };
        per_agent.or(cfg.llm.as_ref()).map(|c| c.model.clone())
    }

    /// Get the effective quality threshold (adaptive or configured).
    fn effective_threshold(&self) -> f32 {
        if self.config.adaptive_threshold_enabled {
            f32::from_bits(self.adaptive_threshold.load(Ordering::Relaxed))
        } else {
            self.config.quality_guard_threshold
        }
    }

    /// Update pipeline metadata if the cell is present.
    fn update_metadata(cell: &Option<MetadataCell>, f: impl FnOnce(&mut PipelineMetadata)) {
        if let Some(cell) = cell
            && let Ok(mut meta) = cell.lock()
        {
            f(&mut meta);
        }
    }

    /// Run input guardrails on the user query.
    ///
    /// Returns:
    /// - `None` to proceed with the (possibly sanitized) query in `messages`.
    /// - `Some(refusal)` if the request was blocked — caller must short-circuit.
    ///
    /// Mutates `messages` in place if the verdict is Sanitize.
    fn apply_input_guardrails(
        &self,
        messages: &mut [ChatMessage],
        progress: &Option<ProgressSender>,
        metadata: &Option<MetadataCell>,
    ) -> Option<LlmResponse> {
        let guard = self.input_guardrails.as_ref()?;
        let query = messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        self.emit_progress(progress, "input_guardrails", StageStatus::Started, None);
        let t = Instant::now();
        let verdict = guard.check(&query);
        let dur = t.elapsed().as_millis() as u64;
        self.emit_progress(progress, "input_guardrails", StageStatus::Done, Some(dur));

        let codes: Vec<&str> = verdict.violations.iter().map(|v| v.code.as_str()).collect();
        let passed = verdict.passed();
        let meta_violations = violations_to_meta(&verdict.violations);
        Self::update_metadata(metadata, |m| {
            m.input_guardrails_pass = Some(passed);
            m.guardrail_violations.extend(meta_violations);
        });

        match verdict.action {
            GuardAction::Pass => None,
            GuardAction::Sanitize(new_query) => {
                debug!(?codes, "Input guardrails: sanitized");
                if let Some(last) = messages.last_mut() {
                    last.content = new_query;
                }
                None
            }
            GuardAction::Block { reason } => {
                warn!(?codes, "Input guardrails: BLOCK");
                Some(LlmResponse {
                    content: reason,
                    usage: LlmUsage::default(),
                })
            }
            // Regenerate is an output-side action; treat as pass on input.
            GuardAction::Regenerate { .. } => None,
        }
    }

    /// Run output guardrails on a final response. Returns the (possibly modified)
    /// response. Detector errors are fail-open per `GuardrailsConfig::fail_open`.
    fn apply_output_guardrails(
        &self,
        response: LlmResponse,
        progress: &Option<ProgressSender>,
        metadata: &Option<MetadataCell>,
    ) -> LlmResponse {
        let Some(guard) = self.output_guardrails.as_ref() else {
            return response;
        };

        self.emit_progress(progress, "output_guardrails", StageStatus::Started, None);
        let t = Instant::now();
        let verdict = guard.check(&response.content);
        let dur = t.elapsed().as_millis() as u64;
        self.emit_progress(progress, "output_guardrails", StageStatus::Done, Some(dur));

        let codes: Vec<&str> = verdict.violations.iter().map(|v| v.code.as_str()).collect();
        let passed = verdict.passed();
        let meta_violations = violations_to_meta(&verdict.violations);
        Self::update_metadata(metadata, |m| {
            m.output_guardrails_pass = Some(passed);
            m.guardrail_violations.extend(meta_violations);
        });

        match verdict.action {
            GuardAction::Pass => response,
            GuardAction::Sanitize(new_content) => {
                debug!(?codes, "Output guardrails: redacted");
                LlmResponse {
                    content: new_content,
                    usage: response.usage,
                }
            }
            GuardAction::Block { reason } => {
                warn!(?codes, "Output guardrails: BLOCK");
                LlmResponse {
                    content: reason,
                    usage: response.usage,
                }
            }
            // Regenerate would require re-invoking the generator — out of scope for
            // PR1; treat as redact when the policy says regenerate but we have no
            // retry pathway here.
            GuardAction::Regenerate { .. } => {
                debug!(
                    ?codes,
                    "Output guardrails: regenerate requested but unavailable, redacting"
                );
                response
            }
        }
    }

    /// Non-streaming pipeline: orchestrator decides which agents to run.
    pub async fn process(
        &self,
        messages: &[ChatMessage],
        scope: &AccessScope,
        memories: &[MemoryEntry],
        available_scopes: &[SearchableScope],
        progress: Option<ProgressSender>,
        metadata: Option<MetadataCell>,
    ) -> Result<LlmResponse> {
        let pipeline_start = Instant::now();
        let budget = LlmBudget::new(self.config.max_llm_calls_per_request);

        // Inject memory context if available
        let mut full_messages = self.inject_memory(messages, memories);

        // ── Input Guardrails (before any LLM work) ──
        if let Some(refusal) = self.apply_input_guardrails(&mut full_messages, &progress, &metadata)
        {
            return Ok(refusal);
        }

        let messages = &full_messages;
        let user_query = messages.last().map(|m| m.content.as_str()).unwrap_or("");

        // ── Agent 1: Query Analyzer + Self-RAG gate (concurrent) ──
        self.emit_progress(&progress, "query_analyzer", StageStatus::Started, None);
        self.emit_progress(&progress, "self_rag_gate", StageStatus::Started, None);
        let t = Instant::now();
        let (analysis, self_rag_decision) = tokio::join!(
            self.run_analyzer_budgeted(user_query, messages, &budget),
            async {
                if let Some(ref self_rag) = self.self_rag
                    && budget.try_spend()
                {
                    return self_rag.should_retrieve(user_query, messages).await.ok();
                }
                None
            }
        );
        let analysis = analysis?;
        let analyzer_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            &progress,
            "query_analyzer",
            StageStatus::Done,
            Some(analyzer_ms),
        );
        info!(
            stage = "query_analyzer",
            duration_ms = analyzer_ms,
            "Pipeline stage complete"
        );
        self.emit_progress(
            &progress,
            "self_rag_gate",
            StageStatus::Done,
            Some(analyzer_ms),
        );
        info!(
            stage = "self_rag_gate",
            duration_ms = analyzer_ms,
            "Pipeline stage complete"
        );
        Self::update_metadata(&metadata, |m| {
            m.intent = Some(format!("{:?}", analysis.intent));
            m.language = Some(format!("{:?}", analysis.language));
            m.complexity = Some(format!("{:?}", analysis.complexity));
        });
        if let Some(ref decision) = self_rag_decision {
            Self::update_metadata(&metadata, |m| match decision {
                RetrievalDecision::NoRetrieve { confidence } => {
                    m.self_rag_decision = Some("no_retrieve".into());
                    m.self_rag_confidence = Some(*confidence);
                }
                RetrievalDecision::Retrieve => {
                    m.self_rag_decision = Some("retrieve".into());
                }
            });
        }
        debug!(intent = ?analysis.intent, language = ?analysis.language, "Pipeline: analyzed");

        // ── Handle Self-RAG decision ──
        if let Some(RetrievalDecision::NoRetrieve { confidence }) = self_rag_decision.as_ref() {
            info!(confidence, "Self-RAG: skipping retrieval");
            self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
            let t2 = Instant::now();
            budget.try_spend(); // count the generation call
            let response = if has_vision_content(messages) {
                let vision_msgs = to_vision_messages(messages);
                self.main_llm.generate_vision(&vision_msgs, None).await?
            } else {
                self.main_llm.generate(messages, None).await?
            };
            let gen_ms = t2.elapsed().as_millis() as u64;
            self.emit_progress(
                &progress,
                "response_generator",
                StageStatus::Done,
                Some(gen_ms),
            );
            info!(
                stage = "response_generator",
                duration_ms = gen_ms,
                "Pipeline stage complete"
            );
            self.maybe_run_ragas(user_query, &CuratedContext::default(), &response.content)
                .await;
            let result = self.maybe_adapt(response, &analysis).await;
            Self::update_metadata(&metadata, |m| {
                m.pipeline_route = Some("direct_llm".into());
                m.generation_ms = Some(gen_ms);
            });
            info!(
                total_ms = pipeline_start.elapsed().as_millis() as u64,
                remaining_budget = budget.remaining(),
                "Pipeline: complete"
            );
            return result.map(|r| self.apply_output_guardrails(r, &progress, &metadata));
        }

        // ── Orchestrator: decide route ──
        self.emit_progress(
            &progress,
            "pipeline_orchestrator",
            StageStatus::Started,
            None,
        );
        let t = Instant::now();
        let route = if budget.try_spend() {
            self.decide_route(&analysis).await
        } else {
            // Budget exhausted, use heuristic routing (no LLM call)
            heuristic_decide(&analysis)
        };
        let orch_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            &progress,
            "pipeline_orchestrator",
            StageStatus::Done,
            Some(orch_ms),
        );
        info!(
            stage = "pipeline_orchestrator",
            duration_ms = orch_ms,
            "Pipeline stage complete"
        );
        // When the user is querying within a workspace context, force retrieval
        // unless the query is clearly a greeting/thanks/meta question.
        let route = if route == PipelineRoute::DirectLlm
            && !scope.workspace_ids.is_empty()
            && !matches!(
                analysis.intent,
                QueryIntent::DirectAnswer | QueryIntent::Clarification
            ) {
            debug!("Pipeline: overriding DirectLlm → SimpleRetrieval (workspace context)");
            PipelineRoute::SimpleRetrieval
        } else {
            route
        };

        Self::update_metadata(&metadata, |m| {
            m.pipeline_route = Some(format!("{:?}", route));
        });
        info!(route = ?route, remaining_budget = budget.remaining(), "Pipeline: orchestrator decided");

        let result = match route {
            PipelineRoute::DirectLlm => match analysis.intent {
                QueryIntent::Clarification => Ok(LlmResponse {
                    content: "Could you please provide more details about your question?".into(),
                    usage: LlmUsage::default(),
                }),
                _ => {
                    self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
                    let t = Instant::now();
                    let resp = if has_vision_content(messages) {
                        let vision_msgs = to_vision_messages(messages);
                        self.main_llm.generate_vision(&vision_msgs, None).await?
                    } else {
                        self.main_llm.generate(messages, None).await?
                    };
                    let gen_ms = t.elapsed().as_millis() as u64;
                    self.emit_progress(
                        &progress,
                        "response_generator",
                        StageStatus::Done,
                        Some(gen_ms),
                    );
                    info!(
                        stage = "response_generator",
                        duration_ms = gen_ms,
                        "Pipeline stage complete"
                    );
                    Self::update_metadata(&metadata, |m| {
                        m.generation_ms = Some(gen_ms);
                    });
                    Ok(resp)
                }
            },
            PipelineRoute::SimpleRetrieval => {
                self.emit_progress(&progress, "search", StageStatus::Started, None);
                let t = Instant::now();
                let rewritten = query_rewriter::fallback_rewrite(user_query);
                let results = self
                    .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
                    .await?;
                let search_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(&progress, "search", StageStatus::Done, Some(search_ms));
                info!(
                    stage = "search",
                    duration_ms = search_ms,
                    "Pipeline stage complete"
                );
                debug!(results = results.len(), "Pipeline(simple): searched");
                Self::update_metadata(&metadata, |m| {
                    m.search_ms = Some(search_ms);
                    m.chunks_retrieved = Some(results.len() as u32);
                    m.avg_chunk_score = if results.is_empty() {
                        None
                    } else {
                        Some(results.iter().map(|r| r.score).sum::<f32>() / results.len() as f32)
                    };
                    m.retrieved_chunks = results
                        .iter()
                        .enumerate()
                        .map(|(i, r)| thairag_core::types::RetrievedChunkMeta {
                            chunk_id: r.chunk.chunk_id.to_string(),
                            doc_id: r.chunk.doc_id.to_string(),
                            doc_title: None,
                            content_preview: r.chunk.content.chars().take(200).collect(),
                            score: r.score,
                            rank: i as u32,
                            contributed: true,
                        })
                        .collect();
                });

                self.emit_progress(&progress, "context_curator", StageStatus::Started, None);
                let t = Instant::now();
                let context = self
                    .run_curator_budgeted(user_query, &results, &budget)
                    .await?;
                let curator_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    &progress,
                    "context_curator",
                    StageStatus::Done,
                    Some(curator_ms),
                );
                info!(
                    stage = "context_curator",
                    duration_ms = curator_ms,
                    "Pipeline stage complete"
                );

                // Retrieval refinement (budget-aware: skip if < 2 calls remain)
                let context = if budget.remaining() >= 2 {
                    self.maybe_refine_retrieval(
                        user_query,
                        &analysis,
                        scope,
                        context,
                        available_scopes,
                    )
                    .await?
                } else {
                    context
                };

                self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
                let t = Instant::now();
                budget.try_spend();
                let response = self
                    .response_generator
                    .generate(&analysis, &context, messages, None)
                    .await?;
                let gen_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    &progress,
                    "response_generator",
                    StageStatus::Done,
                    Some(gen_ms),
                );
                info!(
                    stage = "response_generator",
                    duration_ms = gen_ms,
                    "Pipeline stage complete"
                );
                Self::update_metadata(&metadata, |m| {
                    m.generation_ms = Some(gen_ms);
                });
                self.maybe_adapt(response, &analysis).await
            }
            PipelineRoute::FullPipeline => {
                self.execute_full(
                    user_query,
                    messages,
                    scope,
                    &analysis,
                    false,
                    available_scopes,
                    &progress,
                    &budget,
                    &metadata,
                )
                .await
            }
            PipelineRoute::ComplexPipeline => {
                self.execute_full(
                    user_query,
                    messages,
                    scope,
                    &analysis,
                    true,
                    available_scopes,
                    &progress,
                    &budget,
                    &metadata,
                )
                .await
            }
        };
        info!(
            total_ms = pipeline_start.elapsed().as_millis() as u64,
            remaining_budget = budget.remaining(),
            "Pipeline: complete"
        );
        result.map(|r| self.apply_output_guardrails(r, &progress, &metadata))
    }

    /// Execute the full pipeline (agents 2-6).
    #[allow(clippy::too_many_arguments)]
    async fn execute_full(
        &self,
        user_query: &str,
        messages: &[ChatMessage],
        scope: &AccessScope,
        analysis: &QueryAnalysis,
        force_quality_guard: bool,
        available_scopes: &[SearchableScope],
        progress: &Option<ProgressSender>,
        budget: &LlmBudget,
        metadata: &Option<MetadataCell>,
    ) -> Result<LlmResponse> {
        // ── Agent 2: Query Rewriter ──
        self.emit_progress(progress, "query_rewriter", StageStatus::Started, None);
        let t = Instant::now();
        let rewritten = self
            .run_rewriter_budgeted(user_query, analysis, budget)
            .await?;
        let rewriter_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            progress,
            "query_rewriter",
            StageStatus::Done,
            Some(rewriter_ms),
        );
        info!(
            stage = "query_rewriter",
            duration_ms = rewriter_ms,
            "Pipeline stage complete"
        );
        debug!(primary = %rewritten.primary, sub = rewritten.sub_queries.len(), "Pipeline: rewritten");

        // ── Search (with tool router if enabled) ──
        self.emit_progress(progress, "search", StageStatus::Started, None);
        let t = Instant::now();
        let mut results = self
            .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
            .await?;
        let search_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(progress, "search", StageStatus::Done, Some(search_ms));
        info!(
            stage = "search",
            duration_ms = search_ms,
            "Pipeline stage complete"
        );
        debug!(results = results.len(), "Pipeline: searched");
        Self::update_metadata(metadata, |m| {
            m.search_ms = Some(search_ms);
            m.chunks_retrieved = Some(results.len() as u32);
            m.avg_chunk_score = if results.is_empty() {
                None
            } else {
                Some(results.iter().map(|r| r.score).sum::<f32>() / results.len() as f32)
            };
            m.retrieved_chunks = results
                .iter()
                .enumerate()
                .map(|(i, r)| thairag_core::types::RetrievedChunkMeta {
                    chunk_id: r.chunk.chunk_id.to_string(),
                    doc_id: r.chunk.doc_id.to_string(),
                    doc_title: None,
                    content_preview: r.chunk.content.chars().take(200).collect(),
                    score: r.score,
                    rank: i as u32,
                    contributed: true,
                })
                .collect();
        });

        // ── ColBERT reranking (skip if budget low — needs at least 3 more calls) ──
        if let Some(ref colbert) = self.colbert_reranker {
            if budget.try_spend() {
                self.emit_progress(progress, "colbert_reranker", StageStatus::Started, None);
                let t = Instant::now();
                results = colbert.rerank(user_query, &results).await?;
                let colbert_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    progress,
                    "colbert_reranker",
                    StageStatus::Done,
                    Some(colbert_ms),
                );
                info!(
                    stage = "colbert_reranker",
                    duration_ms = colbert_ms,
                    "Pipeline stage complete"
                );
                debug!(results = results.len(), "Pipeline: ColBERT reranked");
            } else {
                debug!("Pipeline: skipping ColBERT reranking (budget exhausted)");
            }
        }

        // ── Active Learning: adjust scores from feedback history (no LLM call) ──
        if let Some(ref al) = self.active_learning {
            al.adjust_scores(&mut results);
        }

        // ── Graph RAG: skip if budget low (uses 2 LLM calls) ──
        if let Some(ref graph_rag) = self.graph_rag {
            if budget.remaining() >= 4 {
                // Reserve budget for graph_rag (2) + curator (1) + generator (1)
                self.emit_progress(progress, "graph_rag", StageStatus::Started, None);
                let t = Instant::now();
                let graph = self.knowledge_graph.read().unwrap().clone();
                if graph.entity_count() > 0 {
                    budget.try_spend();
                    results = graph_rag
                        .enhance_results(user_query, &results, &graph)
                        .await?;
                    debug!(results = results.len(), "Pipeline: graph-enhanced");
                }
                if !results.is_empty() && budget.try_spend() {
                    let texts: Vec<String> = results
                        .iter()
                        .take(3)
                        .map(|r| r.chunk.content.clone())
                        .collect();
                    let combined = texts.join("\n\n");
                    if let Ok(extraction) = graph_rag.extract_entities(&combined).await {
                        let mut graph = self.knowledge_graph.write().unwrap();
                        for entity in extraction.entities {
                            graph.add_entity(entity);
                        }
                        for rel in extraction.relationships {
                            graph.add_relationship(rel);
                        }
                    }
                }
                let graph_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(progress, "graph_rag", StageStatus::Done, Some(graph_ms));
                info!(
                    stage = "graph_rag",
                    duration_ms = graph_ms,
                    "Pipeline stage complete"
                );
            } else {
                debug!(
                    remaining = budget.remaining(),
                    "Pipeline: skipping graph_rag (budget low)"
                );
            }
        }

        // ── Agent 3: Context Curator ──
        self.emit_progress(progress, "context_curator", StageStatus::Started, None);
        let t = Instant::now();
        let context = self
            .run_curator_budgeted(user_query, &results, budget)
            .await?;
        let curator_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            progress,
            "context_curator",
            StageStatus::Done,
            Some(curator_ms),
        );
        info!(
            stage = "context_curator",
            duration_ms = curator_ms,
            "Pipeline stage complete"
        );
        debug!(
            chunks = context.chunks.len(),
            tokens = context.total_tokens_est,
            "Pipeline: curated"
        );

        // ── Retrieval Refinement (skip if budget low — needs 2+ calls per retry) ──
        let context = if self.config.retrieval_refinement_enabled && budget.remaining() >= 4 {
            self.emit_progress(progress, "retrieval_refinement", StageStatus::Started, None);
            let t = Instant::now();
            let context_inner = self
                .maybe_refine_retrieval(user_query, analysis, scope, context, available_scopes)
                .await?;
            let refine_ms = t.elapsed().as_millis() as u64;
            self.emit_progress(
                progress,
                "retrieval_refinement",
                StageStatus::Done,
                Some(refine_ms),
            );
            info!(
                stage = "retrieval_refinement",
                duration_ms = refine_ms,
                "Pipeline stage complete"
            );
            context_inner
        } else {
            if self.config.retrieval_refinement_enabled {
                debug!(
                    remaining = budget.remaining(),
                    "Pipeline: skipping retrieval refinement (budget low)"
                );
            }
            context
        };

        self.execute_post_retrieval(
            user_query,
            messages,
            scope,
            analysis,
            &results,
            context,
            force_quality_guard,
            progress,
            budget,
            metadata,
        )
        .await
    }

    /// Post-retrieval pipeline stages (CRAG, live retrieval, RAPTOR, compression, generation, quality guard).
    #[allow(clippy::too_many_arguments)]
    async fn execute_post_retrieval(
        &self,
        user_query: &str,
        messages: &[ChatMessage],
        scope: &AccessScope,
        analysis: &QueryAnalysis,
        results: &[thairag_core::types::SearchResult],
        context: CuratedContext,
        force_quality_guard: bool,
        progress: &Option<ProgressSender>,
        budget: &LlmBudget,
        metadata: &Option<MetadataCell>,
    ) -> Result<LlmResponse> {
        // ── CRAG: check context quality (skip if budget low) ──
        let context =
            if self.corrective_rag.is_some() && self.config.crag_enabled && budget.remaining() >= 3
            {
                self.emit_progress(progress, "corrective_rag", StageStatus::Started, None);
                let t = Instant::now();
                budget.try_spend();
                let ctx = self.maybe_corrective_rag(user_query, context).await?;
                let crag_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(progress, "corrective_rag", StageStatus::Done, Some(crag_ms));
                info!(
                    stage = "corrective_rag",
                    duration_ms = crag_ms,
                    "Pipeline stage complete"
                );
                ctx
            } else {
                if self.corrective_rag.is_some() && self.config.crag_enabled {
                    debug!(
                        remaining = budget.remaining(),
                        "Pipeline: skipping CRAG (budget low)"
                    );
                }
                context
            };

        // ── Live Source Retrieval: fetch from connectors if KB context insufficient ──
        let context = self
            .maybe_live_retrieve(user_query, scope, context, budget, progress)
            .await?;

        // ── RAPTOR: build hierarchical summary tree (skip if budget low) ──
        let context = if let Some(ref raptor) = self.raptor {
            if budget.remaining() >= 3 {
                self.emit_progress(progress, "raptor", StageStatus::Started, None);
                let t = Instant::now();
                budget.try_spend();
                let ctx = raptor.build_tree(user_query, &context).await?;
                let raptor_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(progress, "raptor", StageStatus::Done, Some(raptor_ms));
                info!(
                    stage = "raptor",
                    duration_ms = raptor_ms,
                    "Pipeline stage complete"
                );
                ctx
            } else {
                debug!(
                    remaining = budget.remaining(),
                    "Pipeline: skipping RAPTOR (budget low)"
                );
                context
            }
        } else {
            context
        };

        // ── Contextual Compression (skip if budget low) ──
        let context = if let Some(ref compressor) = self.contextual_compression {
            if budget.remaining() >= 3 {
                self.emit_progress(
                    progress,
                    "contextual_compression",
                    StageStatus::Started,
                    None,
                );
                let t = Instant::now();
                budget.try_spend();
                let ctx = compressor.compress(user_query, &context).await?;
                let compress_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    progress,
                    "contextual_compression",
                    StageStatus::Done,
                    Some(compress_ms),
                );
                info!(
                    stage = "contextual_compression",
                    duration_ms = compress_ms,
                    "Pipeline stage complete"
                );
                ctx
            } else {
                debug!(
                    remaining = budget.remaining(),
                    "Pipeline: skipping compression (budget low)"
                );
                context
            }
        } else {
            context
        };

        // ── Multi-modal RAG (skip if budget low) ──
        let context = if let Some(ref mm) = self.multimodal_rag {
            if budget.remaining() >= 3 {
                self.emit_progress(progress, "multimodal_rag", StageStatus::Started, None);
                let t = Instant::now();
                budget.try_spend();
                let ctx = mm.enrich_context(user_query, &context).await?;
                let mm_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(progress, "multimodal_rag", StageStatus::Done, Some(mm_ms));
                info!(
                    stage = "multimodal_rag",
                    duration_ms = mm_ms,
                    "Pipeline stage complete"
                );
                ctx
            } else {
                debug!(
                    remaining = budget.remaining(),
                    "Pipeline: skipping multimodal RAG (budget low)"
                );
                context
            }
        } else {
            context
        };

        // ── Map-Reduce (skip if budget low) ──
        if let Some(ref mr) = self.map_reduce
            && mr.should_use(analysis, results)
            && budget.remaining() >= 2
        {
            self.emit_progress(progress, "map_reduce", StageStatus::Started, None);
            let t = Instant::now();
            info!("Pipeline: using map-reduce for synthesis query");
            budget.try_spend();
            let response = mr.process(user_query, results).await?;
            let mr_ms = t.elapsed().as_millis() as u64;
            self.emit_progress(progress, "map_reduce", StageStatus::Done, Some(mr_ms));
            info!(
                stage = "map_reduce",
                duration_ms = mr_ms,
                "Pipeline stage complete"
            );
            self.maybe_run_ragas(user_query, &context, &response.content)
                .await;
            return self.maybe_adapt(response, analysis).await;
        }

        // ── Agent 4: Response Generator (always runs — this is the core) ──
        self.emit_progress(progress, "response_generator", StageStatus::Started, None);
        let t = Instant::now();
        budget.try_spend();
        let mut response = if let Some(ref spec) = self.speculative_rag {
            info!("Pipeline: using speculative generation");
            spec.speculative_generate(analysis, &context, messages, user_query)
                .await?
        } else {
            self.response_generator
                .generate(analysis, &context, messages, None)
                .await?
        };
        let gen_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            progress,
            "response_generator",
            StageStatus::Done,
            Some(gen_ms),
        );
        info!(
            stage = "response_generator",
            duration_ms = gen_ms,
            "Pipeline stage complete"
        );
        debug!(len = response.content.len(), "Pipeline: generated");
        Self::update_metadata(metadata, |m| {
            m.generation_ms = Some(gen_ms);
        });

        // ── Agent 5: Quality Guard (budget-aware retry loop) ──
        let threshold = self.effective_threshold();
        let run_guard = force_quality_guard || self.quality_guard.is_some();
        if run_guard
            && let Some(ref guard) = self.quality_guard
            && budget.try_spend()
        {
            self.emit_progress(progress, "quality_guard", StageStatus::Started, None);
            let t = Instant::now();
            for attempt in 0..=self.config.quality_guard_max_retries {
                let verdict = guard
                    .check_with_threshold(user_query, &response.content, &context, threshold)
                    .await?;
                if verdict.pass {
                    Self::update_metadata(metadata, |m| {
                        m.quality_guard_pass = Some(true);
                    });
                    debug!(attempt, "Pipeline: quality passed");
                    break;
                }
                if attempt < self.config.quality_guard_max_retries && budget.try_spend() {
                    let feedback = verdict
                        .feedback
                        .unwrap_or_else(|| "Improve relevance and reduce hallucination.".into());
                    warn!(attempt, feedback = %feedback, "Pipeline: quality failed, retrying");
                    response = self
                        .response_generator
                        .generate_with_feedback(analysis, &context, messages, &feedback, None)
                        .await?;
                } else {
                    Self::update_metadata(metadata, |m| {
                        m.quality_guard_pass = Some(false);
                    });
                    warn!(
                        "Pipeline: quality guard exhausted retries or budget, using last response"
                    );
                    break;
                }
            }
            let guard_ms = t.elapsed().as_millis() as u64;
            self.emit_progress(progress, "quality_guard", StageStatus::Done, Some(guard_ms));
            info!(
                stage = "quality_guard",
                duration_ms = guard_ms,
                "Pipeline stage complete"
            );
        }

        // ── RAGAS evaluation (async, sampled — no budget impact) ──
        self.maybe_run_ragas(user_query, &context, &response.content)
            .await;

        // ── Agent 6: Language Adapter (skip if query is English — LLMs default to English output) ──
        let needs_adaptation = !matches!(analysis.language, QueryLanguage::English);
        if self.language_adapter.is_some() && needs_adaptation && budget.try_spend() {
            self.emit_progress(progress, "language_adapter", StageStatus::Started, None);
            let t = Instant::now();
            let response = self.maybe_adapt(response, analysis).await?;
            let adapt_ms = t.elapsed().as_millis() as u64;
            self.emit_progress(
                progress,
                "language_adapter",
                StageStatus::Done,
                Some(adapt_ms),
            );
            info!(
                stage = "language_adapter",
                duration_ms = adapt_ms,
                "Pipeline stage complete"
            );
            return Ok(response);
        }

        Ok(response)
    }

    /// Apply language adapter if configured.
    async fn maybe_adapt(
        &self,
        mut response: LlmResponse,
        analysis: &QueryAnalysis,
    ) -> Result<LlmResponse> {
        if let Some(ref adapter) = self.language_adapter {
            let adapted = adapter.adapt(&response.content, &analysis.language).await?;
            response.content = adapted;
        }
        Ok(response)
    }

    /// Inject conversation memory into the message list.
    fn inject_memory(
        &self,
        messages: &[ChatMessage],
        memories: &[MemoryEntry],
    ) -> Vec<ChatMessage> {
        if memories.is_empty() || !self.config.conversation_memory_enabled {
            return messages.to_vec();
        }
        let mut full = Vec::with_capacity(messages.len() + 1);
        if let Some(mem_msg) = ConversationMemory::build_memory_context(memories, &self.prompts) {
            full.push(mem_msg);
        }
        full.extend_from_slice(messages);
        full
    }

    /// Feature 2: Multi-turn Retrieval Refinement.
    /// If context quality is too low and refinement is enabled, rewrite and retry search.
    async fn maybe_refine_retrieval(
        &self,
        user_query: &str,
        analysis: &QueryAnalysis,
        scope: &AccessScope,
        context: CuratedContext,
        available_scopes: &[SearchableScope],
    ) -> Result<CuratedContext> {
        if !self.config.retrieval_refinement_enabled {
            return Ok(context);
        }

        let avg_score = if context.chunks.is_empty() {
            0.0
        } else {
            context
                .chunks
                .iter()
                .map(|c| c.relevance_score)
                .sum::<f32>()
                / context.chunks.len() as f32
        };

        if avg_score >= self.config.refinement_min_relevance {
            return Ok(context);
        }

        // If we have chunks but scores are 0.0 (uncalibrated vector store), skip
        // refinement. Score=0.0 means the source doesn't provide calibrated scores,
        // not that results are irrelevant.
        if !context.chunks.is_empty() && avg_score == 0.0 {
            debug!(
                chunks = context.chunks.len(),
                "Pipeline: skipping refinement (scores uncalibrated, have chunks)"
            );
            return Ok(context);
        }

        info!(
            avg_score,
            threshold = self.config.refinement_min_relevance,
            "Pipeline: context quality below threshold, attempting retrieval refinement"
        );

        let rewriter = match &self.query_rewriter {
            Some(r) => r,
            None => return Ok(context), // No rewriter, can't refine
        };

        let mut best_context = context;
        let max_retries = self.config.refinement_max_retries.min(2); // Cap at 2 to limit LLM calls
        for attempt in 0..max_retries {
            let feedback = format!(
                "Previous search returned results with avg relevance {:.2}. \
                 Try different keywords or broader/narrower terms.",
                avg_score
            );
            let alt_rewritten = rewriter
                .rewrite_with_feedback(user_query, analysis, &feedback)
                .await?;

            let mut alt_results = self
                .run_search_with_tools(&alt_rewritten, scope, user_query, available_scopes)
                .await?;

            // Merge with previous results
            let prev_results: Vec<thairag_core::types::SearchResult> = best_context
                .chunks
                .iter()
                .map(|c| thairag_core::types::SearchResult {
                    chunk: thairag_core::types::DocumentChunk {
                        chunk_id: Default::default(),
                        doc_id: Default::default(),
                        workspace_id: Default::default(),
                        content: c.content.clone(),
                        chunk_index: 0,
                        embedding: None,
                        metadata: None,
                    },
                    score: c.relevance_score,
                })
                .collect();
            alt_results.extend(prev_results);
            deduplicate_results(&mut alt_results);

            let new_context = self.run_curator(user_query, &alt_results).await?;
            let new_avg = if new_context.chunks.is_empty() {
                0.0
            } else {
                new_context
                    .chunks
                    .iter()
                    .map(|c| c.relevance_score)
                    .sum::<f32>()
                    / new_context.chunks.len() as f32
            };

            debug!(
                attempt,
                old_avg = avg_score,
                new_avg,
                "Retrieval refinement attempt"
            );

            if new_avg > avg_score {
                best_context = new_context;
                if new_avg >= self.config.refinement_min_relevance {
                    info!(new_avg, "Retrieval refinement succeeded");
                    break;
                }
            }
        }

        Ok(best_context)
    }

    /// Streaming pipeline with 3-layer defense.
    pub async fn process_stream(
        &self,
        messages: &[ChatMessage],
        scope: &AccessScope,
        memories: &[MemoryEntry],
        available_scopes: &[SearchableScope],
        progress: Option<ProgressSender>,
        metadata: Option<MetadataCell>,
    ) -> Result<LlmStreamResponse> {
        let pipeline_start = Instant::now();
        let budget = LlmBudget::new(self.config.max_llm_calls_per_request);

        let mut full_messages = self.inject_memory(messages, memories);

        // ── Input Guardrails (before any LLM work) ──
        if let Some(refusal) = self.apply_input_guardrails(&mut full_messages, &progress, &metadata)
        {
            return Ok(Self::refusal_stream(refusal.content));
        }

        let messages = &full_messages;

        let user_query = messages.last().map(|m| m.content.as_str()).unwrap_or("");

        // ── Agent 1: Query Analyzer + Self-RAG gate (concurrent) ──
        self.emit_progress(&progress, "query_analyzer", StageStatus::Started, None);
        self.emit_progress(&progress, "self_rag_gate", StageStatus::Started, None);
        let t = Instant::now();
        let (analysis, self_rag_decision) = tokio::join!(
            self.run_analyzer_budgeted(user_query, messages, &budget),
            async {
                if let Some(ref self_rag) = self.self_rag
                    && budget.try_spend()
                {
                    return self_rag.should_retrieve(user_query, messages).await.ok();
                }
                None
            }
        );
        let analysis = analysis?;
        let analyzer_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            &progress,
            "query_analyzer",
            StageStatus::Done,
            Some(analyzer_ms),
        );
        info!(
            stage = "query_analyzer",
            duration_ms = analyzer_ms,
            "Pipeline stage complete"
        );
        self.emit_progress(
            &progress,
            "self_rag_gate",
            StageStatus::Done,
            Some(analyzer_ms),
        );
        info!(
            stage = "self_rag_gate",
            duration_ms = analyzer_ms,
            "Pipeline stage complete"
        );
        Self::update_metadata(&metadata, |m| {
            m.intent = Some(format!("{:?}", analysis.intent));
            m.language = Some(format!("{:?}", analysis.language));
            m.complexity = Some(format!("{:?}", analysis.complexity));
        });
        if let Some(ref decision) = self_rag_decision {
            Self::update_metadata(&metadata, |m| match decision {
                RetrievalDecision::NoRetrieve { confidence } => {
                    m.self_rag_decision = Some("no_retrieve".into());
                    m.self_rag_confidence = Some(*confidence);
                }
                RetrievalDecision::Retrieve => {
                    m.self_rag_decision = Some("retrieve".into());
                }
            });
        }

        // ── Handle Self-RAG decision (streaming) ──
        if let Some(RetrievalDecision::NoRetrieve { confidence }) = self_rag_decision.as_ref() {
            info!(confidence, "Self-RAG(stream): skipping retrieval");
            self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
            Self::update_metadata(&metadata, |m| {
                m.pipeline_route = Some("direct_llm".into());
            });
            info!(
                total_ms = pipeline_start.elapsed().as_millis() as u64,
                remaining_budget = budget.remaining(),
                "Pipeline: complete"
            );
            let stream = self.main_llm.generate_stream(messages, None).await?;
            return Ok(self.wrap_stream_with_output_guardrails(
                stream,
                progress.clone(),
                metadata.clone(),
            ));
        }

        // ── Orchestrator: decide route ──
        self.emit_progress(
            &progress,
            "pipeline_orchestrator",
            StageStatus::Started,
            None,
        );
        let t = Instant::now();
        let route = if budget.try_spend() {
            self.decide_route(&analysis).await
        } else {
            heuristic_decide(&analysis)
        };
        let orch_ms = t.elapsed().as_millis() as u64;
        self.emit_progress(
            &progress,
            "pipeline_orchestrator",
            StageStatus::Done,
            Some(orch_ms),
        );
        info!(
            stage = "pipeline_orchestrator",
            duration_ms = orch_ms,
            "Pipeline stage complete"
        );
        // When the user is querying within a workspace context, force retrieval
        let route = if route == PipelineRoute::DirectLlm
            && !scope.workspace_ids.is_empty()
            && !matches!(
                analysis.intent,
                QueryIntent::DirectAnswer | QueryIntent::Clarification
            ) {
            debug!("Pipeline(stream): overriding DirectLlm → SimpleRetrieval (workspace context)");
            PipelineRoute::SimpleRetrieval
        } else {
            route
        };

        Self::update_metadata(&metadata, |m| {
            m.pipeline_route = Some(format!("{:?}", route));
        });
        debug!(route = ?route, remaining_budget = budget.remaining(), "Pipeline(stream): orchestrator decided");

        match route {
            PipelineRoute::DirectLlm => match analysis.intent {
                QueryIntent::Clarification => {
                    let msg = "Could you please provide more details about your question?".into();
                    info!(
                        total_ms = pipeline_start.elapsed().as_millis() as u64,
                        remaining_budget = budget.remaining(),
                        "Pipeline: complete"
                    );
                    Ok(LlmStreamResponse {
                        stream: Box::pin(tokio_stream::once(Ok(msg))),
                        usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
                    })
                }
                _ => {
                    self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
                    info!(
                        total_ms = pipeline_start.elapsed().as_millis() as u64,
                        remaining_budget = budget.remaining(),
                        "Pipeline: complete"
                    );
                    self.main_llm.generate_stream(messages, None).await
                }
            },
            PipelineRoute::SimpleRetrieval => {
                self.emit_progress(&progress, "search", StageStatus::Started, None);
                let t = Instant::now();
                let rewritten = query_rewriter::fallback_rewrite(user_query);
                let results = self
                    .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
                    .await?;
                let search_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(&progress, "search", StageStatus::Done, Some(search_ms));
                info!(
                    stage = "search",
                    duration_ms = search_ms,
                    "Pipeline stage complete"
                );
                Self::update_metadata(&metadata, |m| {
                    m.search_ms = Some(search_ms);
                    m.chunks_retrieved = Some(results.len() as u32);
                    m.avg_chunk_score = if results.is_empty() {
                        None
                    } else {
                        Some(results.iter().map(|r| r.score).sum::<f32>() / results.len() as f32)
                    };
                    m.retrieved_chunks = results
                        .iter()
                        .enumerate()
                        .map(|(i, r)| thairag_core::types::RetrievedChunkMeta {
                            chunk_id: r.chunk.chunk_id.to_string(),
                            doc_id: r.chunk.doc_id.to_string(),
                            doc_title: None,
                            content_preview: r.chunk.content.chars().take(200).collect(),
                            score: r.score,
                            rank: i as u32,
                            contributed: true,
                        })
                        .collect();
                });

                self.emit_progress(&progress, "context_curator", StageStatus::Started, None);
                let t = Instant::now();
                let context = self
                    .run_curator_budgeted(user_query, &results, &budget)
                    .await?;
                let curator_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    &progress,
                    "context_curator",
                    StageStatus::Done,
                    Some(curator_ms),
                );
                info!(
                    stage = "context_curator",
                    duration_ms = curator_ms,
                    "Pipeline stage complete"
                );

                let context = if budget.remaining() >= 2 {
                    self.maybe_refine_retrieval(
                        user_query,
                        &analysis,
                        scope,
                        context,
                        available_scopes,
                    )
                    .await?
                } else {
                    context
                };
                let context = self
                    .maybe_live_retrieve(user_query, scope, context, &budget, &progress)
                    .await?;
                if let Some(resp) = self.context_insufficient_response(&context) {
                    info!(
                        total_ms = pipeline_start.elapsed().as_millis() as u64,
                        remaining_budget = budget.remaining(),
                        "Pipeline: complete"
                    );
                    return Ok(resp);
                }
                self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
                budget.try_spend();
                let stream = self
                    .response_generator
                    .generate_stream(&analysis, &context, messages, None)
                    .await?;
                info!(
                    total_ms = pipeline_start.elapsed().as_millis() as u64,
                    remaining_budget = budget.remaining(),
                    "Pipeline: complete"
                );
                let stream = self.wrap_stream_with_quality_guard(stream, user_query, context);
                Ok(self.wrap_stream_with_output_guardrails(
                    stream,
                    progress.clone(),
                    metadata.clone(),
                ))
            }
            PipelineRoute::FullPipeline | PipelineRoute::ComplexPipeline => {
                self.emit_progress(&progress, "query_rewriter", StageStatus::Started, None);
                let t = Instant::now();
                let rewritten = self
                    .run_rewriter_budgeted(user_query, &analysis, &budget)
                    .await?;
                let rewriter_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    &progress,
                    "query_rewriter",
                    StageStatus::Done,
                    Some(rewriter_ms),
                );
                info!(
                    stage = "query_rewriter",
                    duration_ms = rewriter_ms,
                    "Pipeline stage complete"
                );

                self.emit_progress(&progress, "search", StageStatus::Started, None);
                let t = Instant::now();
                let results = self
                    .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
                    .await?;
                let search_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(&progress, "search", StageStatus::Done, Some(search_ms));
                info!(
                    stage = "search",
                    duration_ms = search_ms,
                    "Pipeline stage complete"
                );
                Self::update_metadata(&metadata, |m| {
                    m.search_ms = Some(search_ms);
                    m.chunks_retrieved = Some(results.len() as u32);
                    m.avg_chunk_score = if results.is_empty() {
                        None
                    } else {
                        Some(results.iter().map(|r| r.score).sum::<f32>() / results.len() as f32)
                    };
                    m.retrieved_chunks = results
                        .iter()
                        .enumerate()
                        .map(|(i, r)| thairag_core::types::RetrievedChunkMeta {
                            chunk_id: r.chunk.chunk_id.to_string(),
                            doc_id: r.chunk.doc_id.to_string(),
                            doc_title: None,
                            content_preview: r.chunk.content.chars().take(200).collect(),
                            score: r.score,
                            rank: i as u32,
                            contributed: true,
                        })
                        .collect();
                });

                self.emit_progress(&progress, "context_curator", StageStatus::Started, None);
                let t = Instant::now();
                let context = self
                    .run_curator_budgeted(user_query, &results, &budget)
                    .await?;
                let curator_ms = t.elapsed().as_millis() as u64;
                self.emit_progress(
                    &progress,
                    "context_curator",
                    StageStatus::Done,
                    Some(curator_ms),
                );
                info!(
                    stage = "context_curator",
                    duration_ms = curator_ms,
                    "Pipeline stage complete"
                );

                let context = if budget.remaining() >= 3 {
                    self.maybe_refine_retrieval(
                        user_query,
                        &analysis,
                        scope,
                        context,
                        available_scopes,
                    )
                    .await?
                } else {
                    context
                };
                let context = self
                    .maybe_live_retrieve(user_query, scope, context, &budget, &progress)
                    .await?;
                if let Some(resp) = self.context_insufficient_response(&context) {
                    info!(
                        total_ms = pipeline_start.elapsed().as_millis() as u64,
                        remaining_budget = budget.remaining(),
                        "Pipeline: complete"
                    );
                    return Ok(resp);
                }
                self.emit_progress(&progress, "response_generator", StageStatus::Started, None);
                budget.try_spend();
                let stream = self
                    .response_generator
                    .generate_stream(&analysis, &context, messages, None)
                    .await?;
                info!(
                    total_ms = pipeline_start.elapsed().as_millis() as u64,
                    remaining_budget = budget.remaining(),
                    "Pipeline: complete"
                );
                let stream = self.wrap_stream_with_quality_guard(stream, user_query, context);
                Ok(self.wrap_stream_with_output_guardrails(
                    stream,
                    progress.clone(),
                    metadata.clone(),
                ))
            }
        }
    }

    /// Attempt live retrieval from MCP connectors when KB context is insufficient.
    async fn maybe_live_retrieve(
        &self,
        query: &str,
        scope: &AccessScope,
        context: CuratedContext,
        budget: &LlmBudget,
        progress: &Option<ProgressSender>,
    ) -> Result<CuratedContext> {
        // Check if live retrieval is enabled and conditions are met
        if !self.config.live_retrieval_enabled {
            return Ok(context);
        }
        let live = match &self.live_retrieval {
            Some(lr) => lr,
            None => return Ok(context),
        };
        let connector_provider = match &self.connector_provider {
            Some(cp) => cp,
            None => return Ok(context),
        };
        if budget.remaining() < 2 {
            debug!(
                remaining = budget.remaining(),
                "Pipeline: skipping live retrieval (budget low)"
            );
            return Ok(context);
        }

        // Check if context is actually insufficient
        let is_empty = context.chunks.is_empty();
        let avg_score = if context.chunks.is_empty() {
            0.0
        } else {
            context
                .chunks
                .iter()
                .map(|c| c.relevance_score)
                .sum::<f32>()
                / context.chunks.len() as f32
        };
        if !is_empty && avg_score >= 0.15 {
            return Ok(context);
        }

        // Get active connectors for this scope
        let connectors = connector_provider(scope);
        if connectors.is_empty() {
            debug!("Pipeline: no connectors available for live retrieval");
            return Ok(context);
        }

        self.emit_progress(progress, "live_retrieval", StageStatus::Started, None);
        let t = std::time::Instant::now();
        budget.try_spend();

        let live_context = live.fetch_live_context(query, &connectors).await?;

        self.emit_progress(
            progress,
            "live_retrieval",
            StageStatus::Done,
            Some(t.elapsed().as_millis() as u64),
        );

        if live_context.chunks.is_empty() {
            debug!("Pipeline: live retrieval returned no results");
            Ok(context) // Keep original (even if poor)
        } else {
            info!(
                chunks = live_context.chunks.len(),
                "Pipeline: using live-retrieved context"
            );
            Ok(live_context)
        }
    }

    /// Layer 1: Pre-stream context guard.
    fn context_insufficient_response(&self, context: &CuratedContext) -> Option<LlmStreamResponse> {
        if context.chunks.is_empty() {
            let msg = "I don't have enough information in the knowledge base to answer this question. \
                       Please try rephrasing your query or check if the relevant documents have been uploaded."
                .to_string();
            info!("Pipeline(stream): no context, returning insufficient-info response");
            return Some(LlmStreamResponse {
                stream: Box::pin(tokio_stream::once(Ok(msg))),
                usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
            });
        }

        let avg_score = context
            .chunks
            .iter()
            .map(|c| c.relevance_score)
            .sum::<f32>()
            / context.chunks.len() as f32;
        if avg_score < 0.15 {
            let msg =
                "I found some documents but they don't appear to be relevant to your question. \
                       Could you rephrase your query or provide more details?"
                    .to_string();
            info!(
                avg_score,
                "Pipeline(stream): context too low quality, returning insufficient-info response"
            );
            return Some(LlmStreamResponse {
                stream: Box::pin(tokio_stream::once(Ok(msg))),
                usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
            });
        }

        None
    }

    /// Layer 3: Post-stream quality check.
    fn wrap_stream_with_quality_guard(
        &self,
        inner: LlmStreamResponse,
        user_query: &str,
        context: CuratedContext,
    ) -> LlmStreamResponse {
        let guard_clone = match &self.quality_guard {
            Some(g) => Some(Arc::clone(g)),
            None => return inner,
        };

        let query = user_query.to_string();
        let usage = inner.usage.clone();
        let threshold = self.effective_threshold();

        let stream = async_stream::stream! {
            let mut inner_stream = inner.stream;
            let mut collected = String::new();

            while let Some(chunk) = inner_stream.next().await {
                if let Ok(text) = &chunk { collected.push_str(text) }
                yield chunk;
            }

            if !collected.is_empty() && let Some(ref guard) = guard_clone {
                match guard.check_with_threshold(&query, &collected, &context, threshold).await {
                    Ok(verdict) => {
                        if !verdict.pass {
                            warn!(
                                relevance = verdict.relevance_score,
                                hallucination = verdict.hallucination_score,
                                "Pipeline(stream): post-stream quality guard FAILED"
                            );
                            yield Ok("\n\n---\n⚠️ *Note: This response may contain inaccuracies. \
                                     Please verify important information against the source documents.*"
                                .to_string());
                        } else {
                            debug!("Pipeline(stream): post-stream quality guard passed");
                        }
                    }
                    Err(e) => {
                        debug!(error = %e, "Pipeline(stream): post-stream quality guard error, skipping");
                    }
                }
            }
        };

        LlmStreamResponse {
            stream: Box::pin(stream),
            usage,
        }
    }

    /// Build a single-chunk refusal stream (used when input guardrails block).
    fn refusal_stream(content: String) -> LlmStreamResponse {
        let stream = async_stream::stream! {
            yield Ok::<_, thairag_core::error::ThaiRagError>(content);
        };
        LlmStreamResponse {
            stream: Box::pin(stream),
            usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
        }
    }

    /// Wrap an outgoing stream with post-stream output guardrails (deterministic only).
    /// LLM-based output checks are not applied to streams in PR1 — they run only
    /// on non-streaming responses to avoid latency / mid-stream rewriting.
    fn wrap_stream_with_output_guardrails(
        &self,
        inner: LlmStreamResponse,
        progress: Option<ProgressSender>,
        metadata: Option<MetadataCell>,
    ) -> LlmStreamResponse {
        let guard_clone = match &self.output_guardrails {
            Some(g) => Some(Arc::clone(g)),
            None => return inner,
        };

        let usage = inner.usage.clone();
        let stream = async_stream::stream! {
            let mut inner_stream = inner.stream;
            let mut collected = String::new();

            while let Some(chunk) = inner_stream.next().await {
                if let Ok(text) = &chunk { collected.push_str(text) }
                yield chunk;
            }

            if let Some(ref guard) = guard_clone {
                let verdict = guard.check(&collected);
                let codes: Vec<&str> = verdict.violations.iter().map(|v| v.code.as_str()).collect();
                let passed = verdict.passed();
                let meta_violations = violations_to_meta(&verdict.violations);
                Self::update_metadata(&metadata, |m| {
                    m.output_guardrails_pass = Some(passed);
                    m.guardrail_violations.extend(meta_violations);
                });
                if !passed {
                    warn!(?codes, "Pipeline(stream): output guardrails flagged");
                    yield Ok::<_, thairag_core::error::ThaiRagError>(
                        "\n\n---\n⚠️ *Note: This response may have contained sensitive content \
                         that was flagged by your organization's policy.*".to_string()
                    );
                }
            }
            // Mark the synthetic stage as complete in progress events.
            if let Some(tx) = &progress {
                let _ = tx.send(PipelineProgress {
                    stage: "output_guardrails".to_string(),
                    status: StageStatus::Done,
                    duration_ms: None,
                    model: None,
                });
            }
        };

        LlmStreamResponse {
            stream: Box::pin(stream),
            usage,
        }
    }

    // ── Orchestrator ──

    async fn decide_route(&self, analysis: &QueryAnalysis) -> PipelineRoute {
        if let Some(ref orch) = self.pipeline_orchestrator {
            orch.decide(analysis).await
        } else {
            heuristic_decide(analysis)
        }
    }

    // ── Agent runners with fallback ──

    async fn run_analyzer(&self, query: &str, messages: &[ChatMessage]) -> Result<QueryAnalysis> {
        if let Some(ref analyzer) = self.query_analyzer {
            analyzer.analyze(query, messages).await
        } else {
            Ok(query_analyzer::fallback_analyze(query))
        }
    }

    /// Budget-aware analyzer: use heuristic fallback if budget exhausted.
    async fn run_analyzer_budgeted(
        &self,
        query: &str,
        messages: &[ChatMessage],
        budget: &LlmBudget,
    ) -> Result<QueryAnalysis> {
        if self.query_analyzer.is_some() && budget.try_spend() {
            self.run_analyzer(query, messages).await
        } else {
            Ok(query_analyzer::fallback_analyze(query))
        }
    }

    async fn run_rewriter(
        &self,
        query: &str,
        analysis: &QueryAnalysis,
    ) -> Result<RewrittenQueries> {
        if let Some(ref rewriter) = self.query_rewriter {
            rewriter.rewrite(query, analysis).await
        } else {
            Ok(query_rewriter::fallback_rewrite(query))
        }
    }

    /// Budget-aware rewriter: use heuristic fallback if budget exhausted.
    async fn run_rewriter_budgeted(
        &self,
        query: &str,
        analysis: &QueryAnalysis,
        budget: &LlmBudget,
    ) -> Result<RewrittenQueries> {
        if self.query_rewriter.is_some() && budget.try_spend() {
            self.run_rewriter(query, analysis).await
        } else {
            Ok(query_rewriter::fallback_rewrite(query))
        }
    }

    /// Budget-aware curator: use heuristic fallback if budget exhausted.
    async fn run_curator_budgeted(
        &self,
        query: &str,
        results: &[thairag_core::types::SearchResult],
        budget: &LlmBudget,
    ) -> Result<CuratedContext> {
        if self.context_curator.is_some() && budget.try_spend() {
            self.run_curator(query, results).await
        } else {
            Ok(context_curator::fallback_curate(
                results,
                self.config.max_context_tokens,
            ))
        }
    }

    /// Search using tool router (if enabled) or standard search.
    async fn run_search_with_tools(
        &self,
        rewritten: &RewrittenQueries,
        scope: &AccessScope,
        original_query: &str,
        available_scopes: &[SearchableScope],
    ) -> Result<Vec<thairag_core::types::SearchResult>> {
        // Feature 3: Agentic Tool Use
        if self.config.tool_use_enabled
            && let Some(ref router) = self.tool_router
        {
            return router
                .plan_and_execute(original_query, available_scopes, scope.is_unrestricted())
                .await;
        }

        // Standard search path
        self.run_search(rewritten, scope).await
    }

    async fn run_search(
        &self,
        rewritten: &RewrittenQueries,
        scope: &AccessScope,
    ) -> Result<Vec<thairag_core::types::SearchResult>> {
        let mut all_results = Vec::new();

        let primary_query = SearchQuery {
            text: rewritten.primary.clone(),
            top_k: 5,
            workspace_ids: scope.workspace_ids.clone(),
            unrestricted: scope.is_unrestricted(),
        };
        let mut results = self.search_engine.search(&primary_query).await?;
        all_results.append(&mut results);

        for sq in &rewritten.sub_queries {
            let sub_query = SearchQuery {
                text: sq.clone(),
                top_k: 3,
                workspace_ids: scope.workspace_ids.clone(),
                unrestricted: scope.is_unrestricted(),
            };
            if let Ok(mut r) = self.search_engine.search(&sub_query).await {
                all_results.append(&mut r);
            }
        }

        if let Some(ref hyde) = rewritten.hyde_query {
            let hyde_query = SearchQuery {
                text: hyde.clone(),
                top_k: 3,
                workspace_ids: scope.workspace_ids.clone(),
                unrestricted: scope.is_unrestricted(),
            };
            if let Ok(mut r) = self.search_engine.search(&hyde_query).await {
                all_results.append(&mut r);
            }
        }

        deduplicate_results(&mut all_results);
        Ok(all_results)
    }

    async fn run_curator(
        &self,
        query: &str,
        results: &[thairag_core::types::SearchResult],
    ) -> Result<CuratedContext> {
        let mut ctx = if let Some(ref curator) = self.context_curator {
            curator.curate(query, results).await?
        } else {
            context_curator::fallback_curate(results, self.config.max_context_tokens)
        };

        // Enrich chunks with document titles so the LLM knows which
        // document each chunk comes from (critical for counting/listing docs).
        if let Some(ref resolver) = self.doc_title_resolver {
            ctx.resolve_doc_titles(resolver.as_ref());
        }

        Ok(ctx)
    }

    /// CRAG: evaluate context and supplement/replace with web search if needed.
    async fn maybe_corrective_rag(
        &self,
        query: &str,
        context: CuratedContext,
    ) -> Result<CuratedContext> {
        let crag = match &self.corrective_rag {
            Some(c) if self.config.crag_enabled => c,
            _ => return Ok(context),
        };

        let action = crag.evaluate_context(query, &context).await?;
        match action {
            ContextAction::Correct => {
                debug!("CRAG: context is correct, proceeding");
                Ok(context)
            }
            ContextAction::Ambiguous | ContextAction::Incorrect => {
                if !crag.has_web_search() {
                    debug!(
                        "CRAG: context is {:?} but no web search configured",
                        if matches!(action, ContextAction::Ambiguous) {
                            "ambiguous"
                        } else {
                            "incorrect"
                        }
                    );
                    return Ok(context);
                }
                let web_results = crag.web_search(query).await?;
                if web_results.is_empty() {
                    return Ok(context);
                }
                let distilled = crag.distill_web_results(query, &web_results).await?;
                if distilled.is_empty() {
                    return Ok(context);
                }
                // Supplement the context with web content
                let mut enhanced = context;
                enhanced.chunks.push(crate::context_curator::CuratedChunk {
                    index: enhanced.chunks.len() + 1,
                    content: distilled,
                    relevance_score: 0.5,
                    source_doc_id: Default::default(),
                    source_chunk_id: Default::default(),
                    source_doc_title: Some("Web Search".to_string()),
                });
                info!(
                    web_results = web_results.len(),
                    "CRAG: supplemented with web search"
                );
                Ok(enhanced)
            }
        }
    }

    /// Run RAGAS evaluation asynchronously (non-blocking, sampled).
    async fn maybe_run_ragas(&self, query: &str, context: &CuratedContext, response: &str) {
        let evaluator = match &self.ragas_evaluator {
            Some(e) if self.config.ragas_enabled => e,
            _ => return,
        };

        if !evaluator.should_evaluate() {
            return;
        }

        let evaluator = Arc::clone(evaluator);
        let query = query.to_string();
        let context = context.clone();
        let response = response.to_string();

        // Fire-and-forget async evaluation
        tokio::spawn(async move {
            match evaluator.evaluate(&query, &context, &response).await {
                Ok(scores) => {
                    info!(
                        faithfulness = scores.faithfulness,
                        answer_relevancy = scores.answer_relevancy,
                        context_precision = scores.context_precision,
                        overall = scores.overall,
                        "RAGAS: evaluation scores"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "RAGAS: evaluation failed");
                }
            }
        });
    }

    /// Get the RAGAS evaluator handle (for external stats access).
    pub fn ragas_evaluator(&self) -> Option<&Arc<RagasEvaluator>> {
        self.ragas_evaluator.as_ref()
    }

    /// Get a reference to the conversation memory agent (for external summarization).
    pub fn conversation_memory(&self) -> Option<&ConversationMemory> {
        self.conversation_memory.as_ref()
    }

    /// Get the active learning handle (for external feedback recording).
    pub fn active_learning(&self) -> Option<&Arc<ActiveLearning>> {
        self.active_learning.as_ref()
    }
}

/// Deduplicate search results by chunk_id, keeping the highest score.
fn deduplicate_results(results: &mut Vec<thairag_core::types::SearchResult>) {
    use std::collections::HashMap;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut keep: Vec<thairag_core::types::SearchResult> = Vec::new();

    for r in results.iter() {
        let key = r.chunk.chunk_id.to_string();
        if let Some(&idx) = seen.get(&key) {
            if r.score > keep[idx].score {
                keep[idx] = r.clone();
            }
        } else {
            seen.insert(key, keep.len());
            keep.push(r.clone());
        }
    }

    keep.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    *results = keep;
}
