//! Reasoning-based ("PageIndex") retrieval.
//!
//! Instead of embedding the query and ranking by vector/lexical similarity, this
//! retriever hands an LLM the *table-of-contents trees* of the in-scope documents
//! (section titles + one-line summaries, never the full text) and asks it to
//! **navigate** to the sections that answer the question. The selected nodes'
//! **full section text** — sliced intact from the document's converted text by
//! page range, never re-chunked — is returned as ordinary [`SearchResult`]s, so
//! the rest of the pipeline (answer generation, citations) is unchanged.
//!
//! Feeding the *whole selected section* (not chunks) is the core of the
//! PageIndex approach: a table or cross-referenced passage reaches the answer
//! model intact, instead of being shredded by fixed-size chunking — the very
//! thing vectorless RAG exists to avoid. Re-chunked content is only a fallback
//! for documents whose converted text has no sliceable page structure.
//!
//! It is store-agnostic: trees, full text, and chunks arrive via injected
//! closures ([`TreeResolver`], [`ContentResolver`], [`ChunkResolver`]). When it
//! can't produce anything (no trees in scope, navigation yields nothing, or no
//! node maps to content) it returns an empty vec and the caller falls back to
//! lexical retrieval.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use regex::Regex;
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
/// Resolves a document's converted full text (with `## Page N` markers).
pub type ContentResolver = Arc<dyn Fn(DocId) -> Option<String> + Send + Sync>;
/// Resolves a document's chunks (in `chunk_index` order). Fallback for docs
/// whose converted text can't be sliced into the selected section.
pub type ChunkResolver = Arc<dyn Fn(DocId) -> Vec<DocumentChunk> + Send + Sync>;

/// Hard cap on sections/chunks returned, so a broad selection can't dump an
/// entire corpus into context (the curator trims further by token budget).
const MAX_RESULT_CHUNKS: usize = 60;

/// LLM-navigated retriever over per-document PageIndex trees.
pub struct ReasoningRetriever {
    nav_llm: Arc<dyn LlmProvider>,
    tree_resolver: TreeResolver,
    content_resolver: ContentResolver,
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
        content_resolver: ContentResolver,
        chunk_resolver: ChunkResolver,
        max_docs: usize,
        max_nodes: usize,
        max_tokens: u32,
    ) -> Self {
        Self {
            nav_llm,
            tree_resolver,
            content_resolver,
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
        // deterministic doc prefilter (`query.doc_ids`) when present.
        let mut trees = (self.tree_resolver)(&query.workspace_ids);
        if !query.doc_ids.is_empty() {
            let keep: HashSet<DocId> = query.doc_ids.iter().copied().collect();
            trees.retain(|t| keep.contains(&t.doc_id));
        }
        // Cap how many doc outlines the navigator sees. When more docs are in
        // scope than the cap (and no prefilter already narrowed them), pick the
        // most query-relevant trees rather than an arbitrary first-N — otherwise
        // the answer's document might never be offered to the navigator at all.
        if trees.len() > self.max_docs {
            let before = trees.len();
            trees = select_relevant_trees(trees, &query.text, self.max_docs);
            debug!(
                kept = trees.len(),
                dropped = before - trees.len(),
                "reasoning retrieval: ranked in-scope trees by query relevance and capped"
            );
        }
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

        // Map each selected "d{i}:{node_id}" key back to its node, then to the
        // node's FULL section text (sliced intact from the document's converted
        // text by page range). Re-chunked content is only a fallback for docs
        // whose converted text has no `## Page N` structure to slice.
        let mut results: Vec<SearchResult> = Vec::new();
        let mut seen_sections: HashSet<(DocId, usize, usize)> = HashSet::new();
        let mut seen_chunks: HashSet<String> = HashSet::new();
        let mut content_cache: std::collections::HashMap<DocId, Option<String>> =
            std::collections::HashMap::new();
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
            let doc_id = tree.doc_id;

            // Primary path: the node's full section text, intact.
            let section = node_page_span(node).and_then(|(s, e)| {
                if !seen_sections.insert((doc_id, s, e)) {
                    return None; // this section already emitted (e.g. parent+child)
                }
                content_cache
                    .entry(doc_id)
                    .or_insert_with(|| (self.content_resolver)(doc_id))
                    .as_deref()
                    .and_then(|text| slice_section(text, s, e))
                    // Linearize HTML tables into a clean rectangular grid so the
                    // answer model can read cell↔header alignment (the dominant
                    // cause of table-QA failure — see `table_linearize`).
                    .map(|content| (crate::table_linearize::linearize_tables(&content), s, e))
            });
            if let Some((content, s, e)) = section {
                let score = 1.0 / (1.0 + results.len() as f32);
                results.push(section_result(doc_id, node, content, s, e, score));
                if results.len() >= MAX_RESULT_CHUNKS {
                    return Ok(results);
                }
                continue;
            }

            // Fallback: re-chunked content for docs without sliceable pages.
            let chunks = chunk_cache
                .entry(doc_id)
                .or_insert_with(|| (self.chunk_resolver)(doc_id));
            for chunk in chunks_for_node(node, chunks) {
                if !seen_chunks.insert(chunk.chunk_id.to_string()) {
                    continue;
                }
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

/// Keep the `max_docs` trees most relevant to the query, ranked by how many of
/// the query's key terms appear in the tree's outline text (title + every node
/// title + summary). Deterministic, LLM-free, and stable on ties — so the cap
/// drops the *least* relevant docs instead of an arbitrary suffix. If the query
/// has no usable terms (or nothing overlaps), falls back to the first `max_docs`.
fn select_relevant_trees(trees: Vec<DocTree>, query: &str, max_docs: usize) -> Vec<DocTree> {
    let terms = key_terms(query);
    if terms.is_empty() {
        let mut t = trees;
        t.truncate(max_docs);
        return t;
    }
    let mut scored: Vec<(usize, DocTree)> = trees
        .into_iter()
        .map(|t| {
            let bag = tree_text(&t).to_lowercase();
            let score = terms.iter().filter(|term| bag.contains(*term)).count();
            (score, t)
        })
        .collect();
    // Stable sort by score desc keeps the resolver's original order on ties.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().take(max_docs).map(|(_, t)| t).collect()
}

/// Concatenate a tree's searchable text: title + every node's title + summary.
fn tree_text(tree: &DocTree) -> String {
    fn walk(n: &DocTreeNode, out: &mut String) {
        out.push(' ');
        out.push_str(&n.title);
        out.push(' ');
        out.push_str(&n.summary);
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut s = tree.title.clone();
    walk(&tree.root, &mut s);
    s
}

/// Extract distinct query key terms: numbers, Latin words (2+ chars), and Thai
/// runs (3+ chars — Thai has no word spaces). Mirrors the bench scorer so the
/// relevance signal is consistent with how accuracy is measured.
fn key_terms(query: &str) -> Vec<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\d+|[a-z]{2,}|[\u{0e01}-\u{0e5b}]{3,}").unwrap());
    let lower = query.to_lowercase();
    let mut terms: Vec<String> = RE
        .find_iter(&lower)
        .map(|m| m.as_str().to_string())
        .collect();
    terms.sort();
    terms.dedup();
    terms
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
/// Slice the converted text for the (inclusive) page range `[start, end]`,
/// intact, using the `## Page N` markers `assemble_document_markdown` emits.
/// Returns `None` when the document has no page markers (caller falls back to
/// chunk mapping) or the range is empty.
fn slice_section(converted_text: &str, start: usize, end: usize) -> Option<String> {
    let mut out = String::new();
    let mut cur: Option<usize> = None;
    let mut any = false;
    for line in converted_text.lines() {
        if let Some(rest) = line.strip_prefix("## Page ")
            && let Ok(p) = rest.trim().parse::<usize>()
        {
            cur = Some(p);
            if p >= start && p <= end {
                out.push_str(line);
                out.push('\n');
                any = true;
            }
            continue;
        }
        if cur.is_some_and(|p| p >= start && p <= end) {
            out.push_str(line);
            out.push('\n');
            any = true;
        }
    }
    if any {
        Some(out.trim().to_string())
    } else {
        None
    }
}

/// Wrap a node's full section text as a `SearchResult`, carrying the page range
/// and section title as metadata so downstream citations stay accurate.
fn section_result(
    doc_id: DocId,
    node: &DocTreeNode,
    content: String,
    page_start: usize,
    page_end: usize,
    score: f32,
) -> SearchResult {
    use thairag_core::types::{ChunkId, ChunkMetadata, WorkspaceId};
    SearchResult {
        chunk: DocumentChunk {
            chunk_id: ChunkId::new(),
            doc_id,
            workspace_id: WorkspaceId::default(),
            content,
            chunk_index: 0,
            embedding: None,
            metadata: Some(ChunkMetadata {
                section_title: Some(node.title.clone()),
                page_numbers: Some((page_start..=page_end).collect()),
                ..Default::default()
            }),
        },
        score,
    }
}

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

    /// Build a retriever whose content resolver always returns `None`, so the
    /// chunk-fallback path is exercised (these tests predate full-section text).
    fn retriever(
        reply: &str,
        trees: Vec<DocTree>,
        chunks: Vec<DocumentChunk>,
    ) -> (ReasoningRetriever, Arc<Mutex<Option<String>>>) {
        retriever_with_content(reply, trees, chunks, |_| None)
    }

    fn retriever_with_content(
        reply: &str,
        trees: Vec<DocTree>,
        chunks: Vec<DocumentChunk>,
        content: fn(DocId) -> Option<String>,
    ) -> (ReasoningRetriever, Arc<Mutex<Option<String>>>) {
        let prompt = Arc::new(Mutex::new(None));
        let llm = Arc::new(StubLlm {
            reply: reply.into(),
            prompt: Arc::clone(&prompt),
        });
        let tree_resolver: TreeResolver = Arc::new(move |_ws: &[WorkspaceId]| trees.clone());
        let content_resolver: ContentResolver = Arc::new(move |doc: DocId| content(doc));
        let chunk_resolver: ChunkResolver = Arc::new(move |doc: DocId| {
            chunks.iter().filter(|c| c.doc_id == doc).cloned().collect()
        });
        (
            ReasoningRetriever::new(
                llm,
                tree_resolver,
                content_resolver,
                chunk_resolver,
                5,
                12,
                1024,
            ),
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

    #[test]
    fn select_relevant_trees_surfaces_match_outside_first_n() {
        // The doc whose tree mentions the query terms is 3rd of 4; with a cap of
        // 2 it must still be offered (and rank first) — not arbitrarily dropped.
        let mk = |title: &str, kid_title: &str, kid_summary: &str| -> DocTree {
            tree(
                DocId::new(),
                title,
                vec![DocTreeNode {
                    node_id: "n0.0".into(),
                    title: kid_title.into(),
                    summary: kid_summary.into(),
                    page_start: Some(1),
                    page_end: Some(1),
                    children: vec![],
                }],
            )
        };
        let t0 = mk("Loan A", "Terms", "general terms");
        let t1 = mk("Loan B", "Terms", "general terms");
        let t2 = mk(
            "Loan C",
            "Collateral",
            "requires fixed deposit as collateral",
        );
        let t3 = mk("Loan D", "Terms", "general terms");
        let want = t2.doc_id;

        let picked = select_relevant_trees(vec![t0, t1, t2, t3], "what collateral is required?", 2);
        assert_eq!(picked.len(), 2);
        assert_eq!(
            picked[0].doc_id, want,
            "most query-relevant tree ranks first"
        );
        assert!(
            picked.iter().any(|t| t.doc_id == want),
            "the matching doc must be offered despite the cap"
        );
    }

    #[test]
    fn select_relevant_trees_falls_back_to_first_n_without_overlap() {
        let t0 = tree(
            DocId::new(),
            "A",
            vec![node("n0.0", "x", Some(1), Some(1), vec![])],
        );
        let first = t0.doc_id;
        let t1 = tree(
            DocId::new(),
            "B",
            vec![node("n0.0", "y", Some(1), Some(1), vec![])],
        );
        // Query terms ("zzzz") appear in neither tree → score 0 for all → keep
        // the first N in original order.
        let picked = select_relevant_trees(vec![t0, t1], "zzzz", 1);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].doc_id, first);
    }

    #[test]
    fn slice_section_returns_full_intact_pages() {
        let doc = "# Title\n\n---\n## Page 1\n<table><tr><td>rate</td><td>3.0</td></tr></table>\n\n---\n## Page 2\nother content\n";
        // Page 1 returns the whole intact table, not a fragment; page 2 excluded.
        let p1 = slice_section(doc, 1, 1).unwrap();
        assert!(p1.contains("<table>") && p1.contains("3.0") && p1.contains("</table>"));
        assert!(!p1.contains("other content"));
        // No page markers → None (caller falls back to chunks).
        assert!(slice_section("plain text no markers", 1, 1).is_none());
    }

    #[tokio::test]
    async fn feeds_full_section_text_not_chunks() {
        // The doc's converted text holds the WHOLE table intact; a chunk would
        // only carry a fragment. Reasoning must feed the full section.
        let ws = WorkspaceId::new();
        let doc = DocId::new();
        let t = tree(
            doc,
            "Rates",
            vec![node("n0.0", "Rate table", Some(1), Some(1), vec![])],
        );
        // A chunk exists too, but with only a fragment — the section path must win.
        let frag = chunk(
            doc,
            0,
            "อัตราภาษี 3.0 (fragment)",
            Some(vec![1]),
            Some("Rate table"),
        );
        let (rr, _) = retriever_with_content(
            r#"{"selected_node_ids":["d0:n0.0"]}"#,
            vec![t],
            vec![frag],
            |_| {
                Some(
                    "# Rates\n\n---\n## Page 1\n<table><tr><td>อัตราภาษี</td><td>3.0</td><td>10.0</td></tr></table>\n\n---\n## Page 2\nfootnotes\n"
                        .to_string(),
                )
            },
        );
        let out = rr.retrieve(&query(ws, vec![])).await.unwrap();
        assert_eq!(out.len(), 1);
        // Full table, linearized (no raw <table>), incl. the cell the chunk
        // fragment dropped (10.0).
        assert!(out[0].chunk.content.contains("10.0"));
        assert!(out[0].chunk.content.contains("3.0"));
        assert!(!out[0].chunk.content.contains("<table>")); // linearized
        assert!(!out[0].chunk.content.contains("fragment"));
        assert!(!out[0].chunk.content.contains("footnotes")); // page 2 excluded
        // Citation metadata preserved.
        let md = out[0].chunk.metadata.as_ref().unwrap();
        assert_eq!(md.section_title.as_deref(), Some("Rate table"));
        assert_eq!(md.page_numbers.as_deref(), Some(&[1usize][..]));
    }
}
