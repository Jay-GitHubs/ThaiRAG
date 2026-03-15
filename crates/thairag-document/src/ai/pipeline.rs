use std::sync::Arc;

use thairag_config::schema::{AiPreprocessingConfig, AiRetryConfig};
use thairag_core::error::Result;
use thairag_core::traits::{Chunker, LlmProvider, QualityChecker, SmartChunker};
use thairag_core::types::{
    ChunkId, ChunkMetadata, ContentType, DocId, DocumentAnalysis, DocumentChunk,
    OrchestratorAction, PipelineSnapshot, StructureLevel, WorkspaceId,
};
use thairag_core::PromptRegistry;
use tracing::{info, warn};

use crate::pipeline::StepCallback;

use crate::chunker::MarkdownChunker;
use crate::converter::MarkdownConverter;

use super::analyzer::LlmDocumentAnalyzer;
use super::chunker::{validate_chunks, LlmSmartChunker};
use super::converter::LlmDocumentConverter;
use super::enricher::LlmChunkEnricher;
use super::orchestrator::LlmOrchestrator;
use super::quality::LlmQualityChecker;

/// AI-powered document preprocessing pipeline.
///
/// Orchestrates 5 agents: Analyzer → Converter → Quality Checker → Smart Chunker,
/// with an Orchestrator agent that makes adaptive decisions between steps.
///
/// When `orchestrator_enabled` is false, uses deterministic retry logic (AiRetryConfig).
/// When true, the Orchestrator LLM decides retry/accept/fallback/flag after each step.
pub struct AiDocumentPipeline {
    analyzer: LlmDocumentAnalyzer,
    converter: LlmDocumentConverter,
    quality_checker: LlmQualityChecker,
    smart_chunker: LlmSmartChunker,
    enricher: Option<LlmChunkEnricher>,
    orchestrator: Option<LlmOrchestrator>,
    // Mechanical fallbacks
    mechanical_converter: MarkdownConverter,
    mechanical_chunker: MarkdownChunker,
    // Config — None means "auto" (use AI-recommended value)
    min_ai_size_bytes: usize,
    max_chunk_size: usize,
    chunk_overlap: usize,
    quality_threshold_override: Option<f32>,
    max_chunk_size_override: Option<usize>,
    min_ai_size_override: Option<usize>,
    // Retry config (used when orchestrator is disabled)
    retry: AiRetryConfig,
    // Orchestrator config
    auto_orchestrator_budget: bool,
    max_orchestrator_calls: u32,
}

/// Mutable state tracked during a single pipeline run.
struct PipelineState {
    orchestrator_calls: u32,
    /// Dynamic budget — computed after analysis based on doc complexity.
    /// Capped by `max_orchestrator_calls` from config.
    budget: u32,
    decisions: Vec<String>,
    flagged_for_review: bool,
    effective_quality_threshold: f32,
    effective_max_chunk_size: usize,
}

impl AiDocumentPipeline {
    /// Create from a shared LLM provider and config.
    /// Each agent uses the shared LLM.
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        config: &AiPreprocessingConfig,
        max_chunk_size: usize,
        chunk_overlap: usize,
    ) -> Self {
        Self::new_per_agent(
            Arc::clone(&llm),
            Arc::clone(&llm),
            Arc::clone(&llm),
            Arc::clone(&llm),
            if config.enricher_enabled {
                Some(Arc::clone(&llm))
            } else {
                None
            },
            if config.orchestrator_enabled {
                Some(Arc::clone(&llm))
            } else {
                None
            },
            config,
            max_chunk_size,
            chunk_overlap,
        )
    }

    /// Create with separate LLM providers per agent for optimal resource usage.
    pub fn new_per_agent(
        analyzer_llm: Arc<dyn LlmProvider>,
        converter_llm: Arc<dyn LlmProvider>,
        quality_llm: Arc<dyn LlmProvider>,
        chunker_llm: Arc<dyn LlmProvider>,
        enricher_llm: Option<Arc<dyn LlmProvider>>,
        orchestrator_llm: Option<Arc<dyn LlmProvider>>,
        config: &AiPreprocessingConfig,
        max_chunk_size: usize,
        chunk_overlap: usize,
    ) -> Self {
        Self::new_per_agent_with_prompts(
            analyzer_llm, converter_llm, quality_llm, chunker_llm,
            enricher_llm, orchestrator_llm, config, max_chunk_size, chunk_overlap,
            Arc::new(PromptRegistry::new()),
        )
    }

    /// Create with separate LLM providers per agent and shared prompt registry.
    pub fn new_per_agent_with_prompts(
        analyzer_llm: Arc<dyn LlmProvider>,
        converter_llm: Arc<dyn LlmProvider>,
        quality_llm: Arc<dyn LlmProvider>,
        chunker_llm: Arc<dyn LlmProvider>,
        enricher_llm: Option<Arc<dyn LlmProvider>>,
        orchestrator_llm: Option<Arc<dyn LlmProvider>>,
        config: &AiPreprocessingConfig,
        max_chunk_size: usize,
        chunk_overlap: usize,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        // When auto_params is on, these become overrides (None = let AI decide)
        let (quality_threshold_override, max_chunk_size_override, min_ai_size_override) =
            if config.auto_params {
                (None, None, None)
            } else {
                (
                    Some(config.quality_threshold),
                    Some(max_chunk_size),
                    Some(config.min_ai_size_bytes),
                )
            };

        // Resolve per-agent max_tokens: agent LLM config → shared LLM config → global
        let resolve_mt = |agent_llm: &Option<thairag_config::schema::LlmConfig>| -> u32 {
            agent_llm.as_ref()
                .and_then(|c| c.max_tokens)
                .or_else(|| config.llm.as_ref().and_then(|c| c.max_tokens))
                .unwrap_or(config.agent_max_tokens)
        };
        let analyzer_mt = resolve_mt(&config.analyzer_llm);
        let converter_mt = resolve_mt(&config.converter_llm);
        let quality_mt = resolve_mt(&config.quality_llm);
        let chunker_mt = resolve_mt(&config.chunker_llm);
        let enricher_mt = resolve_mt(&config.enricher_llm);
        let orchestrator_mt = resolve_mt(&config.orchestrator_llm);

        Self {
            analyzer: LlmDocumentAnalyzer::new_with_prompts(analyzer_llm, analyzer_mt, Arc::clone(&prompts)),
            converter: LlmDocumentConverter::new_with_prompts(
                converter_llm,
                config.max_llm_input_chars,
                converter_mt,
                Arc::clone(&prompts),
            ),
            quality_checker: LlmQualityChecker::new_with_prompts(
                quality_llm,
                config.quality_threshold,
                quality_mt,
                Arc::clone(&prompts),
            ),
            smart_chunker: LlmSmartChunker::new_with_prompts(chunker_llm, chunker_mt, Arc::clone(&prompts)),
            enricher: enricher_llm.map(|llm| LlmChunkEnricher::new_with_prompts(llm, enricher_mt, Arc::clone(&prompts))),
            orchestrator: orchestrator_llm
                .map(|llm| LlmOrchestrator::new_with_prompts(llm, orchestrator_mt, Arc::clone(&prompts))),
            mechanical_converter: MarkdownConverter::new(),
            mechanical_chunker: MarkdownChunker::new(),
            min_ai_size_bytes: config.min_ai_size_bytes,
            max_chunk_size,
            chunk_overlap,
            quality_threshold_override,
            max_chunk_size_override,
            min_ai_size_override,
            retry: config.retry.clone(),
            auto_orchestrator_budget: config.auto_orchestrator_budget,
            max_orchestrator_calls: config.max_orchestrator_calls,
        }
    }

    /// Report a step via the callback, if present.
    fn report_step(on_step: &Option<StepCallback>, step: &str) {
        if let Some(cb) = on_step {
            cb(step);
        }
    }

    /// Process a document through the AI agent team.
    pub async fn process(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: Option<StepCallback>,
    ) -> Result<Vec<DocumentChunk>> {
        // Step 0: Extract text
        let pages = self.mechanical_converter.convert_by_pages(raw, mime_type)?;
        let is_multipage = pages.len() > 1;

        if pages.is_empty() {
            info!(%doc_id, "Document has no extractable text");
            return Ok(vec![]);
        }

        let raw_text: String = pages
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let total_size: usize = pages.iter().map(|(_, t)| t.len()).sum();

        let pre_min_size = self.min_ai_size_override.unwrap_or(self.min_ai_size_bytes);
        if total_size < pre_min_size {
            info!(%doc_id, size = total_size, min = pre_min_size, "Document too small for AI, using mechanical pipeline");
            return self.mechanical_fallback(&raw_text, doc_id, workspace_id);
        }

        // Check if vision path is available for OCR documents
        let use_vision = self.converter.supports_vision();

        info!(%doc_id, pages = pages.len(), total_size, is_multipage,
            orchestrator = self.orchestrator.is_some(),
            vision = use_vision,
            "Starting AI preprocessing");

        if self.orchestrator.is_some() {
            self.process_orchestrated(
                &pages, &raw_text, total_size, mime_type, is_multipage,
                if use_vision { Some(raw) } else { None },
                doc_id, workspace_id, &on_step,
            )
            .await
        } else {
            self.process_retry_based(
                &pages, &raw_text, total_size, mime_type, is_multipage,
                if use_vision { Some(raw) } else { None },
                doc_id, workspace_id, &on_step,
            )
            .await
        }
    }

    // ── Orchestrator-driven flow ────────────────────────────────────────

    async fn process_orchestrated(
        &self,
        pages: &[(usize, String)],
        raw_text: &str,
        total_size: usize,
        mime_type: &str,
        is_multipage: bool,
        raw_bytes: Option<&[u8]>,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: &Option<StepCallback>,
    ) -> Result<Vec<DocumentChunk>> {
        let orchestrator = self.orchestrator.as_ref().unwrap();

        let effective_quality_threshold = self
            .quality_threshold_override
            .unwrap_or(0.7);
        let effective_max_chunk_size = self
            .max_chunk_size_override
            .unwrap_or(self.max_chunk_size);

        let mut state = PipelineState {
            orchestrator_calls: 0,
            // Start with user's cap; will be refined after analysis
            budget: self.max_orchestrator_calls,
            decisions: Vec::new(),
            flagged_for_review: false,
            effective_quality_threshold,
            effective_max_chunk_size,
        };

        // ── Step 1: Analyze ─────────────────────────────────────────────
        Self::report_step(on_step, "analyzing");
        let use_vision_analyzer = raw_bytes.is_some() && self.analyzer.supports_vision();
        if use_vision_analyzer {
            info!(%doc_id, "AI Agent: analyzing document with vision");
        } else {
            info!(%doc_id, "AI Agent: analyzing document");
        }

        let mut excerpt_size = self.analyzer.default_excerpt_chars();
        let analysis = loop {
            let a = if use_vision_analyzer {
                match self
                    .analyzer
                    .analyze_with_vision(raw_bytes.unwrap(), mime_type, raw_text, total_size)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Vision analysis failed, falling back to text");
                        match self.analyzer.analyze_with_excerpt_size(raw_text, mime_type, total_size, excerpt_size).await {
                            Ok(a) => a,
                            Err(e2) => {
                                warn!(%doc_id, error = %e2, "Text analysis also failed, falling back to mechanical");
                                return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                            }
                        }
                    }
                }
            } else {
                match self
                    .analyzer
                    .analyze_with_excerpt_size(raw_text, mime_type, total_size, excerpt_size)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Analysis failed, falling back to mechanical");
                        return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                    }
                }
            };

            info!(
                %doc_id, confidence = a.confidence,
                language = %a.primary_language,
                content_type = ?a.content_type,
                "Analysis result"
            );

            // Ask orchestrator
            let snapshot = PipelineSnapshot {
                completed_stage: "analyzer".into(),
                analysis_confidence: Some(a.confidence),
                analysis_language: Some(a.primary_language.clone()),
                analysis_content_type: Some(format!("{:?}", a.content_type)),
                quality_overall: None,
                quality_issues: None,
                chunk_count: None,
                chunk_issues: None,
                orchestrator_call_count: state.orchestrator_calls,
                max_orchestrator_calls: state.budget,
                decision_history: state.decisions.clone(),
                effective_quality_threshold: state.effective_quality_threshold,
                effective_max_chunk_size: state.effective_max_chunk_size,
                doc_size_bytes: total_size,
                mime_type: mime_type.to_string(),
                needs_ocr_correction: Some(a.needs_ocr_correction),
            };

            let decision = if state.orchestrator_calls < state.budget {
                Self::report_step(on_step, "orchestrator_reviewing_analysis");
                state.orchestrator_calls += 1;
                orchestrator.decide(&snapshot).await?
            } else {
                // Budget exhausted — auto-accept if confidence > 0.3
                if a.confidence > 0.3 {
                    thairag_core::types::OrchestratorDecision {
                        action: OrchestratorAction::Accept,
                        reasoning: "Budget exhausted, auto-accepting".into(),
                        confidence: a.confidence,
                    }
                } else {
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }
            };

            let decision_summary = format!("analyzer: {} ({})", action_name(&decision.action), decision.reasoning);
            info!(%doc_id, decision = %decision_summary, "Orchestrator decision");
            state.decisions.push(decision_summary);

            match decision.action {
                OrchestratorAction::Accept => {
                    // Update effective params from AI recommendations
                    if let Some(qt) = a.recommended_quality_threshold {
                        if self.quality_threshold_override.is_none() {
                            state.effective_quality_threshold = qt;
                        }
                    }
                    if let Some(mcs) = a.recommended_max_chunk_size {
                        if self.max_chunk_size_override.is_none() {
                            state.effective_max_chunk_size = mcs;
                        }
                    }
                    break a;
                }
                OrchestratorAction::Retry { params, .. } => {
                    if let Some(p) = params {
                        if let Some(es) = p.excerpt_size {
                            excerpt_size = es;
                        }
                    } else {
                        excerpt_size *= 2;
                    }
                    Self::report_step(on_step, "retrying_analysis");
                    continue;
                }
                OrchestratorAction::AdjustParams { params } => {
                    if let Some(qt) = params.quality_threshold {
                        state.effective_quality_threshold = qt;
                    }
                    if let Some(mcs) = params.max_chunk_size {
                        state.effective_max_chunk_size = mcs;
                    }
                    break a;
                }
                OrchestratorAction::FallbackMechanical { reason } => {
                    warn!(%doc_id, reason, "Orchestrator chose mechanical fallback after analysis");
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }
                OrchestratorAction::FlagForReview { reason } => {
                    warn!(%doc_id, reason, "Orchestrator flagged for review after analysis");
                    state.flagged_for_review = true;
                    break a;
                }
                OrchestratorAction::Skip { .. } => {
                    // Can't skip analysis — accept whatever we got
                    break a;
                }
            }
        };

        // Compute orchestrator budget: dynamic (based on doc complexity) or fixed
        let budget = if self.auto_orchestrator_budget {
            // Auto mode: formula is the authority, no user-configured ceiling
            compute_orchestrator_budget(&analysis, is_multipage)
        } else {
            self.max_orchestrator_calls
        };
        state.budget = budget;

        info!(
            %doc_id,
            quality_threshold = state.effective_quality_threshold,
            max_chunk_size = state.effective_max_chunk_size,
            orchestrator_budget = budget,
            auto_budget = self.auto_orchestrator_budget,
            "Effective processing parameters"
        );

        // ── Step 2+3: Convert → Quality Check ───────────────────────────
        Self::report_step(on_step, "converting");
        let mut converted = if analysis.needs_ocr_correction && raw_bytes.is_some() {
            // Vision path: send actual document to vision model
            Self::report_step(on_step, "converting_with_vision");
            info!(%doc_id, "AI Agent: converting with vision model (OCR document)");
            match self.do_convert_vision(raw_bytes.unwrap(), mime_type, raw_text, &analysis, doc_id).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(%doc_id, error = %e, "Vision conversion failed, falling back to text");
                    self.do_convert(pages, raw_text, is_multipage, &analysis, doc_id).await?
                }
            }
        } else {
            self.do_convert(pages, raw_text, is_multipage, &analysis, doc_id).await?
        };

        let use_vision_quality = raw_bytes.is_some() && self.quality_checker.supports_vision();
        let quality = loop {
            Self::report_step(on_step, "checking_quality");

            let mut q = if use_vision_quality {
                info!(%doc_id, "AI Agent: checking quality with vision");
                match self.quality_checker.check_with_vision(raw_bytes.unwrap(), mime_type, raw_text, &converted).await {
                    Ok(q) => q,
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Vision quality check failed, trying text-only");
                        match self.quality_checker.check(raw_text, &converted).await {
                            Ok(q) => q,
                            Err(e2) => {
                                warn!(%doc_id, error = %e2, "Text quality check also failed, falling back");
                                return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                            }
                        }
                    }
                }
            } else {
                info!(%doc_id, "AI Agent: checking conversion quality");
                match self.quality_checker.check(raw_text, &converted).await {
                    Ok(q) => q,
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Quality check failed, falling back");
                        return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                    }
                }
            };
            q.passed = q.overall_score >= state.effective_quality_threshold;

            // Ask orchestrator
            let snapshot = PipelineSnapshot {
                completed_stage: "quality_checker".into(),
                analysis_confidence: Some(analysis.confidence),
                analysis_language: Some(analysis.primary_language.clone()),
                analysis_content_type: Some(format!("{:?}", analysis.content_type)),
                quality_overall: Some(q.overall_score),
                quality_issues: Some(q.issues.clone()),
                chunk_count: None,
                chunk_issues: None,
                orchestrator_call_count: state.orchestrator_calls,
                max_orchestrator_calls: state.budget,
                decision_history: state.decisions.clone(),
                effective_quality_threshold: state.effective_quality_threshold,
                effective_max_chunk_size: state.effective_max_chunk_size,
                doc_size_bytes: total_size,
                mime_type: mime_type.to_string(),
                needs_ocr_correction: Some(analysis.needs_ocr_correction),
            };

            let decision = if state.orchestrator_calls < state.budget {
                Self::report_step(on_step, "orchestrator_reviewing_quality");
                state.orchestrator_calls += 1;
                orchestrator.decide(&snapshot).await?
            } else {
                // Budget exhausted — accept if passed, fallback otherwise
                if q.passed {
                    thairag_core::types::OrchestratorDecision {
                        action: OrchestratorAction::Accept,
                        reasoning: "Budget exhausted, quality passed".into(),
                        confidence: q.overall_score,
                    }
                } else {
                    warn!(%doc_id, score = q.overall_score, "Budget exhausted and quality failed, falling back");
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }
            };

            let decision_summary = format!(
                "quality(score={:.2}): {} ({})",
                q.overall_score, action_name(&decision.action), decision.reasoning
            );
            info!(%doc_id, decision = %decision_summary, "Orchestrator decision");
            state.decisions.push(decision_summary);

            match decision.action {
                OrchestratorAction::Accept => break q,
                OrchestratorAction::Retry { .. } => {
                    Self::report_step(on_step, "retrying_conversion");
                    info!(%doc_id, "Orchestrator: retrying conversion with feedback");
                    converted = self
                        .do_convert_with_feedback(
                            pages,
                            raw_text,
                            is_multipage,
                            &analysis,
                            &converted.markdown,
                            &q.issues,
                            doc_id,
                        )
                        .await?;
                    continue;
                }
                OrchestratorAction::AdjustParams { params } => {
                    if let Some(qt) = params.quality_threshold {
                        info!(%doc_id, old = state.effective_quality_threshold, new = qt,
                            "Orchestrator adjusted quality threshold");
                        state.effective_quality_threshold = qt;
                    }
                    if let Some(mcs) = params.max_chunk_size {
                        state.effective_max_chunk_size = mcs;
                    }
                    // Re-evaluate with new threshold
                    q.passed = q.overall_score >= state.effective_quality_threshold;
                    if q.passed {
                        break q;
                    }
                    // Still failing with new threshold — loop will re-check
                    continue;
                }
                OrchestratorAction::FallbackMechanical { reason } => {
                    warn!(%doc_id, reason, "Orchestrator chose mechanical fallback after quality check");
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }
                OrchestratorAction::FlagForReview { reason } => {
                    warn!(%doc_id, reason, "Orchestrator flagged for review after quality check");
                    state.flagged_for_review = true;
                    break q;
                }
                OrchestratorAction::Skip { .. } => break q,
            }
        };

        // ── Step 4: Smart Chunk ─────────────────────────────────────────
        Self::report_step(on_step, "chunking");
        info!(%doc_id, max_chunk_size = state.effective_max_chunk_size, "AI Agent: semantic chunking");

        let mut chunks: Vec<thairag_core::types::EnrichedChunk> = match self
            .smart_chunker
            .chunk(&converted, state.effective_max_chunk_size)
            .await
        {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => {
                warn!(%doc_id, "Smart chunker returned empty, falling back");
                return self.mechanical_fallback(raw_text, doc_id, workspace_id);
            }
            Err(e) => {
                warn!(%doc_id, error = %e, "Smart chunking failed, falling back");
                return self.mechanical_fallback(raw_text, doc_id, workspace_id);
            }
        };

        // Validate chunks and consult orchestrator if issues
        let chunk_issues =
            validate_chunks(&chunks, &converted.markdown, state.effective_max_chunk_size);

        if !chunk_issues.is_empty() {
            let snapshot = PipelineSnapshot {
                completed_stage: "chunker".into(),
                analysis_confidence: Some(analysis.confidence),
                analysis_language: Some(analysis.primary_language.clone()),
                analysis_content_type: Some(format!("{:?}", analysis.content_type)),
                quality_overall: Some(quality.overall_score),
                quality_issues: None,
                chunk_count: Some(chunks.len()),
                chunk_issues: Some(chunk_issues.clone()),
                orchestrator_call_count: state.orchestrator_calls,
                max_orchestrator_calls: state.budget,
                decision_history: state.decisions.clone(),
                effective_quality_threshold: state.effective_quality_threshold,
                effective_max_chunk_size: state.effective_max_chunk_size,
                doc_size_bytes: total_size,
                mime_type: mime_type.to_string(),
                needs_ocr_correction: Some(analysis.needs_ocr_correction),
            };

            if state.orchestrator_calls < state.budget {
                Self::report_step(on_step, "orchestrator_reviewing_chunks");
                state.orchestrator_calls += 1;
                let decision = orchestrator.decide(&snapshot).await?;

                let decision_summary = format!(
                    "chunker(chunks={},issues={}): {} ({})",
                    chunks.len(),
                    chunk_issues.len(),
                    action_name(&decision.action),
                    decision.reasoning
                );
                info!(%doc_id, decision = %decision_summary, "Orchestrator decision");
                state.decisions.push(decision_summary);

                match decision.action {
                    OrchestratorAction::Retry { .. } => {
                        Self::report_step(on_step, "retrying_chunking");
                        info!(%doc_id, "Orchestrator: retrying chunker with feedback");
                        match self
                            .smart_chunker
                            .chunk_with_feedback(
                                &converted,
                                state.effective_max_chunk_size,
                                &chunk_issues,
                            )
                            .await
                        {
                            Ok(c) if !c.is_empty() => chunks = c,
                            Ok(_) => warn!(%doc_id, "Chunker retry returned empty, keeping previous"),
                            Err(e) => warn!(%doc_id, error = %e, "Chunker retry failed, keeping previous"),
                        }
                    }
                    OrchestratorAction::FlagForReview { reason } => {
                        warn!(%doc_id, reason, "Orchestrator flagged for review after chunking");
                        state.flagged_for_review = true;
                    }
                    OrchestratorAction::FallbackMechanical { reason } => {
                        warn!(%doc_id, reason, "Orchestrator chose mechanical fallback after chunking");
                        return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                    }
                    _ => {} // Accept, Skip, AdjustParams → keep current chunks
                }
            }
        }

        // ── Step 5: Enrich chunks ──────────────────────────────────────
        let mut doc_chunks: Vec<DocumentChunk> = chunks
            .into_iter()
            .enumerate()
            .map(|(i, ec)| DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id,
                workspace_id,
                content: ec.content,
                chunk_index: i,
                embedding: None,
                metadata: Some(ChunkMetadata {
                    topic: ec.topic,
                    section_title: ec.section_title,
                    language: ec.language,
                    chunk_type: if state.flagged_for_review {
                        Some("flagged_for_review".to_string())
                    } else {
                        ec.chunk_type
                    },
                    quality_score: Some(quality.overall_score),
                    page_numbers: ec.page_numbers,
                    ..Default::default()
                }),
            })
            .collect();

        if let Some(ref enricher) = self.enricher {
            Self::report_step(on_step, "enriching");
            info!(%doc_id, chunks = doc_chunks.len(), "AI Agent: enriching chunks for search");
            let title = ""; // Title not available at this level; enricher handles gracefully
            if let Err(e) = enricher.enrich(&mut doc_chunks, &analysis, title).await {
                warn!(%doc_id, error = %e, "Chunk enrichment failed, continuing without enrichment");
            }
        }

        info!(
            %doc_id,
            chunks = doc_chunks.len(),
            flagged = state.flagged_for_review,
            orchestrator_calls = state.orchestrator_calls,
            "AI preprocessing complete"
        );

        Ok(doc_chunks)
    }

    // ── Deterministic retry-based flow (original logic) ─────────────────

    async fn process_retry_based(
        &self,
        pages: &[(usize, String)],
        raw_text: &str,
        total_size: usize,
        mime_type: &str,
        is_multipage: bool,
        raw_bytes: Option<&[u8]>,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: &Option<StepCallback>,
    ) -> Result<Vec<DocumentChunk>> {
        // ── Step 1: Analyze (with confidence retry) ──────────────────────
        Self::report_step(on_step, "analyzing");
        let use_vision_analyzer = raw_bytes.is_some() && self.analyzer.supports_vision();
        if use_vision_analyzer {
            info!(%doc_id, "AI Agent: analyzing document with vision");
        } else {
            info!(%doc_id, "AI Agent: analyzing document");
        }

        let analysis = {
            let max_retries = if self.retry.enabled {
                self.retry.analyzer_max_retries
            } else {
                0
            };
            let mut excerpt_size = self.analyzer.default_excerpt_chars();
            let mut result = None;
            // Try vision first on the first attempt
            let mut tried_vision = false;

            for attempt in 0..=max_retries {
                let analyze_result = if use_vision_analyzer && !tried_vision {
                    tried_vision = true;
                    match self.analyzer.analyze_with_vision(raw_bytes.unwrap(), mime_type, raw_text, total_size).await {
                        Ok(a) => Ok(a),
                        Err(e) => {
                            warn!(%doc_id, error = %e, "Vision analysis failed, falling back to text");
                            self.analyzer.analyze_with_excerpt_size(raw_text, mime_type, total_size, excerpt_size).await
                        }
                    }
                } else {
                    self.analyzer.analyze_with_excerpt_size(raw_text, mime_type, total_size, excerpt_size).await
                };

                match analyze_result {
                    Ok(a)
                        if a.confidence > self.retry.analyzer_retry_below_confidence
                            || !self.retry.enabled =>
                    {
                        if a.confidence > 0.3 {
                            info!(
                                %doc_id, confidence = a.confidence,
                                language = %a.primary_language,
                                content_type = ?a.content_type,
                                structure = ?a.structure_level,
                                rec_quality = ?a.recommended_quality_threshold,
                                rec_chunk_size = ?a.recommended_max_chunk_size,
                                "Analysis complete"
                            );
                            result = Some(a);
                            break;
                        }
                        warn!(%doc_id, confidence = a.confidence, "Analysis confidence too low, falling back");
                        return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                    }
                    Ok(a) if a.confidence > 0.3 && attempt < max_retries => {
                        excerpt_size *= 2;
                        Self::report_step(
                            on_step,
                            &format!("retrying_analysis_{}", attempt + 1),
                        );
                        warn!(
                            %doc_id, confidence = a.confidence, attempt = attempt + 1,
                            max_retries, next_excerpt = excerpt_size,
                            "Analysis confidence borderline, retrying with larger excerpt"
                        );
                        continue;
                    }
                    Ok(a) if a.confidence > 0.3 => {
                        info!(%doc_id, confidence = a.confidence, "Accepting borderline analysis after retries");
                        result = Some(a);
                        break;
                    }
                    Ok(a) => {
                        warn!(%doc_id, confidence = a.confidence, "Analysis confidence too low, falling back");
                        return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                    }
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Analysis failed, falling back to mechanical");
                        return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                    }
                }
            }

            match result {
                Some(a) => a,
                None => return self.mechanical_fallback(raw_text, doc_id, workspace_id),
            }
        };

        // Resolve effective parameters
        let effective_quality_threshold = self
            .quality_threshold_override
            .or(analysis.recommended_quality_threshold)
            .unwrap_or(0.7);
        let effective_max_chunk_size = self
            .max_chunk_size_override
            .or(analysis.recommended_max_chunk_size)
            .unwrap_or(self.max_chunk_size);

        info!(
            %doc_id,
            quality_threshold = effective_quality_threshold,
            max_chunk_size = effective_max_chunk_size,
            auto = self.quality_threshold_override.is_none(),
            "Effective processing parameters"
        );

        // ── Step 2+3: Convert → Quality Check ───────────────────────────
        Self::report_step(on_step, "converting");

        let (converted, quality) = {
            let mut converted = if analysis.needs_ocr_correction && raw_bytes.is_some() {
                Self::report_step(on_step, "converting_with_vision");
                info!(%doc_id, "AI Agent: converting with vision model (OCR document)");
                match self.do_convert_vision(raw_bytes.unwrap(), mime_type, raw_text, &analysis, doc_id).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Vision conversion failed, falling back to text");
                        self.do_convert(pages, raw_text, is_multipage, &analysis, doc_id).await?
                    }
                }
            } else {
                self.do_convert(pages, raw_text, is_multipage, &analysis, doc_id).await?
            };

            let converter_max_retries = if self.retry.enabled {
                self.retry.converter_max_retries
            } else {
                0
            };
            let mut quality;
            let mut converter_attempt = 0u32;

            loop {
                Self::report_step(
                    on_step,
                    if converter_attempt == 0 {
                        "checking_quality"
                    } else {
                        "rechecking_quality"
                    },
                );
                let use_vision_quality = raw_bytes.is_some() && self.quality_checker.supports_vision();
                if use_vision_quality {
                    info!(%doc_id, attempt = converter_attempt, "AI Agent: checking quality with vision");
                } else {
                    info!(%doc_id, attempt = converter_attempt, "AI Agent: checking conversion quality");
                }

                quality = if use_vision_quality {
                    match self.quality_checker.check_with_vision(raw_bytes.unwrap(), mime_type, raw_text, &converted).await {
                        Ok(mut q) => {
                            q.passed = q.overall_score >= effective_quality_threshold;
                            q
                        }
                        Err(e) => {
                            warn!(%doc_id, error = %e, "Vision quality check failed, trying text-only");
                            match self.quality_checker.check(raw_text, &converted).await {
                                Ok(mut q) => {
                                    q.passed = q.overall_score >= effective_quality_threshold;
                                    q
                                }
                                Err(e2) => {
                                    warn!(%doc_id, error = %e2, "Quality check failed, falling back");
                                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                                }
                            }
                        }
                    }
                } else {
                    match self.quality_checker.check(raw_text, &converted).await {
                        Ok(mut q) => {
                            q.passed = q.overall_score >= effective_quality_threshold;
                            q
                        }
                        Err(e) => {
                            warn!(%doc_id, error = %e, "Quality check failed, falling back");
                            return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                        }
                    }
                };

                if quality.passed {
                    info!(
                        %doc_id, overall = quality.overall_score,
                        threshold = effective_quality_threshold,
                        "Quality check passed"
                    );
                    break;
                }

                if converter_attempt >= converter_max_retries {
                    warn!(
                        %doc_id, overall = quality.overall_score,
                        threshold = effective_quality_threshold,
                        issues = ?quality.issues,
                        attempts = converter_attempt,
                        "Quality below threshold after all retries, falling back"
                    );
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }

                converter_attempt += 1;
                Self::report_step(
                    on_step,
                    &format!("retrying_conversion_{converter_attempt}"),
                );
                warn!(
                    %doc_id, overall = quality.overall_score,
                    threshold = effective_quality_threshold,
                    attempt = converter_attempt, max = converter_max_retries,
                    issues = ?quality.issues,
                    "Quality below threshold, retrying converter with feedback"
                );

                converted = self
                    .do_convert_with_feedback(
                        pages,
                        raw_text,
                        is_multipage,
                        &analysis,
                        &converted.markdown,
                        &quality.issues,
                        doc_id,
                    )
                    .await?;
            }

            (converted, quality)
        };

        // ── Step 4: Smart Chunk ─────────────────────────────────────────
        Self::report_step(on_step, "chunking");
        info!(%doc_id, max_chunk_size = effective_max_chunk_size, "AI Agent: semantic chunking");

        let enriched_chunks = {
            let chunker_max_retries = if self.retry.enabled {
                self.retry.chunker_max_retries
            } else {
                0
            };

            let mut chunks: Vec<thairag_core::types::EnrichedChunk> = match self
                .smart_chunker
                .chunk(&converted, effective_max_chunk_size)
                .await
            {
                Ok(c) if !c.is_empty() => c,
                Ok(_) => {
                    warn!(%doc_id, "Smart chunker returned empty, falling back");
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }
                Err(e) => {
                    warn!(%doc_id, error = %e, "Smart chunking failed, falling back");
                    return self.mechanical_fallback(raw_text, doc_id, workspace_id);
                }
            };

            for attempt in 1..=chunker_max_retries {
                let issues =
                    validate_chunks(&chunks, &converted.markdown, effective_max_chunk_size);
                if issues.is_empty() {
                    break;
                }

                Self::report_step(on_step, &format!("retrying_chunking_{attempt}"));
                warn!(
                    %doc_id, attempt, max = chunker_max_retries,
                    issues = ?issues, "Chunk validation failed, retrying with feedback"
                );

                match self
                    .smart_chunker
                    .chunk_with_feedback(&converted, effective_max_chunk_size, &issues)
                    .await
                {
                    Ok(c) if !c.is_empty() => chunks = c,
                    Ok(_) => {
                        warn!(%doc_id, "Chunker retry returned empty, keeping previous");
                        break;
                    }
                    Err(e) => {
                        warn!(%doc_id, error = %e, "Chunker retry failed, keeping previous");
                        break;
                    }
                }
            }

            chunks
        };

        // ── Step 5: Enrich chunks ──────────────────────────────────────
        let mut doc_chunks: Vec<DocumentChunk> = enriched_chunks
            .into_iter()
            .enumerate()
            .map(|(i, ec)| DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id,
                workspace_id,
                content: ec.content,
                chunk_index: i,
                embedding: None,
                metadata: Some(ChunkMetadata {
                    topic: ec.topic,
                    section_title: ec.section_title,
                    language: ec.language,
                    chunk_type: ec.chunk_type,
                    quality_score: Some(quality.overall_score),
                    page_numbers: ec.page_numbers,
                    ..Default::default()
                }),
            })
            .collect();

        if let Some(ref enricher) = self.enricher {
            Self::report_step(on_step, "enriching");
            info!(%doc_id, chunks = doc_chunks.len(), "AI Agent: enriching chunks for search");
            let title = "";
            if let Err(e) = enricher.enrich(&mut doc_chunks, &analysis, title).await {
                warn!(%doc_id, error = %e, "Chunk enrichment failed, continuing without enrichment");
            }
        }

        info!(
            %doc_id,
            chunks = doc_chunks.len(),
            "AI preprocessing complete"
        );

        Ok(doc_chunks)
    }

    // ── Shared helpers ──────────────────────────────────────────────────

    /// Convert using vision model — sends the raw document bytes to the vision-capable LLM.
    async fn do_convert_vision(
        &self,
        raw_bytes: &[u8],
        mime_type: &str,
        raw_text: &str,
        analysis: &thairag_core::types::DocumentAnalysis,
        doc_id: DocId,
    ) -> Result<thairag_core::types::ConvertedDocument> {
        info!(%doc_id, mime_type, size = raw_bytes.len(), "AI Agent: vision-based conversion");
        self.converter
            .convert_with_vision(raw_bytes, mime_type, raw_text, analysis)
            .await
    }

    async fn do_convert(
        &self,
        pages: &[(usize, String)],
        raw_text: &str,
        is_multipage: bool,
        analysis: &thairag_core::types::DocumentAnalysis,
        doc_id: DocId,
    ) -> Result<thairag_core::types::ConvertedDocument> {
        if is_multipage {
            info!(%doc_id, pages = pages.len(), "AI Agent: converting pages individually");
            let total_pages = pages.len();
            let mut markdown_parts = Vec::with_capacity(total_pages);

            for (page_num, page_text) in pages {
                let page_md = match self
                    .converter
                    .convert_page(page_text, analysis, *page_num, total_pages)
                    .await
                {
                    Ok(md) => md,
                    Err(e) => {
                        warn!(%doc_id, page = page_num, error = %e, "Page conversion failed, using raw text");
                        page_text.clone()
                    }
                };
                markdown_parts.push(format!("<!-- page:{page_num} -->\n{page_md}"));
            }

            let markdown = markdown_parts.join("\n\n");
            Ok(thairag_core::types::ConvertedDocument {
                markdown,
                analysis: analysis.clone(),
            })
        } else {
            info!(%doc_id, "AI Agent: converting single-page document");
            use thairag_core::traits::AiDocumentConverter;
            self.converter.convert(raw_text, analysis).await
        }
    }

    async fn do_convert_with_feedback(
        &self,
        pages: &[(usize, String)],
        raw_text: &str,
        is_multipage: bool,
        analysis: &thairag_core::types::DocumentAnalysis,
        previous_markdown: &str,
        issues: &[String],
        doc_id: DocId,
    ) -> Result<thairag_core::types::ConvertedDocument> {
        if is_multipage {
            let total_pages = pages.len();
            let mut markdown_parts = Vec::with_capacity(total_pages);

            for (page_num, page_text) in pages {
                let page_md = match self
                    .converter
                    .convert_page_with_feedback(
                        page_text,
                        analysis,
                        *page_num,
                        total_pages,
                        previous_markdown,
                        issues,
                    )
                    .await
                {
                    Ok(md) => md,
                    Err(e) => {
                        warn!(%doc_id, page = page_num, error = %e, "Feedback page conversion failed, using raw text");
                        page_text.clone()
                    }
                };
                markdown_parts.push(format!("<!-- page:{page_num} -->\n{page_md}"));
            }

            let markdown = markdown_parts.join("\n\n");
            Ok(thairag_core::types::ConvertedDocument {
                markdown,
                analysis: analysis.clone(),
            })
        } else {
            self.converter
                .convert_with_feedback(raw_text, analysis, previous_markdown, issues)
                .await
        }
    }

    fn mechanical_fallback(
        &self,
        text: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let chunks = self
            .mechanical_chunker
            .chunk(text, self.max_chunk_size, self.chunk_overlap);
        Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(i, content)| DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id,
                workspace_id,
                content,
                chunk_index: i,
                embedding: None,
                metadata: None,
            })
            .collect())
    }
}

/// Calculate dynamic orchestrator budget based on document complexity.
///
/// Base budget (3) + complexity factors. Fully adaptive — no user-configured ceiling.
/// Internal safety cap of 15 prevents runaway in extreme edge cases.
///
/// Factors:
/// - OCR documents: +2 (more likely to need converter retries)
/// - Unstructured: +1, SemiStructured: +0, WellStructured: -1
/// - Mixed/Tabular/Form/Slides content: +1 (harder conversion)
/// - Large docs (>20 sections): +1
/// - Multipage: +1
///
/// Result is clamped to [2, 15].
fn compute_orchestrator_budget(
    analysis: &DocumentAnalysis,
    is_multipage: bool,
) -> u32 {
    let mut budget: i32 = 3; // base: analyze + quality + chunk reviews

    // OCR is messy — more retries likely
    if analysis.needs_ocr_correction {
        budget += 2;
    }

    // Structure complexity
    match analysis.structure_level {
        StructureLevel::Unstructured => budget += 1,
        StructureLevel::SemiStructured => {}
        StructureLevel::WellStructured => budget -= 1,
    }

    // Content type complexity
    match analysis.content_type {
        ContentType::Mixed | ContentType::Tabular | ContentType::Form => budget += 1,
        ContentType::Slides => budget += 1,
        ContentType::Narrative => {}
    }

    // Large documents with many sections
    if analysis.estimated_sections > 20 {
        budget += 1;
    }

    // Multipage (PDF) documents
    if is_multipage {
        budget += 1;
    }

    // Clamp: minimum 2, internal safety cap 15
    (budget.max(2) as u32).min(15)
}

/// Helper: extract a short name from an OrchestratorAction for logging.
fn action_name(action: &OrchestratorAction) -> &'static str {
    match action {
        OrchestratorAction::Accept => "accept",
        OrchestratorAction::Retry { .. } => "retry",
        OrchestratorAction::Skip { .. } => "skip",
        OrchestratorAction::FallbackMechanical { .. } => "fallback_mechanical",
        OrchestratorAction::FlagForReview { .. } => "flag_for_review",
        OrchestratorAction::AdjustParams { .. } => "adjust_params",
    }
}
