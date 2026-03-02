use thairag_core::types::{ChunkId, DocId, DocumentChunk, WorkspaceId};
use thairag_core::error::Result;

use crate::chunker::MarkdownChunker;
use crate::converter::MarkdownConverter;
use thairag_core::traits::{Chunker, DocumentProcessor};

/// Orchestrates: convert raw bytes → chunk text → produce DocumentChunks.
pub struct DocumentPipeline {
    converter: MarkdownConverter,
    chunker: MarkdownChunker,
    max_chunk_size: usize,
    chunk_overlap: usize,
}

impl DocumentPipeline {
    pub fn new(max_chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            converter: MarkdownConverter::new(),
            chunker: MarkdownChunker::new(),
            max_chunk_size,
            chunk_overlap,
        }
    }

    pub fn process(
        &self,
        raw: &[u8],
        mime_type: &str,
        doc_id: DocId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DocumentChunk>> {
        let text = self.converter.convert(raw, mime_type)?;
        let chunks = self.chunker.chunk(&text, self.max_chunk_size, self.chunk_overlap);

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
            .process(b"Hello world", "text/plain", doc_id, ws_id)
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
            .process(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        // All paragraphs fit within 1000 chars, so should be 1 chunk
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Paragraph one."));
        assert!(chunks[0].content.contains("Paragraph three."));
    }

    #[test]
    fn process_splits_at_max_chunk_size() {
        let pipeline = DocumentPipeline::new(30, 0);
        let text = b"Short one.\n\nAnother paragraph here.";
        let chunks = pipeline
            .process(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn process_empty_input() {
        let pipeline = DocumentPipeline::new(1000, 0);
        let chunks = pipeline
            .process(b"", "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn process_unique_chunk_ids() {
        let pipeline = DocumentPipeline::new(20, 0);
        let text = b"AAA\n\nBBB\n\nCCC";
        let chunks = pipeline
            .process(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        let ids: std::collections::HashSet<_> =
            chunks.iter().map(|c| c.chunk_id).collect();
        assert_eq!(ids.len(), chunks.len());
    }

    #[test]
    fn process_chunk_indices_sequential() {
        let pipeline = DocumentPipeline::new(10, 0);
        let text = b"AA\n\nBB\n\nCC";
        let chunks = pipeline
            .process(text, "text/plain", DocId::new(), WorkspaceId::new())
            .unwrap();
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i);
        }
    }
}
