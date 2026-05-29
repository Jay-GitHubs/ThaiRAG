use std::sync::Arc;

use thairag_config::schema::{AiPreprocessingConfig, ChunkingStrategy};
use thairag_core::PromptRegistry;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::{Chunker, DocumentProcessor, LlmProvider};
use thairag_core::types::{
    ChunkId, ChunkMetadata, DocId, DocumentChunk, DocumentContentType, WorkspaceId,
};
use tracing::info;

use crate::ai::pipeline::AiDocumentPipeline;
use crate::chunker::MarkdownChunker;
use crate::converter::{MarkdownConverter, extract_pdf_pages_unfiltered};
use crate::image;
use crate::pdf_rasterizer::{self, RasterizeOptions};
use crate::table_extractor;
use crate::text_utils::meaningful_char_count;
use crate::thai_chunker::ThaiAwareChunker;

const PDF_MIME: &str = "application/pdf";

/// Stable reason codes for [`ThaiRagError::EmptyExtraction`].
/// Surface these in admin UIs and treat as a stable API.
pub mod empty_reason {
    /// Format produced no meaningful text and no vision-capable LLM is wired up.
    pub const NO_TEXT_VISION_UNAVAILABLE: &str = "no_text_vision_unavailable";
    /// Format produced no meaningful text; vision was attempted but also yielded nothing.
    pub const NO_TEXT_VISION_FAILED: &str = "no_text_vision_failed";
    /// Document exceeded the per-doc vision-page budget before producing usable content.
    pub const VISION_BUDGET_EXCEEDED: &str = "vision_budget_exceeded";
    /// Format produced no meaningful text and no vision fallback exists for this format.
    pub const NO_TEXT_NO_FALLBACK: &str = "no_text_no_fallback";
}

/// Callback invoked when the pipeline enters a new processing step.
/// Steps: "analyzing", "converting", "checking_quality", "chunking", "indexing".
pub type StepCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Output of [`DocumentPipeline::process_to_document`]: the chunks plus the
/// artifacts the caller (the API/store layer) persists — the canonical
/// semantic-markdown document (smart-PDF path) and any extracted image blobs.
/// Image ids are already embedded in the chunks (`image_blob_id`) and the
/// markdown (`[IMAGE:<id>]`), so the caller only needs to save the bytes.
pub struct ProcessedDocument {
    pub chunks: Vec<DocumentChunk>,
    pub images: Vec<crate::smart_pdf::ExtractedImageBlob>,
    pub markdown: Option<String>,
}

/// Orchestrates: convert raw bytes → chunk text → produce DocumentChunks.
/// When AI preprocessing is enabled, delegates to the AI agent team.
/// Supports multi-modal content: images (via LLM vision) and table extraction.
pub struct DocumentPipeline {
    converter: MarkdownConverter,
    chunker: Box<dyn Chunker>,
    max_chunk_size: usize,
    chunk_overlap: usize,
    ai_pipeline: Option<AiDocumentPipeline>,
    /// LLM provider for image description (only used when image_description_enabled).
    vision_llm: Option<Arc<dyn LlmProvider>>,
    /// Whether to generate LLM descriptions for uploaded images.
    image_description_enabled: bool,
    /// Whether to extract tables from text content and add as separate chunks.
    table_extraction_enabled: bool,
    /// Chunking strategy for the mechanical (and AI) path.
    chunking_strategy: ChunkingStrategy,
    /// Sentence-window: neighbour sentences on each side.
    sentence_window_size: usize,
    /// Parent-document: target parent chunk size (chars).
    parent_chunk_size: usize,
    /// Parent-document: target child chunk size (chars).
    child_chunk_size: usize,
    /// Enable vision-LLM rasterization for PDF pages with no extractable text.
    /// No-op unless [`vision_llm`] is also configured.
    pdf_vision_fallback_enabled: bool,
    /// Per-page threshold below which a PDF page is treated as image-only.
    pdf_min_chars_per_page: usize,
    /// Hard cap on the number of pages that may be rasterized per PDF
    /// (prevents pathological 10,000-page uploads from blowing up vision spend).
    pdf_max_vision_pages: usize,
}

impl DocumentPipeline {
    /// Create a mechanical-only pipeline (no AI).
    pub fn new(max_chunk_size: usize, chunk_overlap: usize) -> Self {
        Self::new_with_language_aware(max_chunk_size, chunk_overlap, true)
    }

    /// Create a mechanical-only pipeline with configurable language awareness.
    pub fn new_with_language_aware(
        max_chunk_size: usize,
        chunk_overlap: usize,
        language_aware_chunking: bool,
    ) -> Self {
        let chunker: Box<dyn Chunker> = if language_aware_chunking {
            Box::new(ThaiAwareChunker::new())
        } else {
            Box::new(MarkdownChunker::new())
        };
        Self {
            converter: MarkdownConverter::new(),
            chunker,
            max_chunk_size,
            chunk_overlap,
            ai_pipeline: None,
            vision_llm: None,
            image_description_enabled: false,
            table_extraction_enabled: true,
            chunking_strategy: ChunkingStrategy::Standard,
            sentence_window_size: 3,
            parent_chunk_size: 2048,
            child_chunk_size: 384,
            pdf_vision_fallback_enabled: true,
            pdf_min_chars_per_page: 50,
            pdf_max_vision_pages: 100,
        }
    }

    /// Create a pipeline with optional AI preprocessing (shared LLM for all agents).
    pub fn new_with_ai(
        max_chunk_size: usize,
        chunk_overlap: usize,
        llm: Arc<dyn LlmProvider>,
        ai_config: &AiPreprocessingConfig,
    ) -> Self {
        let enricher_llm = if ai_config.enricher_enabled {
            Some(Arc::clone(&llm))
        } else {
            None
        };
        let orchestrator_llm = if ai_config.orchestrator_enabled {
            Some(Arc::clone(&llm))
        } else {
            None
        };
        Self::new_with_per_agent_ai(
            max_chunk_size,
            chunk_overlap,
            Arc::clone(&llm),
            Arc::clone(&llm),
            Arc::clone(&llm),
            Arc::clone(&llm),
            enricher_llm,
            orchestrator_llm,
            ai_config,
        )
    }

    /// Create a pipeline with per-agent LLM providers.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_per_agent_ai(
        max_chunk_size: usize,
        chunk_overlap: usize,
        analyzer_llm: Arc<dyn LlmProvider>,
        converter_llm: Arc<dyn LlmProvider>,
        quality_llm: Arc<dyn LlmProvider>,
        chunker_llm: Arc<dyn LlmProvider>,
        enricher_llm: Option<Arc<dyn LlmProvider>>,
        orchestrator_llm: Option<Arc<dyn LlmProvider>>,
        ai_config: &AiPreprocessingConfig,
    ) -> Self {
        Self::new_with_per_agent_ai_and_prompts(
            max_chunk_size,
            chunk_overlap,
            analyzer_llm,
            converter_llm,
            quality_llm,
            chunker_llm,
            enricher_llm,
            orchestrator_llm,
            ai_config,
            Arc::new(PromptRegistry::new()),
        )
    }

    /// Create a pipeline with per-agent LLM providers and shared prompt registry.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_per_agent_ai_and_prompts(
        max_chunk_size: usize,
        chunk_overlap: usize,
        analyzer_llm: Arc<dyn LlmProvider>,
        converter_llm: Arc<dyn LlmProvider>,
        quality_llm: Arc<dyn LlmProvider>,
        chunker_llm: Arc<dyn LlmProvider>,
        enricher_llm: Option<Arc<dyn LlmProvider>>,
        orchestrator_llm: Option<Arc<dyn LlmProvider>>,
        ai_config: &AiPreprocessingConfig,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        let ai_pipeline = if ai_config.enabled {
            Some(AiDocumentPipeline::new_per_agent_with_prompts(
                analyzer_llm,
                converter_llm,
                quality_llm,
                chunker_llm,
                enricher_llm,
                orchestrator_llm,
                ai_config,
                max_chunk_size,
                chunk_overlap,
                prompts,
            ))
        } else {
            None
        };

        let chunker: Box<dyn Chunker> = Box::new(ThaiAwareChunker::new());
        Self {
            converter: MarkdownConverter::new(),
            chunker,
            max_chunk_size,
            chunk_overlap,
            ai_pipeline,
            vision_llm: None,
            image_description_enabled: false,
            table_extraction_enabled: true,
            chunking_strategy: ChunkingStrategy::Standard,
            sentence_window_size: 3,
            parent_chunk_size: 2048,
            child_chunk_size: 384,
            pdf_vision_fallback_enabled: true,
            pdf_min_chars_per_page: 50,
            pdf_max_vision_pages: 100,
        }
    }

    /// Set the vision LLM and enable image description.
    pub fn with_image_description(mut self, llm: Arc<dyn LlmProvider>, enabled: bool) -> Self {
        if enabled {
            self.vision_llm = Some(llm);
            self.image_description_enabled = true;
        }
        self
    }

    /// Configure the PDF vision fallback (used for PowerPoint-derived or
    /// scanned PDFs where text extraction yields nothing). The fallback
    /// is only activated when a vision LLM is also configured.
    pub fn with_pdf_vision_fallback(
        mut self,
        enabled: bool,
        min_chars_per_page: usize,
        max_vision_pages: usize,
    ) -> Self {
        self.pdf_vision_fallback_enabled = enabled;
        self.pdf_min_chars_per_page = min_chars_per_page;
        self.pdf_max_vision_pages = max_vision_pages;
        self
    }

    /// Set whether table extraction is enabled.
    pub fn with_table_extraction(mut self, enabled: bool) -> Self {
        self.table_extraction_enabled = enabled;
        self
    }

    /// Configure the chunking strategy and its sizing parameters.
    ///
    /// A non-`Standard` strategy bypasses the AI preprocessing pipeline for
    /// chunking — sentence-window and parent-document splitting are an
    /// alternative chunking philosophy to AI semantic chunking.
    pub fn with_chunking_strategy(
        mut self,
        strategy: ChunkingStrategy,
        sentence_window_size: usize,
        parent_chunk_size: usize,
        child_chunk_size: usize,
    ) -> Self {
        self.chunking_strategy = strategy;
        self.sentence_window_size = sentence_window_size;
        self.parent_chunk_size = parent_chunk_size;
        self.child_chunk_size = child_chunk_size;
        self
    }

    /// Split converted text into chunks per the configured strategy.
    fn chunk_with_strategy(
        &self,
        text: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Vec<DocumentChunk> {
        match self.chunking_strategy {
            ChunkingStrategy::Standard => {
                let chunks = self
                    .chunker
                    .chunk(text, self.max_chunk_size, self.chunk_overlap);
                chunks
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
                    .collect()
            }
            ChunkingStrategy::SentenceWindow => {
                crate::window_chunker::build_sentence_window_chunks(
                    text,
                    self.sentence_window_size,
                    doc_id,
                    workspace_id,
                )
            }
            ChunkingStrategy::ParentDocument => {
                crate::window_chunker::build_parent_document_chunks(
                    text,
                    self.parent_chunk_size,
                    self.child_chunk_size,
                    doc_id,
                    workspace_id,
                )
            }
        }
    }

    /// Process document bytes into chunks.
    /// If AI preprocessing is enabled, uses the AI agent team.
    /// Otherwise, uses the mechanical pipeline.
    /// The optional `on_step` callback is invoked at each processing stage.
    ///
    /// Supports multi-modal content:
    /// - Image files are described via LLM vision (if enabled) and stored as text chunks.
    /// - Tables in text/PDF content are extracted and appended as structured markdown chunks.
    ///
    /// Returns [`ThaiRagError::EmptyExtraction`] when no meaningful content
    /// could be extracted — callers should surface the structured reason/hint
    /// to operators rather than silently marking the document as Ready.
    pub async fn process(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: Option<StepCallback>,
    ) -> Result<Vec<DocumentChunk>> {
        if self.smart_pdf_eligible(mime_type) && crate::pdfium_engine::is_available() {
            match self.process_pdf_smart(raw, doc_id, workspace_id).await {
                Ok(doc) if !doc.chunks.is_empty() => return Ok(doc.chunks),
                Ok(_) => tracing::warn!(
                    %doc_id,
                    "smart-pdf produced no chunks — falling back to legacy vision path"
                ),
                Err(e) => tracing::warn!(
                    %doc_id, error = %e,
                    "smart-pdf path failed — falling back to legacy vision path"
                ),
            }
        }
        self.process_non_smart(raw, mime_type, doc_id, workspace_id, on_step)
            .await
    }

    /// Like [`process`](Self::process), but also returns the canonical semantic
    /// markdown and the extracted image blobs (smart-PDF path) so the caller
    /// can persist them. Non-smart paths return chunks only (`markdown: None`,
    /// no images).
    pub async fn process_to_document(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: Option<StepCallback>,
    ) -> Result<ProcessedDocument> {
        if self.smart_pdf_eligible(mime_type) && crate::pdfium_engine::is_available() {
            match self.process_pdf_smart(raw, doc_id, workspace_id).await {
                Ok(doc) if !doc.chunks.is_empty() => return Ok(doc),
                Ok(_) => tracing::warn!(
                    %doc_id,
                    "smart-pdf produced no chunks — falling back to legacy vision path"
                ),
                Err(e) => tracing::warn!(
                    %doc_id, error = %e,
                    "smart-pdf path failed — falling back to legacy vision path"
                ),
            }
        }
        let chunks = self
            .process_non_smart(raw, mime_type, doc_id, workspace_id, on_step)
            .await?;
        Ok(ProcessedDocument {
            chunks,
            images: Vec::new(),
            markdown: None,
        })
    }

    /// Whether the pdfium smart-PDF engine should be attempted for this input.
    fn smart_pdf_eligible(&self, mime_type: &str) -> bool {
        mime_type == PDF_MIME
            && self.pdf_vision_fallback_enabled
            && self.image_description_enabled
            && self.vision_llm.is_some()
    }

    /// All non-smart-PDF processing: the image route, the legacy page-aware PDF
    /// vision fallback (used when pdfium is unavailable or the smart path
    /// produced nothing), and the mechanical/AI text path with table extraction
    /// and the universal zero-chunk guard.
    async fn process_non_smart(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: Option<StepCallback>,
    ) -> Result<Vec<DocumentChunk>> {
        // Route image files to the image description pipeline
        if image::is_image_mime(mime_type) {
            return self
                .process_image(raw, mime_type, doc_id, workspace_id)
                .await;
        }

        // Legacy page-aware vision fallback for PowerPoint→PDF exports and
        // scanned PDFs, used when the pdfium smart engine is unavailable.
        if mime_type == PDF_MIME
            && self.pdf_vision_fallback_enabled
            && self.image_description_enabled
            && self.vision_llm.is_some()
        {
            return self
                .process_pdf_with_vision(raw, doc_id, workspace_id)
                .await;
        }

        // A non-Standard chunking strategy bypasses the AI pipeline: the
        // window/parent splitters are an alternative chunking philosophy.
        let mut chunks = if self.chunking_strategy != ChunkingStrategy::Standard {
            if self.ai_pipeline.is_some() {
                info!(
                    %doc_id,
                    strategy = ?self.chunking_strategy,
                    "Non-standard chunking strategy — bypassing AI preprocessing for chunking"
                );
            }
            self.process_mechanical(raw, mime_type, doc_id, workspace_id)?
        } else if let Some(ai) = &self.ai_pipeline {
            ai.process(raw, mime_type, doc_id, workspace_id, on_step)
                .await?
        } else {
            self.process_mechanical(raw, mime_type, doc_id, workspace_id)?
        };

        // Run table extraction on text-based content and append table chunks
        if self.table_extraction_enabled {
            let text_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
            let table_chunks =
                self.extract_table_chunks(&text_content, doc_id, workspace_id, chunks.len());
            if !table_chunks.is_empty() {
                info!(
                    %doc_id,
                    table_count = table_chunks.len(),
                    "Extracted tables as separate chunks"
                );
                chunks.extend(table_chunks);
            }
        }

        // Universal zero-chunk guard: if we reach here with no chunks, the
        // document is unsearchable. Fail loud with a reason the operator
        // can act on, instead of silently storing an empty document.
        if chunks.is_empty() {
            return Err(self.empty_extraction_error(mime_type));
        }

        Ok(chunks)
    }

    /// Build a structured [`ThaiRagError::EmptyExtraction`] tailored to the
    /// document format and the current vision-LLM availability. This is the
    /// single place that decides what hint to surface to operators.
    fn empty_extraction_error(&self, mime_type: &str) -> ThaiRagError {
        let vision_ready = self.vision_llm.is_some()
            && self.image_description_enabled
            && self
                .vision_llm
                .as_ref()
                .map(|llm| llm.supports_vision())
                .unwrap_or(false);

        let (reason, hint) = if mime_type == PDF_MIME {
            if vision_ready {
                (
                    empty_reason::NO_TEXT_VISION_FAILED,
                    "PDF text extraction yielded no content and the vision-LLM fallback \
                     also produced no usable text. The document may be blank, corrupted, \
                     or in an unsupported encoding."
                        .to_string(),
                )
            } else {
                (
                    empty_reason::NO_TEXT_VISION_UNAVAILABLE,
                    "PDF appears to be image-only (e.g. exported from PowerPoint or scanned) \
                     and no vision-capable LLM is configured. Set \
                     `[document].image_description_enabled = true` and use an LLM that \
                     supports vision (e.g. Ollama `llava`, Claude 3+, GPT-4V)."
                        .to_string(),
                )
            }
        } else if image::is_image_mime(mime_type) {
            (
                empty_reason::NO_TEXT_VISION_UNAVAILABLE,
                format!(
                    "Image upload produced no description. Enable \
                     `[document].image_description_enabled` with a vision-capable LLM to OCR \
                     uploaded images. Got mime: {mime_type}."
                ),
            )
        } else {
            (
                empty_reason::NO_TEXT_NO_FALLBACK,
                format!(
                    "Format `{mime_type}` produced no meaningful text and has no vision \
                     fallback. The document may be empty, password-protected, or composed \
                     entirely of embedded images. Try converting it to PDF first so the \
                     PDF vision fallback can OCR it."
                ),
            )
        };

        ThaiRagError::empty_extraction(reason, hint)
    }

    /// Mechanical pipeline: convert → chunk → produce DocumentChunks.
    /// Honours the configured [`ChunkingStrategy`].
    ///
    /// Emits [`ThaiRagError::EmptyExtraction`] when the converter produces no
    /// meaningful text (heuristic: strips page numbers, separators, and
    /// repeated whitespace before measuring). For PDFs the caller may want
    /// to handle the error by routing to the vision-fallback path instead.
    pub fn process_mechanical(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let text = self.converter.convert(raw, mime_type)?;

        // Image MIME types are routed through process_image() upstream and
        // arrive here only when the caller bypassed process(); in that case
        // the converter returns a placeholder like "[Image: ..., N bytes]"
        // which we deliberately keep as searchable text.
        if !image::is_image_mime(mime_type) && meaningful_char_count(&text) == 0 {
            return Err(self.empty_extraction_error(mime_type));
        }

        Ok(self.chunk_with_strategy(&text, doc_id, workspace_id))
    }

    /// Smart PDF path: extract text per page, fall back to vision-LLM
    /// rasterization for pages where extraction yields fewer than
    /// `pdf_min_chars_per_page` characters. Each produced chunk carries
    /// the originating page number in metadata.
    ///
    /// Hard caps on vision usage prevent abusive PDFs from translating to
    /// thousands of vision-LLM calls.
    /// Smart per-page PDF extraction (pdfium): pick a strategy per page, build
    /// one semantic-markdown document, and chunk it per page with strategy /
    /// page metadata. The caller has already confirmed pdfium is available and
    /// a vision LLM is configured.
    async fn process_pdf_smart(
        &self,
        raw: &[u8],
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<ProcessedDocument> {
        use crate::semantic::PageStrategy;
        use crate::smart_pdf::SmartPdfConfig;

        let llm = self
            .vision_llm
            .as_ref()
            .expect("process_pdf_smart called without vision_llm — caller must check")
            .clone();

        let cfg = SmartPdfConfig {
            min_chars_per_page: self.pdf_min_chars_per_page,
            max_vision_pages: self.pdf_max_vision_pages,
            ..SmartPdfConfig::default()
        };

        // Phase 1 (sync, pdfium is !Send): extract per-page data off the async
        // runtime. The PDF bytes are moved into the blocking task.
        let raw_owned = raw.to_vec();
        let cfg_blocking = cfg.clone();
        let extracts = tokio::task::spawn_blocking(move || {
            crate::smart_pdf::extract_pages(&raw_owned, &cfg_blocking)
        })
        .await
        .map_err(|e| ThaiRagError::Validation(format!("smart-pdf extract task join: {e}")))??;

        // Phase 2 (async): vision per page + assemble the document.
        let doc = crate::smart_pdf::render_to_document("", extracts, llm.as_ref(), &cfg).await;

        info!(
            %doc_id,
            total_pages = doc.total_pages,
            vision_pages_used = doc.vision_pages_used,
            pages_vision_failed = doc.pages_vision_failed,
            markdown_bytes = doc.markdown.len(),
            vision_model = llm.model_name(),
            "Smart PDF (pdfium) processing complete"
        );

        // Chunk each page's markdown separately so chunks carry page number,
        // strategy, and content-type metadata.
        let mut chunks = Vec::new();
        let mut chunk_index = 0usize;
        for page in &doc.pages {
            let body = page.markdown.trim();
            if body.is_empty() {
                continue;
            }
            let content_type = match page.strategy {
                PageStrategy::Tabular => DocumentContentType::Table,
                PageStrategy::ImageHeavy | PageStrategy::Scanned => DocumentContentType::Image,
                PageStrategy::Mixed => DocumentContentType::Mixed,
                PageStrategy::TextOnly => DocumentContentType::Text,
            };
            let strategy = page.strategy.as_str().to_string();
            // Link every chunk of this page to the page's persisted image blob
            // (if one was rendered), so retrieval can surface the page image.
            let image_blob_id = doc
                .images
                .iter()
                .find(|b| b.page_num == page.page_num as u32)
                .map(|b| b.image_id);
            for content in self
                .chunker
                .chunk(body, self.max_chunk_size, self.chunk_overlap)
            {
                chunks.push(DocumentChunk {
                    chunk_id: ChunkId::new(),
                    doc_id,
                    workspace_id,
                    content,
                    chunk_index,
                    embedding: None,
                    metadata: Some(ChunkMetadata {
                        content_type: Some(content_type.clone()),
                        chunk_type: Some(strategy.clone()),
                        mime_type: Some(PDF_MIME.to_string()),
                        page_numbers: Some(vec![page.page_num]),
                        page_strategy: Some(strategy.clone()),
                        image_blob_id,
                        ..Default::default()
                    }),
                });
                chunk_index += 1;
            }
        }
        Ok(ProcessedDocument {
            chunks,
            images: doc.images,
            markdown: Some(doc.markdown),
        })
    }

    async fn process_pdf_with_vision(
        &self,
        raw: &[u8],
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let pages = extract_pdf_pages_unfiltered(raw)?;
        let total_pages = pages.len();
        let llm = self
            .vision_llm
            .as_ref()
            .expect("process_pdf_with_vision called without vision_llm — caller must check");

        let mut chunks: Vec<DocumentChunk> = Vec::new();
        let mut vision_pages_used: usize = 0;
        let mut chunk_index: usize = 0;
        let mut pages_needing_vision: usize = 0;
        let mut pages_over_budget: usize = 0;
        // Tracked separately so the failure reason can tell an operator
        // whether the problem is server-side page rendering (pdftoppm) or the
        // vision model itself — they need very different fixes.
        let mut pages_rasterize_failed: usize = 0;
        let mut pages_llm_failed: usize = 0;

        for (page_num, page_text) in pages {
            let trimmed = page_text.trim();
            // Use the meaningful-text heuristic instead of raw char count so
            // PowerPoint-PDFs that yield only a page number ("- 1 -") still
            // route to the vision fallback.
            let meaningful_count = meaningful_char_count(trimmed);
            let needs_vision = meaningful_count < self.pdf_min_chars_per_page;
            if needs_vision {
                pages_needing_vision += 1;
            }

            let (page_content, used_vision) = if needs_vision
                && vision_pages_used < self.pdf_max_vision_pages
            {
                // Two distinct stages: render the page to PNG (pdftoppm),
                // then ask the vision model to describe it. Keep their
                // failures apart so diagnostics point at the right layer.
                match rasterize_pdf_page(raw, page_num).await {
                    Err(e) => {
                        pages_rasterize_failed += 1;
                        tracing::warn!(
                            %doc_id,
                            page = page_num,
                            error = %e,
                            "PDF page rasterization (pdftoppm) failed — keeping extracted \
                             text. This is a server-side rendering problem, not the vision \
                             model."
                        );
                        (trimmed.to_string(), false)
                    }
                    Ok(png) => match image::describe_image(llm.as_ref(), &png, "image/png").await {
                        Ok(desc) => {
                            vision_pages_used += 1;
                            info!(
                                %doc_id,
                                page = page_num,
                                vision_model = llm.model_name(),
                                desc_len = desc.len(),
                                "PDF page rasterized and described via vision fallback"
                            );
                            (desc, true)
                        }
                        Err(e) => {
                            pages_llm_failed += 1;
                            tracing::warn!(
                                %doc_id,
                                page = page_num,
                                vision_model = llm.model_name(),
                                error = %e,
                                "Vision model failed to describe PDF page — keeping \
                                 extracted text"
                            );
                            (trimmed.to_string(), false)
                        }
                    },
                }
            } else {
                if needs_vision {
                    pages_over_budget += 1;
                    tracing::warn!(
                        %doc_id,
                        page = page_num,
                        cap = self.pdf_max_vision_pages,
                        "Skipping vision fallback — pdf_max_vision_pages cap reached"
                    );
                }
                (trimmed.to_string(), false)
            };

            if page_content.trim().is_empty() {
                continue;
            }

            // Chunk the page text and tag each produced chunk with its page number.
            let page_chunks =
                self.chunker
                    .chunk(&page_content, self.max_chunk_size, self.chunk_overlap);
            for content in page_chunks {
                chunks.push(DocumentChunk {
                    chunk_id: ChunkId::new(),
                    doc_id,
                    workspace_id,
                    content,
                    chunk_index,
                    embedding: None,
                    metadata: Some(ChunkMetadata {
                        content_type: Some(if used_vision {
                            DocumentContentType::Image
                        } else {
                            DocumentContentType::Text
                        }),
                        chunk_type: Some(
                            if used_vision {
                                "pdf_vision_page"
                            } else {
                                "pdf_text_page"
                            }
                            .to_string(),
                        ),
                        mime_type: Some(PDF_MIME.to_string()),
                        page_numbers: Some(vec![page_num]),
                        ..Default::default()
                    }),
                });
                chunk_index += 1;
            }
        }

        info!(
            %doc_id,
            total_pages,
            vision_pages_used,
            pages_over_budget,
            pages_rasterize_failed,
            pages_llm_failed,
            vision_model = llm.model_name(),
            chunks_produced = chunks.len(),
            "Smart PDF processing complete"
        );

        // If we produced nothing at all, surface a structured reason so the
        // operator knows whether to raise the vision budget, install a real
        // vision model, or accept the document is genuinely empty.
        if chunks.is_empty() {
            return Err(self.pdf_empty_reason(
                total_pages,
                pages_needing_vision,
                pages_over_budget,
                pages_rasterize_failed,
                pages_llm_failed,
                llm.model_name(),
            ));
        }

        Ok(chunks)
    }

    /// Pick the most informative reason code when a PDF produced zero chunks.
    /// Ordered most-actionable first.
    fn pdf_empty_reason(
        &self,
        total_pages: usize,
        pages_needing_vision: usize,
        pages_over_budget: usize,
        pages_rasterize_failed: usize,
        pages_llm_failed: usize,
        vision_model: &str,
    ) -> ThaiRagError {
        if pages_over_budget > 0 {
            return ThaiRagError::empty_extraction(
                empty_reason::VISION_BUDGET_EXCEEDED,
                format!(
                    "PDF needed vision OCR on {pages_needing_vision} of {total_pages} pages but the \
                     budget of {budget} pages was reached after {used} usable extractions. Raise \
                     `[document].pdf_max_vision_pages` or split the document.",
                    budget = self.pdf_max_vision_pages,
                    used = self.pdf_max_vision_pages.saturating_sub(pages_over_budget),
                ),
            );
        }
        // Rasterization runs before the model is ever called, so a pdftoppm
        // failure is a server-side rendering problem — say so instead of
        // pointing the operator at the (innocent) vision model.
        if pages_rasterize_failed > 0 {
            return ThaiRagError::empty_extraction(
                empty_reason::NO_TEXT_VISION_FAILED,
                format!(
                    "PDF needed vision OCR on {pages_needing_vision} of {total_pages} pages, but \
                     rendering the page(s) to images with pdftoppm (poppler-utils) failed on \
                     {pages_rasterize_failed}. This is a server-side rasterization problem, not \
                     the vision model — check the poppler-utils install and the \
                     `THAIRAG__PDF_RASTERIZER__*` limits (see OPERATOR_GUIDE §2.6.6). The \
                     per-page warning logs carry pdftoppm's exact error."
                ),
            );
        }
        if pages_llm_failed > 0 {
            return ThaiRagError::empty_extraction(
                empty_reason::NO_TEXT_VISION_FAILED,
                format!(
                    "PDF needed vision OCR on {pages_needing_vision} of {total_pages} pages but the \
                     vision model `{vision_model}` failed on {pages_llm_failed} pages. Check the \
                     LLM connection and that the model supports vision (e.g. Ollama `llava`, \
                     Claude 3+)."
                ),
            );
        }
        self.empty_extraction_error(PDF_MIME)
    }

    /// Process an image file: generate a text description and return as a single chunk.
    ///
    /// When `image_description_enabled` is true but the configured LLM
    /// cannot do vision, this fails loud with a structured reason so the
    /// operator knows to install a vision model — silently storing a
    /// metadata placeholder would hide the misconfiguration.
    async fn process_image(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let description = if self.image_description_enabled {
            match &self.vision_llm {
                Some(llm) if llm.supports_vision() => {
                    image::describe_image(llm.as_ref(), raw, mime_type).await?
                }
                _ => {
                    return Err(ThaiRagError::empty_extraction(
                        empty_reason::NO_TEXT_VISION_UNAVAILABLE,
                        format!(
                            "Image upload (mime: {mime_type}) requires a vision-capable LLM. \
                             `image_description_enabled` is on but the configured LLM does not \
                             support vision. Install a vision model (e.g. Ollama `llava`, \
                             Claude 3+, GPT-4V) or disable image_description_enabled to fall \
                             back to a metadata placeholder."
                        ),
                    ));
                }
            }
        } else {
            // Image description disabled — operator explicitly opted out, so
            // store a metadata placeholder rather than failing.
            let meta = image::extract_image_metadata(raw, mime_type);
            image::format_placeholder_description(&meta)
        };

        let metadata = image::extract_image_metadata(raw, mime_type);
        info!(
            %doc_id,
            format = %metadata.format,
            size = metadata.size_bytes,
            description_len = description.len(),
            "Processed image document"
        );

        Ok(vec![DocumentChunk {
            chunk_id: ChunkId::new(),
            doc_id,
            workspace_id,
            content: description,
            chunk_index: 0,
            embedding: None,
            metadata: Some(ChunkMetadata {
                content_type: Some(DocumentContentType::Image),
                chunk_type: Some("image_description".to_string()),
                mime_type: Some(mime_type.to_string()),
                ..Default::default()
            }),
        }])
    }

    /// Extract tables from text content and produce separate markdown table chunks.
    fn extract_table_chunks(
        &self,
        text: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        start_index: usize,
    ) -> Vec<DocumentChunk> {
        let tables = table_extractor::extract_tables(text);
        tables
            .iter()
            .enumerate()
            .filter_map(|(i, table)| {
                let md = table_extractor::table_to_markdown(table);
                if md.is_empty() {
                    return None;
                }
                Some(DocumentChunk {
                    chunk_id: ChunkId::new(),
                    doc_id,
                    workspace_id,
                    content: md,
                    chunk_index: start_index + i,
                    embedding: None,
                    metadata: Some(ChunkMetadata {
                        content_type: Some(DocumentContentType::Table),
                        chunk_type: Some("extracted_table".to_string()),
                        ..Default::default()
                    }),
                })
            })
            .collect()
    }
}

/// Rasterize one PDF page to PNG on a blocking thread (subprocess I/O) so it
/// does not stall the async runtime. Describing the PNG with the vision model
/// is left to the caller, so rasterization and model failures stay distinct.
async fn rasterize_pdf_page(pdf_bytes: &[u8], page: usize) -> Result<Vec<u8>> {
    let pdf_owned = pdf_bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        pdf_rasterizer::rasterize_page(
            &pdf_owned,
            &RasterizeOptions {
                page,
                ..Default::default()
            },
        )
    })
    .await
    .map_err(|e| thairag_core::ThaiRagError::Validation(format!("rasterize task join: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_simple_text() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();
        let chunks = pipeline
            .process_mechanical(b"Hello world", "text/plain", doc_id, ws_id)
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello world");
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].doc_id, doc_id);
        assert_eq!(chunks[0].workspace_id, ws_id);
    }

    #[test]
    fn process_multi_paragraph() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let text = b"Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let chunks = pipeline
            .process_mechanical(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Paragraph one."));
        assert!(chunks[0].content.contains("Paragraph three."));
    }

    #[test]
    fn process_splits_at_max_chunk_size() {
        let pipeline = DocumentPipeline::new(30, 0);
        let text = b"Short one.\n\nAnother paragraph here.";
        let chunks = pipeline
            .process_mechanical(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn process_empty_input_yields_structured_error() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let err = pipeline
            .process_mechanical(b"", "text/plain", DocId::new(), WorkspaceId::new())
            .expect_err("empty input must surface as EmptyExtraction, not Ok(empty chunks)");
        match err {
            ThaiRagError::EmptyExtraction { reason, .. } => {
                assert_eq!(reason, empty_reason::NO_TEXT_NO_FALLBACK);
            }
            other => panic!("expected EmptyExtraction, got {other:?}"),
        }
    }

    #[test]
    fn process_whitespace_only_input_yields_structured_error() {
        let pipeline = DocumentPipeline::new(1000, 0);
        // PowerPoint-style page-number-only "content" — must NOT silently
        // become a Ready document with zero chunks.
        let err = pipeline
            .process_mechanical(
                b"   \n\n  - 1 -  \n\n Page 2 of 3 \n",
                "text/plain",
                DocId::new(),
                WorkspaceId::new(),
            )
            .expect_err("page-number-only content must surface as EmptyExtraction");
        assert!(matches!(err, ThaiRagError::EmptyExtraction { .. }));
    }

    #[tokio::test]
    async fn process_async_empty_input_yields_structured_error() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let err = pipeline
            .process(b"", "text/plain", DocId::new(), WorkspaceId::new(), None)
            .await
            .expect_err("empty async input must surface as EmptyExtraction");
        assert!(matches!(err, ThaiRagError::EmptyExtraction { .. }));
    }

    #[test]
    fn empty_extraction_pdf_without_vision_hints_at_config() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let err = pipeline.empty_extraction_error("application/pdf");
        match err {
            ThaiRagError::EmptyExtraction { reason, hint } => {
                assert_eq!(reason, empty_reason::NO_TEXT_VISION_UNAVAILABLE);
                assert!(
                    hint.contains("image_description_enabled"),
                    "hint should mention the config knob, got: {hint}"
                );
            }
            other => panic!("expected EmptyExtraction, got {other:?}"),
        }
    }

    #[test]
    fn process_unique_chunk_ids() {
        let pipeline = DocumentPipeline::new(20, 0);
        let text = b"AAA\n\nBBB\n\nCCC";
        let chunks = pipeline
            .process_mechanical(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        let ids: std::collections::HashSet<_> = chunks.iter().map(|c| c.chunk_id).collect();
        assert_eq!(ids.len(), chunks.len());
    }

    #[test]
    fn process_chunk_indices_sequential() {
        let pipeline = DocumentPipeline::new(10, 0);
        let text = b"AA\n\nBB\n\nCC";
        let chunks = pipeline
            .process_mechanical(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i);
        }
    }

    #[test]
    fn process_mechanical_standard_strategy_has_no_window_metadata() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let chunks = pipeline
            .process_mechanical(
                b"Plain text here.",
                "text/plain",
                DocId::new(),
                WorkspaceId::new(),
            )
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].metadata.is_none());
    }

    #[test]
    fn process_mechanical_sentence_window_sets_window_text() {
        let pipeline = DocumentPipeline::new(1000, 0).with_chunking_strategy(
            ChunkingStrategy::SentenceWindow,
            2,
            2048,
            384,
        );
        let text = b"First sentence. Second sentence. Third sentence.";
        let chunks = pipeline
            .process_mechanical(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert_eq!(chunks.len(), 3);
        for c in &chunks {
            assert!(c.metadata.as_ref().unwrap().window_text.is_some());
        }
    }

    #[test]
    fn process_mechanical_parent_document_sets_parent_metadata() {
        let pipeline = DocumentPipeline::new(1000, 0).with_chunking_strategy(
            ChunkingStrategy::ParentDocument,
            3,
            64,
            16,
        );
        let text = b"Alpha beta gamma.\n\nDelta epsilon zeta.\n\nEta theta iota.";
        let chunks = pipeline
            .process_mechanical(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert!(!chunks.is_empty());
        for c in &chunks {
            let m = c.metadata.as_ref().unwrap();
            assert!(m.parent_id.is_some());
            assert!(m.parent_content.is_some());
        }
    }

    #[tokio::test]
    async fn process_async_without_ai_uses_mechanical() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let chunks = pipeline
            .process(
                b"Hello async",
                "text/plain",
                DocId::new(),
                WorkspaceId::new(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello async");
        assert!(chunks[0].metadata.is_none());
    }

    /// Build a minimal valid PNG in memory (1x1 white pixel, no actual IDAT compression).
    fn make_minimal_png(width: u32, height: u32) -> Vec<u8> {
        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        // IHDR length = 13
        data.extend_from_slice(&13u32.to_be_bytes());
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        // bit depth, color type, compression, filter, interlace
        data.extend_from_slice(&[8, 2, 0, 0, 0]);
        // CRC placeholder (4 bytes)
        data.extend_from_slice(&[0, 0, 0, 0]);
        data
    }

    /// Build a minimal valid GIF89a header for the given dimensions.
    fn make_minimal_gif(width: u16, height: u16) -> Vec<u8> {
        let mut data = b"GIF89a".to_vec();
        data.extend_from_slice(&width.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        // packed field, bg color index, pixel aspect ratio
        data.extend_from_slice(&[0x00, 0x00, 0x00]);
        data
    }

    #[tokio::test]
    async fn process_image_png_creates_single_chunk_with_metadata() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let png_bytes = make_minimal_png(640, 480);
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();

        let chunks = pipeline
            .process(&png_bytes, "image/png", doc_id, ws_id, None)
            .await
            .unwrap();

        assert_eq!(chunks.len(), 1, "Image should produce exactly one chunk");
        assert_eq!(chunks[0].doc_id, doc_id);
        assert_eq!(chunks[0].workspace_id, ws_id);
        assert_eq!(chunks[0].chunk_index, 0);

        // Content should contain format and dimension info
        let content = &chunks[0].content;
        assert!(
            content.contains("PNG"),
            "Content should mention PNG format, got: {content}"
        );
        assert!(
            content.contains("640x480"),
            "Content should mention dimensions, got: {content}"
        );

        // Metadata should mark this as an image chunk
        let meta = chunks[0]
            .metadata
            .as_ref()
            .expect("Image chunk must have metadata");
        assert_eq!(
            meta.content_type,
            Some(thairag_core::types::DocumentContentType::Image)
        );
        assert_eq!(meta.chunk_type.as_deref(), Some("image_description"));
        assert_eq!(meta.mime_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn process_image_gif_creates_single_chunk_with_metadata() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let gif_bytes = make_minimal_gif(320, 240);
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();

        let chunks = pipeline
            .process(&gif_bytes, "image/gif", doc_id, ws_id, None)
            .await
            .unwrap();

        assert_eq!(
            chunks.len(),
            1,
            "GIF image should produce exactly one chunk"
        );

        let content = &chunks[0].content;
        assert!(
            content.contains("GIF"),
            "Content should mention GIF format, got: {content}"
        );
        assert!(
            content.contains("320x240"),
            "Content should mention dimensions, got: {content}"
        );

        let meta = chunks[0]
            .metadata
            .as_ref()
            .expect("Image chunk must have metadata");
        assert_eq!(
            meta.content_type,
            Some(thairag_core::types::DocumentContentType::Image)
        );
        assert_eq!(meta.mime_type.as_deref(), Some("image/gif"));
    }

    #[tokio::test]
    async fn process_image_jpeg_fallback_for_unknown_dims() {
        let pipeline = DocumentPipeline::new(1000, 0);
        // Use fake JPEG bytes (no valid SOF marker, so dims will be None)
        let fake_jpeg = b"\xFF\xD8\xFF\xE0fake jpeg content";
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();

        let chunks = pipeline
            .process(fake_jpeg, "image/jpeg", doc_id, ws_id, None)
            .await
            .unwrap();

        assert_eq!(chunks.len(), 1);
        let content = &chunks[0].content;
        assert!(
            content.contains("JPEG"),
            "Content should mention JPEG format, got: {content}"
        );
        // When dims are unknown the placeholder says "unknown"
        assert!(
            content.contains("unknown"),
            "Content should mention unknown dims, got: {content}"
        );

        let meta = chunks[0].metadata.as_ref().unwrap();
        assert_eq!(meta.mime_type.as_deref(), Some("image/jpeg"));
    }

    // ── PDF vision fallback ───────────────────────────────────────

    /// Mock vision-capable LLM that records how many times it was called.
    struct MockVisionLlm {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        reply: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockVisionLlm {
        async fn generate(
            &self,
            _messages: &[thairag_core::types::ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<thairag_core::types::LlmResponse> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(thairag_core::types::LlmResponse {
                content: self.reply.clone(),
                usage: Default::default(),
            })
        }

        fn model_name(&self) -> &str {
            "mock-vision"
        }

        fn supports_vision(&self) -> bool {
            true
        }

        async fn generate_vision(
            &self,
            _messages: &[thairag_core::types::VisionMessage],
            _max_tokens: Option<u32>,
        ) -> Result<thairag_core::types::LlmResponse> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(thairag_core::types::LlmResponse {
                content: self.reply.clone(),
                usage: Default::default(),
            })
        }
    }

    fn mock_vision_llm(
        reply: &str,
    ) -> (
        Arc<MockVisionLlm>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let llm = Arc::new(MockVisionLlm {
            calls: std::sync::Arc::clone(&calls),
            reply: reply.to_string(),
        });
        (llm, calls)
    }

    #[tokio::test]
    async fn pdf_vision_fallback_not_triggered_without_vision_llm() {
        // Without a vision LLM, a PDF goes through the standard path.
        // We use a garbage PDF so the standard path errors out — proving
        // we never reached the smart vision path.
        let pipeline = DocumentPipeline::new(1000, 0);
        let result = pipeline
            .process(
                b"not a pdf",
                "application/pdf",
                DocId::new(),
                WorkspaceId::new(),
                None,
            )
            .await;
        assert!(
            result.is_err(),
            "garbage PDF should error in mechanical path"
        );
    }

    #[tokio::test]
    async fn pdf_vision_fallback_disabled_by_config() {
        // With the fallback explicitly disabled, even a configured vision
        // LLM should not be invoked for PDFs.
        let (llm, calls) = mock_vision_llm("ignored");
        let pipeline = DocumentPipeline::new(1000, 0)
            .with_image_description(llm, true)
            .with_pdf_vision_fallback(false, 50, 100);
        let result = pipeline
            .process(
                b"not a pdf",
                "application/pdf",
                DocId::new(),
                WorkspaceId::new(),
                None,
            )
            .await;
        assert!(
            result.is_err(),
            "smart path off → mechanical path errors on bad PDF"
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "vision LLM must not be called when fallback disabled"
        );
    }

    #[tokio::test]
    async fn process_image_fails_loud_when_vision_required_but_unavailable() {
        // image_description_enabled is on but the only "LLM" we hand it
        // does not support vision — pipeline must surface a structured
        // EmptyExtraction instead of silently writing a placeholder.
        struct NonVisionLlm;
        #[async_trait::async_trait]
        impl LlmProvider for NonVisionLlm {
            async fn generate(
                &self,
                _: &[thairag_core::types::ChatMessage],
                _: Option<u32>,
            ) -> Result<thairag_core::types::LlmResponse> {
                unreachable!("must not be called")
            }
            fn model_name(&self) -> &str {
                "non-vision"
            }
            fn supports_vision(&self) -> bool {
                false
            }
        }
        let pipeline =
            DocumentPipeline::new(1000, 0).with_image_description(Arc::new(NonVisionLlm), true);

        let png = make_minimal_png(10, 10);
        let err = pipeline
            .process(&png, "image/png", DocId::new(), WorkspaceId::new(), None)
            .await
            .expect_err("image upload must fail loud when vision unavailable");
        match err {
            ThaiRagError::EmptyExtraction { reason, hint } => {
                assert_eq!(reason, empty_reason::NO_TEXT_VISION_UNAVAILABLE);
                assert!(
                    hint.contains("vision"),
                    "hint should explain vision requirement: {hint}"
                );
            }
            other => panic!("expected EmptyExtraction, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_image_without_description_falls_back_to_placeholder() {
        // image_description_enabled is OFF — operator opted out, so an
        // image upload should still succeed with a metadata placeholder.
        let pipeline = DocumentPipeline::new(1000, 0); // image_description disabled by default
        let png = make_minimal_png(10, 10);
        let chunks = pipeline
            .process(&png, "image/png", DocId::new(), WorkspaceId::new(), None)
            .await
            .expect("placeholder path must succeed when description is off");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("PNG"));
    }

    #[tokio::test]
    async fn pdf_max_vision_pages_zero_skips_all_vision_calls() {
        // Even with vision configured, a cap of 0 must short-circuit every
        // page. Uses a malformed PDF; the unfiltered extractor returns
        // an error before we'd ever rasterize, but the contract we're
        // asserting is that the vision LLM is never called.
        let (llm, calls) = mock_vision_llm("never");
        let pipeline = DocumentPipeline::new(1000, 0)
            .with_image_description(llm, true)
            .with_pdf_vision_fallback(true, 50, 0);
        let _ = pipeline
            .process(
                b"%PDF-bogus",
                "application/pdf",
                DocId::new(),
                WorkspaceId::new(),
                None,
            )
            .await;
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "pdf_max_vision_pages=0 must prevent every vision call"
        );
    }

    #[test]
    fn with_pdf_vision_fallback_sets_fields() {
        let pipeline = DocumentPipeline::new(1000, 0).with_pdf_vision_fallback(true, 75, 42);
        assert!(pipeline.pdf_vision_fallback_enabled);
        assert_eq!(pipeline.pdf_min_chars_per_page, 75);
        assert_eq!(pipeline.pdf_max_vision_pages, 42);
    }
}
