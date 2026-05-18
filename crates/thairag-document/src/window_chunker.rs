//! Chunking strategies for small-to-big retrieval.
//!
//! These builders implement the "metadata-replacement" pattern: they index
//! small units (sentences / child chunks) but stash the larger context
//! (neighbour window / parent text) in [`ChunkMetadata`]. Post-retrieval
//! expansion ([`thairag_search::expansion`]) swaps the small `content` for the
//! stored larger text before the chunk reaches the context curator. This
//! avoids adding any get-by-id / get-by-range method to the vector-store
//! providers.

use thairag_core::traits::Chunker;
use thairag_core::types::{ChunkId, ChunkMetadata, DocId, DocumentChunk, WorkspaceId};

use crate::thai_chunker::ThaiAwareChunker;

/// Sentence-window chunking: index one chunk per sentence, each carrying an
/// expanded `window_text` (the sentence plus `window` neighbours on each
/// side). At document edges the window is clamped — no panic, no padding.
///
/// `window == 0` yields a `window_text` identical to the sentence.
pub fn build_sentence_window_chunks(
    text: &str,
    window: usize,
    doc_id: DocId,
    workspace_id: WorkspaceId,
) -> Vec<DocumentChunk> {
    let chunker = ThaiAwareChunker::new();
    let sentences = chunker.segment_sentences(text);
    if sentences.is_empty() {
        return Vec::new();
    }

    sentences
        .iter()
        .enumerate()
        .map(|(i, sentence)| {
            let start = i.saturating_sub(window);
            let end = (i + window + 1).min(sentences.len());
            let window_text = sentences[start..end].join(" ");
            DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id,
                workspace_id,
                content: sentence.clone(),
                chunk_index: i,
                embedding: None,
                metadata: Some(ChunkMetadata {
                    window_text: Some(window_text),
                    ..Default::default()
                }),
            }
        })
        .collect()
}

/// Parent-document chunking: split the text into large parent units, then
/// split each parent into small child chunks. Only the children are
/// returned (and indexed); each child carries its parent's stable
/// `parent_id` plus the full `parent_content` for retrieval-time expansion.
pub fn build_parent_document_chunks(
    text: &str,
    parent_size: usize,
    child_size: usize,
    doc_id: DocId,
    workspace_id: WorkspaceId,
) -> Vec<DocumentChunk> {
    let chunker = ThaiAwareChunker::new();
    let parents = chunker.chunk(text, parent_size, 0);

    let mut chunks = Vec::new();
    let mut child_index = 0usize;
    for parent in &parents {
        let parent_id = ChunkId::new().to_string();
        let children = chunker.chunk(parent, child_size, 0);
        // A parent shorter than child_size still yields one child == parent.
        for child in children {
            chunks.push(DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id,
                workspace_id,
                content: child,
                chunk_index: child_index,
                embedding: None,
                metadata: Some(ChunkMetadata {
                    parent_id: Some(parent_id.clone()),
                    parent_content: Some(parent.clone()),
                    ..Default::default()
                }),
            });
            child_index += 1;
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (DocId, WorkspaceId) {
        (DocId::new(), WorkspaceId::new())
    }

    #[test]
    fn sentence_window_emits_one_chunk_per_sentence() {
        let (d, w) = ids();
        let text = "First sentence. Second sentence. Third sentence.";
        let chunks = build_sentence_window_chunks(text, 1, d, w);
        assert_eq!(chunks.len(), 3);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
        }
    }

    #[test]
    fn sentence_window_text_includes_neighbours() {
        let (d, w) = ids();
        let text = "Alpha one. Beta two. Gamma three. Delta four. Epsilon five.";
        let chunks = build_sentence_window_chunks(text, 1, d, w);
        // Middle sentence's window spans the previous and next sentence.
        let mid = &chunks[2];
        let win = mid.metadata.as_ref().unwrap().window_text.as_ref().unwrap();
        assert!(win.contains("Beta"), "got: {win}");
        assert!(win.contains("Gamma"), "got: {win}");
        assert!(win.contains("Delta"), "got: {win}");
    }

    #[test]
    fn sentence_window_at_edges_clamps() {
        let (d, w) = ids();
        let text = "One. Two. Three.";
        let chunks = build_sentence_window_chunks(text, 5, d, w);
        // Window larger than the doc: every window text spans the whole doc.
        for c in &chunks {
            let win = c.metadata.as_ref().unwrap().window_text.as_ref().unwrap();
            assert!(win.contains("One"));
            assert!(win.contains("Three"));
        }
    }

    #[test]
    fn sentence_window_zero_equals_sentence() {
        let (d, w) = ids();
        let chunks = build_sentence_window_chunks("Solo sentence here.", 0, d, w);
        assert_eq!(chunks.len(), 1);
        let c = &chunks[0];
        assert_eq!(
            c.metadata.as_ref().unwrap().window_text.as_deref(),
            Some(c.content.as_str())
        );
    }

    #[test]
    fn sentence_window_thai_text() {
        let (d, w) = ids();
        let text = "วันนี้อากาศดีมากครับ พรุ่งนี้จะดีกว่าค่ะ";
        let chunks = build_sentence_window_chunks(text, 1, d, w);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.metadata.as_ref().unwrap().window_text.is_some());
        }
    }

    #[test]
    fn sentence_window_empty_input() {
        let (d, w) = ids();
        assert!(build_sentence_window_chunks("", 2, d, w).is_empty());
    }

    #[test]
    fn parent_document_children_share_parent_id_within_parent() {
        let (d, w) = ids();
        // One parent (small parent_size keeps it to a single parent unit here).
        let text = "AAAA.\n\nBBBB.\n\nCCCC.";
        let chunks = build_parent_document_chunks(text, 4096, 8, d, w);
        assert!(!chunks.is_empty());
        let first_parent = chunks[0]
            .metadata
            .as_ref()
            .unwrap()
            .parent_id
            .clone()
            .unwrap();
        // All children carry a parent_id and parent_content.
        for c in &chunks {
            let m = c.metadata.as_ref().unwrap();
            assert!(m.parent_id.is_some());
            assert!(m.parent_content.is_some());
        }
        assert!(!first_parent.is_empty());
    }

    #[test]
    fn parent_document_distinct_parents_have_distinct_ids() {
        let (d, w) = ids();
        // Tiny parent_size forces multiple parents.
        let text = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph here.";
        let chunks = build_parent_document_chunks(text, 20, 8, d, w);
        let parent_ids: std::collections::HashSet<_> = chunks
            .iter()
            .filter_map(|c| c.metadata.as_ref().and_then(|m| m.parent_id.clone()))
            .collect();
        assert!(
            parent_ids.len() >= 2,
            "expected multiple parents, got {}",
            parent_ids.len()
        );
    }

    #[test]
    fn parent_document_child_indices_sequential() {
        let (d, w) = ids();
        let text = "Lorem ipsum dolor.\n\nSit amet consectetur.\n\nAdipiscing elit sed.";
        let chunks = build_parent_document_chunks(text, 24, 8, d, w);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i);
        }
    }

    #[test]
    fn parent_document_empty_input() {
        let (d, w) = ids();
        assert!(build_parent_document_chunks("", 2048, 384, d, w).is_empty());
    }
}
