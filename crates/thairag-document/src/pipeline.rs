use std::sync::Arc;

use thairag_config::schema::AiPreprocessingConfig;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{Chunker, DocumentProcessor, LlmProvider};
use thairag_core::types::{ChunkId, DocId, DocumentChunk, WorkspaceId};

use crate::ai::pipeline::AiDocumentPipeline;
use crate::chunker::MarkdownChunker;
use crate::converter::MarkdownConverter;

/// Callback invoked when the pipeline enters a new processing step.
/// Steps: "analyzing", "converting", "checking_quality", "chunking", "indexing".
pub type StepCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Orchestrates: convert raw bytes → chunk text → produce DocumentChunks.
/// When AI preprocessing is enabled, delegates to the AI agent team.
pub struct DocumentPipeline {
    converter: MarkdownConverter,
    chunker: MarkdownChunker,
    max_chunk_size: usize,
    chunk_overlap: usize,
    ai_pipeline: Option<AiDocumentPipeline>,
}

impl DocumentPipeline {
    /// Create a mechanical-only pipeline (no AI).
    pub fn new(max_chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            converter: MarkdownConverter::new(),
            chunker: MarkdownChunker::new(),
            max_chunk_size,
            chunk_overlap,
            ai_pipeline: None,
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

        Self {
            converter: MarkdownConverter::new(),
            chunker: MarkdownChunker::new(),
            max_chunk_size,
            chunk_overlap,
            ai_pipeline,
        }
    }

    /// Process document bytes into chunks.
    /// If AI preprocessing is enabled, uses the AI agent team.
    /// Otherwise, uses the mechanical pipeline.
    /// The optional `on_step` callback is invoked at each processing stage.
    pub async fn process(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
        on_step: Option<StepCallback>,
    ) -> Result<Vec<DocumentChunk>> {
        if let Some(ai) = &self.ai_pipeline {
            ai.process(raw, mime_type, doc_id, workspace_id, on_step)
                .await
        } else {
            self.process_mechanical(raw, mime_type, doc_id, workspace_id)
        }
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
}
