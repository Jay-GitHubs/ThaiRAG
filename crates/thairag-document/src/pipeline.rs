use std::sync::Arc;

use thairag_config::schema::AiPreprocessingConfig;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{Chunker, DocumentProcessor, LlmProvider};
use thairag_core::types::{
    ChunkId, ChunkMetadata, DocId, DocumentChunk, DocumentContentType, WorkspaceId,
};
use tracing::info;

use crate::ai::pipeline::AiDocumentPipeline;
use crate::chunker::MarkdownChunker;
use crate::converter::MarkdownConverter;
use crate::image;
use crate::table_extractor;
use crate::thai_chunker::ThaiAwareChunker;

/// Callback invoked when the pipeline enters a new processing step.
/// Steps: "analyzing", "converting", "checking_quality", "chunking", "indexing".
pub type StepCallback = Arc<dyn Fn(&str) + Send + Sync>;

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

    /// Set whether table extraction is enabled.
    pub fn with_table_extraction(mut self, enabled: bool) -> Self {
        self.table_extraction_enabled = enabled;
        self
    }

    /// Process document bytes into chunks.
    /// If AI preprocessing is enabled, uses the AI agent team.
    /// Otherwise, uses the mechanical pipeline.
    /// The optional `on_step` callback is invoked at each processing stage.
    ///
    /// Supports multi-modal content:
    /// - Image files are described via LLM vision (if enabled) and stored as text chunks.
    /// - Tables in text/PDF content are extracted and appended as structured markdown chunks.
    pub async fn process(
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

        let mut chunks = if let Some(ai) = &self.ai_pipeline {
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

        Ok(chunks)
    }

    /// Mechanical pipeline: convert → chunk → produce DocumentChunks.
    pub fn process_mechanical(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let text = self.converter.convert(raw, mime_type)?;
        let chunks = self
            .chunker
            .chunk(&text, self.max_chunk_size, self.chunk_overlap);

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

    /// Process an image file: generate a text description and return as a single chunk.
    async fn process_image(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let description = if self.image_description_enabled {
            if let Some(llm) = &self.vision_llm {
                image::describe_image(llm.as_ref(), raw, mime_type).await?
            } else {
                // No LLM configured — use metadata placeholder
                let meta = image::extract_image_metadata(raw, mime_type);
                image::format_placeholder_description(&meta)
            }
        } else {
            // Image description disabled — use metadata placeholder
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
    fn process_empty_input() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let chunks = pipeline
            .process_mechanical(b"", "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert!(chunks.is_empty());
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
}
