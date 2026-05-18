//! Post-retrieval expansion for the small-to-big retrieval strategies.
//!
//! Implements the read side of the "metadata-replacement" pattern: a search
//! result that was indexed as a small unit (sentence / child chunk) carries
//! its larger context in [`ChunkMetadata`]. [`expand_results`] swaps the small
//! `content` for that larger text before the results reach the context
//! curator, and collapses child chunks that share a parent.
//!
//! Standard chunks — those without window/parent metadata — pass through
//! untouched, so a mixed corpus (some documents reindexed, some not) is safe.

use std::collections::HashSet;

use thairag_core::types::SearchResult;

/// Expand retrieved results per the metadata-replacement pattern.
///
/// - Sentence-window: when `metadata.window_text` is set, `content` is
///   replaced with the neighbour window.
/// - Parent-document: when `metadata.parent_content` is set, `content` is
///   replaced with the parent text; results sharing a `parent_id` are
///   deduped, keeping the highest-scoring child (input order is preserved
///   otherwise, so callers should pass results already sorted by score).
/// - Standard chunks pass through unchanged.
pub fn expand_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen_parents: HashSet<String> = HashSet::new();
    let mut expanded = Vec::with_capacity(results.len());

    for mut result in results {
        let Some(meta) = result.chunk.metadata.as_ref() else {
            expanded.push(result);
            continue;
        };

        // Parent-document: dedupe by parent, swap in the parent text.
        if let Some(parent_id) = meta.parent_id.clone() {
            if !seen_parents.insert(parent_id) {
                // A higher-scoring sibling already represented this parent.
                continue;
            }
            if let Some(parent_content) = meta.parent_content.clone() {
                result.chunk.content = parent_content;
            }
            expanded.push(result);
            continue;
        }

        // Sentence-window: swap in the neighbour window.
        if let Some(window_text) = meta.window_text.clone() {
            result.chunk.content = window_text;
        }
        expanded.push(result);
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{ChunkId, ChunkMetadata, DocId, DocumentChunk, WorkspaceId};

    fn result(content: &str, score: f32, metadata: Option<ChunkMetadata>) -> SearchResult {
        SearchResult {
            chunk: DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id: DocId::new(),
                workspace_id: WorkspaceId::new(),
                content: content.to_string(),
                chunk_index: 0,
                embedding: None,
                metadata,
            },
            score,
        }
    }

    #[test]
    fn standard_chunk_passes_through_unchanged() {
        let out = expand_results(vec![result("plain", 0.9, None)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk.content, "plain");
    }

    #[test]
    fn sentence_window_swaps_content_to_window_text() {
        let meta = ChunkMetadata {
            window_text: Some("prev. THIS. next.".to_string()),
            ..Default::default()
        };
        let out = expand_results(vec![result("THIS.", 0.8, Some(meta))]);
        assert_eq!(out[0].chunk.content, "prev. THIS. next.");
    }

    #[test]
    fn window_text_none_falls_back_to_content() {
        let meta = ChunkMetadata::default();
        let out = expand_results(vec![result("kept", 0.5, Some(meta))]);
        assert_eq!(out[0].chunk.content, "kept");
    }

    #[test]
    fn parent_swaps_content_to_parent_content() {
        let meta = ChunkMetadata {
            parent_id: Some("p1".to_string()),
            parent_content: Some("FULL PARENT TEXT".to_string()),
            ..Default::default()
        };
        let out = expand_results(vec![result("child", 0.7, Some(meta))]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk.content, "FULL PARENT TEXT");
    }

    #[test]
    fn parent_dedupes_children_of_same_parent() {
        let mk = |c: &str, s: f32| {
            result(
                c,
                s,
                Some(ChunkMetadata {
                    parent_id: Some("p1".to_string()),
                    parent_content: Some("PARENT".to_string()),
                    ..Default::default()
                }),
            )
        };
        // Three children of one parent (already sorted by score desc).
        let out = expand_results(vec![mk("c1", 0.9), mk("c2", 0.6), mk("c3", 0.3)]);
        assert_eq!(out.len(), 1, "one parent → one result");
        assert_eq!(out[0].chunk.content, "PARENT");
        assert_eq!(out[0].score, 0.9, "keeps the highest-scoring child");
    }

    #[test]
    fn distinct_parents_both_survive() {
        let mk = |pid: &str, s: f32| {
            result(
                "child",
                s,
                Some(ChunkMetadata {
                    parent_id: Some(pid.to_string()),
                    parent_content: Some(format!("PARENT-{pid}")),
                    ..Default::default()
                }),
            )
        };
        let out = expand_results(vec![mk("a", 0.9), mk("b", 0.8)]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn mixed_results_each_handled() {
        let standard = result("std", 0.95, None);
        let window = result(
            "sent",
            0.9,
            Some(ChunkMetadata {
                window_text: Some("WINDOW".to_string()),
                ..Default::default()
            }),
        );
        let parent = result(
            "child",
            0.85,
            Some(ChunkMetadata {
                parent_id: Some("p".to_string()),
                parent_content: Some("PARENT".to_string()),
                ..Default::default()
            }),
        );
        let out = expand_results(vec![standard, window, parent]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].chunk.content, "std");
        assert_eq!(out[1].chunk.content, "WINDOW");
        assert_eq!(out[2].chunk.content, "PARENT");
    }

    #[test]
    fn empty_input() {
        assert!(expand_results(Vec::new()).is_empty());
    }
}
