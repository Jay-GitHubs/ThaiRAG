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
