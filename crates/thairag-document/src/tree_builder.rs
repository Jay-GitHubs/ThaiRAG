//! Reasoning-based ("PageIndex") tree builder.
//!
//! Given a document's converted markdown, an LLM emits a flat list of sections
//! (title + one-line summary + page range, each with a model-chosen id and a
//! parent id). We reconstruct the hierarchy in Rust and re-number the nodes with
//! stable, path-like ids (`n0`, `n0.0`, `n0.1.2`, …) — never trusting the model's
//! ids for anything but linking — so navigation can reference nodes
//! unambiguously and so a malformed/cyclic parent graph degrades to a flat tree
//! instead of failing.
//!
//! The build is schema-enforced (Ollama `format`) so the model can only emit
//! conforming JSON; providers without schema support fall back to plain text and
//! we parse defensively. Pure-LLM trees are non-deterministic across rebuilds —
//! the caller is expected to drive the provider at temperature 0 and to record
//! `model_name` for provenance.

use serde::Deserialize;
use tracing::warn;

use thairag_core::error::{Result, ThaiRagError};
use thairag_core::models::{DocTree, DocTreeNode};
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, DocId};

use crate::ai::analyzer::strip_json_fences;

/// Hard cap on converted text sent to the model. The page/heading skeleton is
/// what matters for the tree, so we keep the head of the document (title +
/// early pages) rather than risk overrunning a small model's context. Truncation
/// is logged.
const MAX_INPUT_CHARS: usize = 32_000;

/// Backstops against a pathological model response (e.g. one node per line):
/// total nodes kept, and maximum nesting depth.
const MAX_NODES: usize = 400;
const MAX_DEPTH: usize = 6;

/// One section as emitted by the model (flat; linked via `id`/`parent_id`).
#[derive(Debug, Deserialize)]
struct FlatNode {
    /// Model-chosen id used only to resolve `parent_id`. May be missing/blank
    /// (then the node is treated as top-level) or duplicated (first wins).
    #[serde(default)]
    id: Option<String>,
    /// Parent section's `id`. Absent/blank/unknown ⇒ a top-level section.
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    page_start: Option<usize>,
    #[serde(default)]
    page_end: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FlatTree {
    /// Document-level summary, used as the root node's summary for coarse,
    /// cross-document selection.
    #[serde(default)]
    doc_summary: String,
    #[serde(default)]
    nodes: Vec<FlatNode>,
}

/// JSON schema for the flat tree. Deliberately NON-recursive (a flat array with a
/// `parent_id` link) — Ollama's `format` is unreliable on recursive `$ref`/`$defs`,
/// and the reliability of schema-enforced decoding is the whole point of using it.
fn tree_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "doc_summary": { "type": "string", "maxLength": 400 },
            "nodes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "maxLength": 32 },
                        "parent_id": { "type": "string", "maxLength": 32 },
                        "title": { "type": "string", "maxLength": 200 },
                        "summary": { "type": "string", "maxLength": 400 },
                        "page_start": { "type": "integer" },
                        "page_end": { "type": "integer" }
                    },
                    "required": ["id", "title", "summary"]
                }
            }
        },
        "required": ["nodes"]
    })
}

fn build_prompt(title: &str, text: &str) -> String {
    format!(
        "You are building a navigable table of contents (a \"PageIndex\" tree) for a \
document so an assistant can later reason about which sections answer a question.\n\n\
Read the document below and output its hierarchical structure as a FLAT list of \
sections. For each section give:\n\
- \"id\": a short unique id you assign (e.g. \"s1\", \"s1a\").\n\
- \"parent_id\": the id of the enclosing section, or omit it for a top-level section.\n\
- \"title\": the section heading (use the document's own headings where present).\n\
- \"summary\": ONE sentence on what the section covers — enough to decide relevance \
without reading it. Write the summary in the document's primary language.\n\
- \"page_start\"/\"page_end\": the 1-indexed page range the section spans, taken from \
the \"## Page N\" markers, when determinable.\n\n\
Also give a \"doc_summary\": one sentence describing the whole document.\n\n\
Cover the document end to end; prefer 5–40 meaningful sections over hundreds of \
tiny ones. Do not invent content.\n\n\
Document title: {title}\n\n\
=== DOCUMENT START ===\n{text}\n=== DOCUMENT END ==="
    )
}

/// Build a [`DocTree`] for a document from its converted markdown.
///
/// `model_name` on the returned tree is taken from the provider. Returns `Err`
/// when the model yields no usable structure, so the caller can simply skip
/// persisting a tree for that document (and fall back to lexical retrieval).
pub async fn build_tree(
    llm: &dyn LlmProvider,
    doc_id: DocId,
    title: &str,
    converted_text: &str,
    max_tokens: u32,
) -> Result<DocTree> {
    let trimmed = converted_text.trim();
    if trimmed.is_empty() {
        return Err(ThaiRagError::Internal(
            "tree build: empty converted text".into(),
        ));
    }

    let text = if trimmed.len() > MAX_INPUT_CHARS {
        let cut = thairag_core::floor_char_boundary(trimmed, MAX_INPUT_CHARS);
        warn!(
            doc_id = %doc_id,
            kept = cut,
            total = trimmed.len(),
            "tree build: document truncated to fit context"
        );
        &trimmed[..cut]
    } else {
        trimmed
    };

    let messages = vec![ChatMessage {
        role: "user".into(),
        content: build_prompt(title, text),
        images: vec![],
    }];

    let response = llm
        .generate_structured(&messages, Some(max_tokens), &tree_schema())
        .await?;
    let json_str = strip_json_fences(response.content.trim());

    let flat: FlatTree = serde_json::from_str(json_str).map_err(|e| {
        warn!(doc_id = %doc_id, error = %e, "tree build: response was not valid JSON");
        ThaiRagError::Internal(format!("tree build: parse failed: {e}"))
    })?;

    reconstruct(doc_id, title, flat)
        .ok_or_else(|| ThaiRagError::Internal("tree build: model returned no sections".into()))
        .map(|mut tree| {
            tree.model_name = Some(llm.model_name().to_string());
            tree
        })
}

/// Reconstruct a nested [`DocTree`] from the model's flat node list.
///
/// Robust to a malformed parent graph: unknown/self/cyclic `parent_id`s drop the
/// edge and re-root the node, so every (kept) node appears exactly once and the
/// walk always terminates.
fn reconstruct(doc_id: DocId, title: &str, flat: FlatTree) -> Option<DocTree> {
    let nodes: Vec<FlatNode> = flat.nodes.into_iter().take(MAX_NODES).collect();
    if nodes.is_empty() {
        return None;
    }

    // First occurrence of each non-empty id wins.
    let mut id_to_idx: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if let Some(id) = n.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            id_to_idx.entry(id).or_insert(i);
        }
    }

    // Resolve each node's parent index (None ⇒ top-level). A parent that is
    // missing, blank, unknown, or self drops to top-level.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        let parent = n
            .parent_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|p| id_to_idx.get(p).copied())
            .filter(|&p| p != i);
        match parent {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }

    // DFS from the roots, assigning stable path ids and breaking cycles via the
    // visited set. The synthetic root holds the document-level summary.
    let mut visited = vec![false; nodes.len()];
    let mut top: Vec<DocTreeNode> = Vec::new();
    for (k, &r) in roots.iter().enumerate() {
        if let Some(node) = build_node(r, &nodes, &children, &mut visited, &format!("n0.{k}"), 1) {
            top.push(node);
        }
    }
    // Any node not reached (orphaned by a bad parent, or trapped in a cycle)
    // becomes an extra top-level section so nothing is silently dropped.
    for i in 0..nodes.len() {
        if !visited[i] {
            let k = top.len();
            if let Some(node) =
                build_node(i, &nodes, &children, &mut visited, &format!("n0.{k}"), 1)
            {
                top.push(node);
            }
        }
    }

    if top.is_empty() {
        return None;
    }

    let page_start = top.iter().filter_map(|n| n.page_start).min();
    let page_end = top.iter().filter_map(|n| n.page_end).max();
    let doc_summary = {
        let s = flat.doc_summary.trim();
        if s.is_empty() {
            format!("Document: {title}")
        } else {
            s.to_string()
        }
    };

    Some(DocTree {
        doc_id,
        title: title.to_string(),
        root: DocTreeNode {
            node_id: "n0".to_string(),
            title: title.to_string(),
            summary: doc_summary,
            page_start,
            page_end,
            children: top,
        },
        model_name: None,
    })
}

fn build_node(
    idx: usize,
    nodes: &[FlatNode],
    children: &[Vec<usize>],
    visited: &mut [bool],
    node_id: &str,
    depth: usize,
) -> Option<DocTreeNode> {
    if visited[idx] {
        return None;
    }
    visited[idx] = true;
    let n = &nodes[idx];

    let mut kids = Vec::new();
    if depth < MAX_DEPTH {
        for &c in &children[idx] {
            if let Some(child) = build_node(
                c,
                nodes,
                children,
                visited,
                &format!("{node_id}.{}", kids.len()),
                depth + 1,
            ) {
                kids.push(child);
            }
        }
    }

    let title = {
        let t = n.title.trim();
        if t.is_empty() {
            "(untitled section)".to_string()
        } else {
            t.to_string()
        }
    };
    Some(DocTreeNode {
        node_id: node_id.to_string(),
        title,
        summary: n.summary.trim().to_string(),
        page_start: n.page_start,
        page_end: n.page_end,
        children: kids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;
    use thairag_core::types::{LlmResponse, LlmUsage};

    /// A stub LLM that returns canned content from `generate_structured`,
    /// capturing the schema it was handed.
    struct StubLlm {
        reply: String,
        saw_schema: Arc<Mutex<Option<serde_json::Value>>>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for StubLlm {
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: self.reply.clone(),
                usage: LlmUsage::default(),
            })
        }
        async fn generate_structured(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
            json_schema: &serde_json::Value,
        ) -> Result<LlmResponse> {
            *self.saw_schema.lock().unwrap() = Some(json_schema.clone());
            Ok(LlmResponse {
                content: self.reply.clone(),
                usage: LlmUsage::default(),
            })
        }
        fn model_name(&self) -> &str {
            "stub-model"
        }
    }

    fn stub(reply: &str) -> StubLlm {
        StubLlm {
            reply: reply.to_string(),
            saw_schema: Arc::new(Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn builds_nested_tree_and_renumbers_ids() {
        let reply = r#"{
            "doc_summary": "An SME loan product.",
            "nodes": [
                {"id": "a", "title": "Overview", "summary": "intro", "page_start": 1, "page_end": 1},
                {"id": "b", "parent_id": "a", "title": "Eligibility", "summary": "who qualifies", "page_start": 1, "page_end": 2},
                {"id": "c", "title": "Terms", "summary": "rates and limits", "page_start": 3, "page_end": 4}
            ]
        }"#;
        let llm = stub(reply);
        let tree = build_tree(&llm, DocId::new(), "SME Loan", "## Page 1\nhi", 1024)
            .await
            .unwrap();

        assert_eq!(tree.root.node_id, "n0");
        assert_eq!(tree.root.summary, "An SME loan product.");
        assert_eq!(tree.model_name.as_deref(), Some("stub-model"));
        // Two top-level sections; ids are builder-assigned, not the model's.
        assert_eq!(tree.root.children.len(), 2);
        assert_eq!(tree.root.children[0].node_id, "n0.0");
        assert_eq!(tree.root.children[0].title, "Overview");
        // "Eligibility" nested under "Overview".
        assert_eq!(tree.root.children[0].children.len(), 1);
        assert_eq!(tree.root.children[0].children[0].node_id, "n0.0.0");
        assert_eq!(tree.root.children[0].children[0].title, "Eligibility");
        assert_eq!(tree.root.children[1].node_id, "n0.1");
        // Root page span derives from children.
        assert_eq!(tree.root.page_start, Some(1));
        assert_eq!(tree.root.page_end, Some(4));
    }

    #[tokio::test]
    async fn passes_a_non_recursive_schema() {
        let llm = stub(r#"{"nodes":[{"id":"a","title":"t","summary":"s"}]}"#);
        let _ = build_tree(&llm, DocId::new(), "T", "body", 256)
            .await
            .unwrap();
        let schema = llm.saw_schema.lock().unwrap().clone().unwrap();
        let s = serde_json::to_string(&schema).unwrap();
        assert!(!s.contains("$ref"), "schema must not be recursive");
        assert!(s.contains("parent_id"));
    }

    #[tokio::test]
    async fn cyclic_parents_do_not_loop_and_keep_all_nodes() {
        // a→b→a is a cycle; both must still surface exactly once.
        let reply = r#"{
            "nodes": [
                {"id": "a", "parent_id": "b", "title": "A", "summary": ""},
                {"id": "b", "parent_id": "a", "title": "B", "summary": ""}
            ]
        }"#;
        let llm = stub(reply);
        let tree = build_tree(&llm, DocId::new(), "Doc", "body", 256)
            .await
            .unwrap();
        // Collect every node id in the tree.
        let mut titles = Vec::new();
        let mut stack = vec![&tree.root];
        while let Some(n) = stack.pop() {
            titles.push(n.title.clone());
            for c in &n.children {
                stack.push(c);
            }
        }
        assert!(titles.contains(&"A".to_string()));
        assert!(titles.contains(&"B".to_string()));
    }

    #[tokio::test]
    async fn unknown_parent_falls_back_to_top_level() {
        let reply = r#"{"nodes":[{"id":"x","parent_id":"nope","title":"Solo","summary":"s"}]}"#;
        let llm = stub(reply);
        let tree = build_tree(&llm, DocId::new(), "Doc", "body", 256)
            .await
            .unwrap();
        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].title, "Solo");
    }

    #[tokio::test]
    async fn empty_text_errors() {
        let llm = stub("{}");
        assert!(
            build_tree(&llm, DocId::new(), "Doc", "   ", 256)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn no_sections_errors() {
        let llm = stub(r#"{"doc_summary":"x","nodes":[]}"#);
        assert!(
            build_tree(&llm, DocId::new(), "Doc", "body", 256)
                .await
                .is_err()
        );
    }
}
