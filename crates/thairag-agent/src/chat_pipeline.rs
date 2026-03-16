use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use thairag_config::schema::ChatPipelineConfig;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::permission::AccessScope;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{
    ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, QueryIntent, SearchQuery,
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
use crate::language_adapter::LanguageAdapter;
use crate::map_reduce::MapReduceRag;
use crate::multimodal_rag::MultimodalRag;
use crate::pipeline_orchestrator::{PipelineOrchestrator, PipelineRoute, heuristic_decide};
use crate::quality_guard::QualityGuard;
use crate::query_analyzer::{self, QueryAnalysis, QueryAnalyzer};
use crate::query_rewriter::{self, QueryRewriter, RewrittenQueries};
use crate::ragas_eval::RagasEvaluator;
use crate::raptor::Raptor;
use crate::response_generator::ResponseGenerator;
use crate::self_rag::{RetrievalDecision, SelfRag};
use crate::speculative_rag::SpeculativeRag;
use crate::tool_router::{SearchableScope, ToolRouter};

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
        config: ChatPipelineConfig,
        prompts: Arc<PromptRegistry>,
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
            knowledge_graph: Arc::new(std::sync::RwLock::new(KnowledgeGraph::default())),
            main_llm,
            search_engine,
            config,
            adaptive_threshold: Arc::new(AtomicU32::new(threshold_bits)),
            prompts,
        }
    }

    /// Get the shared adaptive threshold handle (for external updates from feedback system).
    pub fn adaptive_threshold_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.adaptive_threshold)
    }

    /// Get the effective quality threshold (adaptive or configured).
    fn effective_threshold(&self) -> f32 {
        if self.config.adaptive_threshold_enabled {
            f32::from_bits(self.adaptive_threshold.load(Ordering::Relaxed))
        } else {
            self.config.quality_guard_threshold
        }
    }

    /// Non-streaming pipeline: orchestrator decides which agents to run.
    pub async fn process(
        &self,
        messages: &[ChatMessage],
        scope: &AccessScope,
        memories: &[MemoryEntry],
        available_scopes: &[SearchableScope],
    ) -> Result<LlmResponse> {
        // Inject memory context if available
        let full_messages = self.inject_memory(messages, memories);
        let messages = &full_messages;

        let user_query = messages.last().map(|m| m.content.as_str()).unwrap_or("");

        // ── Agent 1: Query Analyzer ──
        let analysis = self.run_analyzer(user_query, messages).await?;
        debug!(intent = ?analysis.intent, language = ?analysis.language, "Pipeline: analyzed");

        // ── Self-RAG gate: skip retrieval if not needed ──
        if let Some(ref self_rag) = self.self_rag
            && let Ok(RetrievalDecision::NoRetrieve { confidence }) =
                self_rag.should_retrieve(user_query, messages).await
        {
            info!(confidence, "Self-RAG: skipping retrieval");
            let response = self.main_llm.generate(messages, None).await?;
            self.maybe_run_ragas(user_query, &CuratedContext::default(), &response.content)
                .await;
            return self.maybe_adapt(response, &analysis).await;
        }

        // ── Orchestrator: decide route ──
        let route = self.decide_route(&analysis).await;
        info!(route = ?route, "Pipeline: orchestrator decided");

        match route {
            PipelineRoute::DirectLlm => match analysis.intent {
                QueryIntent::Clarification => Ok(LlmResponse {
                    content: "Could you please provide more details about your question?"
                        .into(),
                    usage: LlmUsage::default(),
                }),
                _ => self.main_llm.generate(messages, None).await,
            },
            PipelineRoute::SimpleRetrieval => {
                let rewritten = query_rewriter::fallback_rewrite(user_query);
                let results = self
                    .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
                    .await?;
                debug!(results = results.len(), "Pipeline(simple): searched");
                let context = self.run_curator(user_query, &results).await?;

                // Retrieval refinement
                let context = self
                    .maybe_refine_retrieval(user_query, &analysis, scope, context, available_scopes)
                    .await?;

                let response = self
                    .response_generator
                    .generate(&analysis, &context, messages, None)
                    .await?;
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
                )
                .await
            }
        }
    }

    /// Execute the full pipeline (agents 2-6).
    async fn execute_full(
        &self,
        user_query: &str,
        messages: &[ChatMessage],
        scope: &AccessScope,
        analysis: &QueryAnalysis,
        force_quality_guard: bool,
        available_scopes: &[SearchableScope],
    ) -> Result<LlmResponse> {
        // ── Agent 2: Query Rewriter ──
        let rewritten = self.run_rewriter(user_query, analysis).await?;
        debug!(primary = %rewritten.primary, sub = rewritten.sub_queries.len(), "Pipeline: rewritten");

        // ── Search (with tool router if enabled) ──
        let mut results = self
            .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
            .await?;
        debug!(results = results.len(), "Pipeline: searched");

        // ── ColBERT reranking: fine-grained LLM-based reranking ──
        if let Some(ref colbert) = self.colbert_reranker {
            results = colbert.rerank(user_query, &results).await?;
            debug!(results = results.len(), "Pipeline: ColBERT reranked");
        }

        // ── Active Learning: adjust scores from feedback history ──
        if let Some(ref al) = self.active_learning {
            al.adjust_scores(&mut results);
        }

        // ── Graph RAG: enhance with entity relationships ──
        if let Some(ref graph_rag) = self.graph_rag {
            let graph = self.knowledge_graph.read().unwrap().clone();
            if graph.entity_count() > 0 {
                results = graph_rag
                    .enhance_results(user_query, &results, &graph)
                    .await?;
                debug!(results = results.len(), "Pipeline: graph-enhanced");
            }
            // Extract entities from results to grow the graph
            if !results.is_empty() {
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
        }

        // ── Agent 3: Context Curator ──
        let context = self.run_curator(user_query, &results).await?;
        debug!(
            chunks = context.chunks.len(),
            tokens = context.total_tokens_est,
            "Pipeline: curated"
        );

        // ── Retrieval Refinement ──
        let context = self
            .maybe_refine_retrieval(user_query, analysis, scope, context, available_scopes)
            .await?;

        // ── CRAG: check context quality, fall back to web if needed ──
        let context = self.maybe_corrective_rag(user_query, context).await?;

        // ── RAPTOR: build hierarchical summary tree ──
        let context = if let Some(ref raptor) = self.raptor {
            raptor.build_tree(user_query, &context).await?
        } else {
            context
        };

        // ── Contextual Compression: reduce context size ──
        let context = if let Some(ref compressor) = self.contextual_compression {
            compressor.compress(user_query, &context).await?
        } else {
            context
        };

        // ── Multi-modal RAG: enrich with image descriptions ──
        let context = if let Some(ref mm) = self.multimodal_rag {
            mm.enrich_context(user_query, &context).await?
        } else {
            context
        };

        // ── Map-Reduce: for complex synthesis queries with many results ──
        if let Some(ref mr) = self.map_reduce && mr.should_use(analysis, &results) {
            info!("Pipeline: using map-reduce for synthesis query");
            let response = mr.process(user_query, &results).await?;
            self.maybe_run_ragas(user_query, &context, &response.content)
                .await;
            return self.maybe_adapt(response, analysis).await;
        }

        // ── Agent 4: Response Generator (or Speculative RAG) ──
        let mut response = if let Some(ref spec) = self.speculative_rag {
            info!("Pipeline: using speculative generation");
            spec.speculative_generate(analysis, &context, messages, user_query)
                .await?
        } else {
            self.response_generator
                .generate(analysis, &context, messages, None)
                .await?
        };
        debug!(len = response.content.len(), "Pipeline: generated");

        // ── Agent 5: Quality Guard (with retry loop) ──
        let threshold = self.effective_threshold();
        let run_guard = force_quality_guard || self.quality_guard.is_some();
        if run_guard && let Some(ref guard) = self.quality_guard {
            for attempt in 0..=self.config.quality_guard_max_retries {
                let verdict = guard
                    .check_with_threshold(user_query, &response.content, &context, threshold)
                    .await?;
                if verdict.pass {
                    debug!(attempt, "Pipeline: quality passed");
                    break;
                }
                if attempt < self.config.quality_guard_max_retries {
                    let feedback = verdict.feedback.unwrap_or_else(|| {
                        "Improve relevance and reduce hallucination.".into()
                    });
                    warn!(attempt, feedback = %feedback, "Pipeline: quality failed, retrying");
                    response = self
                        .response_generator
                        .generate_with_feedback(analysis, &context, messages, &feedback, None)
                        .await?;
                } else {
                    warn!("Pipeline: quality guard exhausted retries, using last response");
                }
            }
        }

        // ── RAGAS evaluation (async, sampled) ──
        self.maybe_run_ragas(user_query, &context, &response.content)
            .await;

        // ── Agent 6: Language Adapter ──
        let response = self.maybe_adapt(response, analysis).await?;

        info!("Pipeline: complete");
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
        for attempt in 0..self.config.refinement_max_retries {
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
    ) -> Result<LlmStreamResponse> {
        let full_messages = self.inject_memory(messages, memories);
        let messages = &full_messages;

        let user_query = messages.last().map(|m| m.content.as_str()).unwrap_or("");

        // ── Agent 1: Query Analyzer ──
        let analysis = self.run_analyzer(user_query, messages).await?;

        // ── Self-RAG gate (streaming) ──
        if let Some(ref self_rag) = self.self_rag
            && let Ok(RetrievalDecision::NoRetrieve { confidence }) =
                self_rag.should_retrieve(user_query, messages).await
        {
            info!(confidence, "Self-RAG(stream): skipping retrieval");
            return self.main_llm.generate_stream(messages, None).await;
        }

        // ── Orchestrator: decide route ──
        let route = self.decide_route(&analysis).await;
        debug!(route = ?route, "Pipeline(stream): orchestrator decided");

        match route {
            PipelineRoute::DirectLlm => match analysis.intent {
                QueryIntent::Clarification => {
                    let msg =
                        "Could you please provide more details about your question?".into();
                    Ok(LlmStreamResponse {
                        stream: Box::pin(tokio_stream::once(Ok(msg))),
                        usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
                    })
                }
                _ => self.main_llm.generate_stream(messages, None).await,
            },
            PipelineRoute::SimpleRetrieval => {
                let rewritten = query_rewriter::fallback_rewrite(user_query);
                let results = self
                    .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
                    .await?;
                let context = self.run_curator(user_query, &results).await?;
                let context = self
                    .maybe_refine_retrieval(user_query, &analysis, scope, context, available_scopes)
                    .await?;
                if let Some(resp) = self.context_insufficient_response(&context) {
                    return Ok(resp);
                }
                let stream = self
                    .response_generator
                    .generate_stream(&analysis, &context, messages, None)
                    .await?;
                Ok(self.wrap_stream_with_quality_guard(stream, user_query, context))
            }
            PipelineRoute::FullPipeline | PipelineRoute::ComplexPipeline => {
                let rewritten = self.run_rewriter(user_query, &analysis).await?;
                let results = self
                    .run_search_with_tools(&rewritten, scope, user_query, available_scopes)
                    .await?;
                let context = self.run_curator(user_query, &results).await?;
                let context = self
                    .maybe_refine_retrieval(user_query, &analysis, scope, context, available_scopes)
                    .await?;
                if let Some(resp) = self.context_insufficient_response(&context) {
                    return Ok(resp);
                }
                let stream = self
                    .response_generator
                    .generate_stream(&analysis, &context, messages, None)
                    .await?;
                Ok(self.wrap_stream_with_quality_guard(stream, user_query, context))
            }
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

    /// Search using tool router (if enabled) or standard search.
    async fn run_search_with_tools(
        &self,
        rewritten: &RewrittenQueries,
        scope: &AccessScope,
        original_query: &str,
        available_scopes: &[SearchableScope],
    ) -> Result<Vec<thairag_core::types::SearchResult>> {
        // Feature 3: Agentic Tool Use
        if self.config.tool_use_enabled && let Some(ref router) = self.tool_router {
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
        if let Some(ref curator) = self.context_curator {
            curator.curate(query, results).await
        } else {
            Ok(context_curator::fallback_curate(
                results,
                self.config.max_context_tokens,
            ))
        }
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
