//! Reasoning-based ("PageIndex") retrieval.
//!
//! Instead of embedding the query and ranking by vector/lexical similarity, this
//! retriever hands an LLM the *table-of-contents trees* of the in-scope documents
//! (section titles + one-line summaries, never the full text) and asks it to
//! **navigate** to the sections that answer the question. The selected nodes are
//! mapped back to their chunks (by page range, falling back to section title) and
//! returned as ordinary [`SearchResult`]s, so the rest of the pipeline — answer
//! generation, citations — is unchanged.
//!
//! It is store-agnostic: the trees and chunks arrive via injected closures
//! ([`TreeResolver`], [`ChunkResolver`]), mirroring the pipeline's other
//! resolvers. When it can't produce anything (no trees in scope, navigation
//! yields nothing, or no node maps to a chunk) it returns an empty vec and the
//! caller falls back to lexical retrieval.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Deserialize;
use tracing::{debug, warn};

use thairag_core::error::Result;
use thairag_core::models::{DocTree, DocTreeNode};
use thairag_core::traits::LlmProvider;
use thairag_core::types::{
    ChatMessage, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId,
};

/// Resolves the document trees available in a set of workspaces.
pub type TreeResolver = Arc<dyn Fn(&[WorkspaceId]) -> Vec<DocTree> + Send + Sync>;
/// Resolves a document's chunks (in `chunk_index` order).
pub type ChunkResolver = Arc<dyn Fn(DocId) -> Vec<DocumentChunk> + Send + Sync>;

/// Hard cap on chunks returned, so a broad selection can't dump an entire
/// corpus into context (the curator trims further by token budget downstream).
const MAX_RESULT_CHUNKS: usize = 60;

/// LLM-navigated retriever over per-document PageIndex trees.
pub struct ReasoningRetriever {
    nav_llm: Arc<dyn LlmProvider>,
    tree_resolver: TreeResolver,
    chunk_resolver: ChunkResolver,
    /// Max in-scope docs whose trees are offered to the navigator.
    max_docs: usize,
    /// Max nodes the navigator may select.
    max_nodes: usize,
    /// Output-token cap for the navigation call.
    max_tokens: u32,
}

impl ReasoningRetriever {
    pub fn new(
        nav_llm: Arc<dyn LlmProvider>,
        tree_resolver: TreeResolver,
        chunk_resolver: ChunkResolver,
        max_docs: usize,
        max_nodes: usize,
        max_tokens: u32,
    ) -> Self {
        Self {
            nav_llm,
            tree_resolver,
            chunk_resolver,
            max_docs: max_docs.max(1),
            max_nodes: max_nodes.max(1),
            max_tokens,
        }
    }

    /// Retrieve by reasoning over the in-scope documents' trees. Returns an empty
    /// vec (not an error) when nothing can be produced, so the caller can fall
    /// back to lexical retrieval.
    pub async fn retrieve(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // Candidate trees: those in the query's workspaces, narrowed to the
        // deterministic doc prefilter (`query.doc_ids`) when present, then capped.
        let mut trees = (self.tree_resolver)(&query.workspace_ids);
        if !query.doc_ids.is_empty() {
            let keep: HashSet<DocId> = query.doc_ids.iter().copied().collect();
            trees.retain(|t| keep.contains(&t.doc_id));
        }
        trees.truncate(self.max_docs);
        if trees.is_empty() {
            debug!("reasoning retrieval: no trees in scope; caller will fall back");
            return Ok(Vec::new());
        }

        // Build the navigation outline (compact: short doc ids + node titles +
        // summaries) and call the navigator.
        let outline = build_outline(&trees);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: build_nav_prompt(&query.text, &outline, self.max_nodes),
            images: vec![],
        }];
        let response = self
            .nav_llm
            .generate_structured(
                &messages,
                Some(self.max_tokens),
                &nav_schema(self.max_nodes),
            )
            .await?;
        let json_str = strip_json_fences(response.content.trim());
        let nav: NavResult = match serde_json::from_str(json_str) {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "reasoning retrieval: navigator JSON parse failed");
                return Ok(Vec::new());
            }
        };
        if nav.selected_node_ids.is_empty() {
            debug!("reasoning retrieval: navigator selected no nodes");
            return Ok(Vec::new());
        }

        // Map each selected "d{i}:{node_id}" key back to its node, then to chunks.
        let mut results: Vec<SearchResult> = Vec::new();
        let mut seen_chunks: HashSet<String> = HashSet::new();
        let mut chunk_cache: std::collections::HashMap<DocId, Vec<DocumentChunk>> =
            std::collections::HashMap::new();

        for key in nav.selected_node_ids.iter().take(self.max_nodes) {
            let Some((doc_idx, node_id)) = parse_key(key) else {
                continue;
            };
            let Some(tree) = trees.get(doc_idx) else {
                continue;
            };
            let Some(node) = find_node(&tree.root, node_id) else {
                continue;
            };

            let chunks = chunk_cache
                .entry(tree.doc_id)
                .or_insert_with(|| (self.chunk_resolver)(tree.doc_id));
            let matched = chunks_for_node(node, chunks);

            for chunk in matched {
                let cid = chunk.chunk_id.to_string();
                if !seen_chunks.insert(cid) {
                    continue; // already emitted via another node
                }
                // Descending synthetic score by emission order — preserves the
                // navigator's relevance ordering for downstream ranking.
                let score = 1.0 / (1.0 + results.len() as f32);
                results.push(SearchResult {
                    chunk: chunk.clone(),
                    score,
                });
                if results.len() >= MAX_RESULT_CHUNKS {
                    return Ok(results);
                }
            }
        }

        Ok(results)
    }
}

/// Navigator output. `reasoning` is captured for logs/telemetry but unused here.
#[derive(Debug, Deserialize)]
struct NavResult {
    #[serde(default)]
    selected_node_ids: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    reasoning: String,
}

fn nav_schema(max_nodes: usize) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "selected_node_ids": {
                "type": "array",
                "items": { "type": "string", "maxLength": 64 },
                "maxItems": max_nodes
            },
            "reasoning": { "type": "string", "maxLength": 400 }
        },
        "required": ["selected_node_ids"]
    })
}

fn build_nav_prompt(query: &str, outline: &str, max_nodes: usize) -> String {
    format!(
        "You are navigating a set of documents to answer a question. Below is a \
table of contents for each in-scope document: every section has an id, a title, \
and a one-line summary.\n\n\
Choose the sections whose content is most likely to answer the question. Return \
their ids in \"selected_node_ids\", most relevant first, at most {max_nodes}. \
Pick across documents if needed; pick only sections you believe are relevant — \
do not pad the list. Output JSON only.\n\n\
Question: {query}\n\n\
=== DOCUMENT OUTLINES ===\n{outline}"
    )
}

/// Render the candidate trees as a compact outline keyed by short doc ids
/// (`d0`, `d1`, …) so the navigator references nodes as `d0:n0.1`.
fn build_outline(trees: &[DocTree]) -> String {
    let mut out = String::new();
    for (i, tree) in trees.iter().enumerate() {
        out.push_str(&format!("[d{i}] {}", tree.title.trim()));
        let root_summary = tree.root.summary.trim();
        if !root_summary.is_empty() {
            out.push_str(&format!(" — {root_summary}"));
        }
        out.push('\n');
        for child in &tree.root.children {
            append_node_lines(&mut out, i, child, 1);
        }
        out.push('\n');
    }
    out
}

fn append_node_lines(out: &mut String, doc_idx: usize, node: &DocTreeNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let pages = match (node.page_start, node.page_end) {
        (Some(s), Some(e)) if e > s => format!(" (pp {s}\u{2013}{e})"),
        (Some(s), _) => format!(" (p {s})"),
        _ => String::new(),
    };
    let summary = node.summary.trim();
    let summary = if summary.is_empty() {
        String::new()
    } else {
        format!(" — {summary}")
    };
    out.push_str(&format!(
        "{indent}d{doc_idx}:{} {}{}{}\n",
        node.node_id,
        node.title.trim(),
        summary,
        pages
    ));
    for child in &node.children {
        append_node_lines(out, doc_idx, child, depth + 1);
    }
}

/// Parse a navigator key `"d{idx}:{node_id}"` into `(doc_index, node_id)`.
fn parse_key(key: &str) -> Option<(usize, &str)> {
    let key = key.trim();
    let rest = key.strip_prefix('d')?;
    let (idx_str, node_id) = rest.split_once(':')?;
    let idx: usize = idx_str.parse().ok()?;
    if node_id.is_empty() {
        return None;
    }
    Some((idx, node_id))
}

/// Find a node by its (tree-unique) `node_id`, searching the whole subtree.
fn find_node<'a>(root: &'a DocTreeNode, node_id: &str) -> Option<&'a DocTreeNode> {
    if root.node_id == node_id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_node(child, node_id) {
            return Some(found);
        }
    }
    None
}

/// Effective page span of a node: its own range, or the min/max over its subtree
/// when the node itself carries no pages.
fn node_page_span(node: &DocTreeNode) -> Option<(usize, usize)> {
    let mut start: Option<usize> = node.page_start;
    let mut end: Option<usize> = node.page_end.or(node.page_start);
    for child in &node.children {
        if let Some((cs, ce)) = node_page_span(child) {
            start = Some(start.map_or(cs, |s| s.min(cs)));
            end = Some(end.map_or(ce, |e| e.max(ce)));
        }
    }
    match (start, end) {
        (Some(s), Some(e)) => Some((s, e.max(s))),
        _ => None,
    }
}

/// Select the chunks belonging to a node: those whose page numbers fall in the
/// node's span; if none match (or the node has no pages), fall back to chunks
/// whose `section_title` equals the node title.
fn chunks_for_node<'a>(node: &DocTreeNode, chunks: &'a [DocumentChunk]) -> Vec<&'a DocumentChunk> {
    if let Some((s, e)) = node_page_span(node) {
        let by_page: Vec<&DocumentChunk> = chunks
            .iter()
            .filter(|c| {
                c.metadata
                    .as_ref()
                    .and_then(|m| m.page_numbers.as_ref())
                    .is_some_and(|pages| pages.iter().any(|p| *p >= s && *p <= e))
            })
            .collect();
        if !by_page.is_empty() {
            return by_page;
        }
    }
    let title = node.title.trim();
    if title.is_empty() {
        return Vec::new();
    }
    chunks
        .iter()
        .filter(|c| {
            c.metadata
                .as_ref()
                .and_then(|m| m.section_title.as_deref())
                .map(str::trim)
                == Some(title)
        })
        .collect()
}

/// Strip ```json fences a model may wrap structured output in.
fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s);
    s.strip_suffix("```").unwrap_or(s).trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use thairag_core::types::{ChunkId, ChunkMetadata, LlmResponse, LlmUsage};

    struct StubLlm {
        reply: String,
        prompt: Arc<Mutex<Option<String>>>,
    }
    #[async_trait::async_trait]
    impl LlmProvider for StubLlm {
        async fn generate(&self, _m: &[ChatMessage], _t: Option<u32>) -> Result<LlmResponse> {
            unreachable!("nav uses generate_structured")
        }
        async fn generate_structured(
            &self,
            messages: &[ChatMessage],
            _t: Option<u32>,
            _schema: &serde_json::Value,
        ) -> Result<LlmResponse> {
            *self.prompt.lock().unwrap() = Some(messages[0].content.clone());
            Ok(LlmResponse {
                content: self.reply.clone(),
                usage: LlmUsage::default(),
            })
        }
        fn model_name(&self) -> &str {
            "stub"
        }
    }

    fn node(
        id: &str,
        title: &str,
        ps: Option<usize>,
        pe: Option<usize>,
        kids: Vec<DocTreeNode>,
    ) -> DocTreeNode {
        DocTreeNode {
            node_id: id.into(),
            title: title.into(),
            summary: format!("summary of {title}"),
            page_start: ps,
            page_end: pe,
            children: kids,
        }
    }

    fn tree(doc_id: DocId, title: &str, children: Vec<DocTreeNode>) -> DocTree {
        DocTree {
            doc_id,
            title: title.into(),
            root: node("n0", title, None, None, children),
            model_name: None,
        }
    }

    fn chunk(
        doc_id: DocId,
        idx: usize,
        content: &str,
        pages: Option<Vec<usize>>,
        section: Option<&str>,
    ) -> DocumentChunk {
        DocumentChunk {
            chunk_id: ChunkId::new(),
            doc_id,
            workspace_id: WorkspaceId::new(),
            content: content.into(),
            chunk_index: idx,
            embedding: None,
            metadata: Some(ChunkMetadata {
                page_numbers: pages,
                section_title: section.map(|s| s.to_string()),
                ..Default::default()
            }),
        }
    }

    fn retriever(
        reply: &str,
        trees: Vec<DocTree>,
        chunks: Vec<DocumentChunk>,
    ) -> (ReasoningRetriever, Arc<Mutex<Option<String>>>) {
        let prompt = Arc::new(Mutex::new(None));
        let llm = Arc::new(StubLlm {
            reply: reply.into(),
            prompt: Arc::clone(&prompt),
        });
        let tree_resolver: TreeResolver = Arc::new(move |_ws: &[WorkspaceId]| trees.clone());
        let chunk_resolver: ChunkResolver = Arc::new(move |doc: DocId| {
            chunks.iter().filter(|c| c.doc_id == doc).cloned().collect()
        });
        (
            ReasoningRetriever::new(llm, tree_resolver, chunk_resolver, 5, 12, 1024),
            prompt,
        )
    }

    fn query(ws: WorkspaceId, doc_ids: Vec<DocId>) -> SearchQuery {
        SearchQuery {
            text: "what is the credit limit?".into(),
            top_k: 10,
            workspace_ids: vec![ws],
            unrestricted: false,
            query_images: vec![],
            doc_ids,
        }
    }

    #[tokio::test]
    async fn maps_selected_nodes_to_page_matched_chunks() {
        let ws = WorkspaceId::new();
        let doc = DocId::new();
        let t = tree(
            doc,
            "SME Loan",
            vec![
                node("n0.0", "Overview", Some(1), Some(1), vec![]),
                node("n0.1", "Limits", Some(2), Some(3), vec![]),
            ],
        );
        let chunks = vec![
            chunk(doc, 0, "intro text", Some(vec![1]), Some("Overview")),
            chunk(doc, 1, "limit is 10M", Some(vec![2]), Some("Limits")),
            chunk(doc, 2, "more limits", Some(vec![3]), Some("Limits")),
        ];
        // Navigator picks the Limits section only.
        let (rr, prompt) = retriever(
            r#"{"selected_node_ids":["d0:n0.1"],"reasoning":"limits answer it"}"#,
            vec![t],
            chunks,
        );
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.chunk.content.contains("limit")));
        // Descending scores.
        assert!(out[0].score > out[1].score);
        // The outline used short doc ids and included the node summaries.
        let p = prompt.lock().unwrap().clone().unwrap();
        assert!(p.contains("d0:n0.1"));
        assert!(p.contains("summary of Limits"));
    }

    #[tokio::test]
    async fn falls_back_to_section_title_when_no_pages() {
        let ws = WorkspaceId::new();
        let doc = DocId::new();
        // Node has no page range; chunk has no page numbers — match by section.
        let t = tree(
            doc,
            "Doc",
            vec![node("n0.0", "Eligibility", None, None, vec![])],
        );
        let chunks = vec![
            chunk(doc, 0, "who qualifies", None, Some("Eligibility")),
            chunk(doc, 1, "unrelated", None, Some("Other")),
        ];
        let (rr, _) = retriever(r#"{"selected_node_ids":["d0:n0.0"]}"#, vec![t], chunks);
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk.content, "who qualifies");
    }

    #[tokio::test]
    async fn dedupes_chunks_across_selected_nodes() {
        let ws = WorkspaceId::new();
        let doc = DocId::new();
        // Parent spans pages 1-2 (subtree), child spans page 1 — both select the
        // page-1 chunk, which must appear once.
        let t = tree(
            doc,
            "Doc",
            vec![node(
                "n0.0",
                "Parent",
                None,
                None,
                vec![node("n0.0.0", "Child", Some(1), Some(1), vec![])],
            )],
        );
        let chunks = vec![chunk(doc, 0, "shared", Some(vec![1]), None)];
        let (rr, _) = retriever(
            r#"{"selected_node_ids":["d0:n0.0","d0:n0.0.0"]}"#,
            vec![t],
            chunks,
        );
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn no_trees_returns_empty_for_lexical_fallback() {
        let ws = WorkspaceId::new();
        let (rr, _) = retriever(r#"{"selected_node_ids":["d0:n0.0"]}"#, vec![], vec![]);
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn empty_selection_returns_empty() {
        let ws = WorkspaceId::new();
        let doc = DocId::new();
        let t = tree(
            doc,
            "Doc",
            vec![node("n0.0", "S", Some(1), Some(1), vec![])],
        );
        let (rr, _) = retriever(r#"{"selected_node_ids":[]}"#, vec![t], vec![]);
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn doc_prefilter_narrows_candidate_trees() {
        let ws = WorkspaceId::new();
        let doc_a = DocId::new();
        let doc_b = DocId::new();
        let ta = tree(
            doc_a,
            "A",
            vec![node("n0.0", "SA", Some(1), Some(1), vec![])],
        );
        let tb = tree(
            doc_b,
            "B",
            vec![node("n0.0", "SB", Some(1), Some(1), vec![])],
        );
        let chunks = vec![
            chunk(doc_a, 0, "from A", Some(vec![1]), None),
            chunk(doc_b, 0, "from B", Some(vec![1]), None),
        ];
        // Prefilter to doc_b only; navigator references d0 (the sole survivor).
        let (rr, prompt) = retriever(r#"{"selected_node_ids":["d0:n0.0"]}"#, vec![ta, tb], chunks);
        let out = rr.retrieve(&query(ws, vec![doc_b])).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk.content, "from B");
        let p = prompt.lock().unwrap().clone().unwrap();
        assert!(p.contains("[d0] B"));
        assert!(!p.contains("[d1]"));
    }

    #[tokio::test]
    async fn unknown_or_malformed_keys_are_ignored() {
        let ws = WorkspaceId::new();
        let doc = DocId::new();
        let t = tree(
            doc,
            "Doc",
            vec![node("n0.0", "S", Some(1), Some(1), vec![])],
        );
        let chunks = vec![chunk(doc, 0, "c", Some(vec![1]), None)];
        let (rr, _) = retriever(
            r#"{"selected_node_ids":["garbage","d9:n0.0","d0:nope","d0:n0.0"]}"#,
            vec![t],
            chunks,
        );
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert_eq!(out.len(), 1);
    }
}
