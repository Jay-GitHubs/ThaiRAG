//! Built-in plugins that ship with ThaiRAG.
//!
//! These serve as both useful defaults and examples for writing custom plugins.

use thairag_core::error::Result;
use thairag_core::traits::{ChunkPlugin, DocumentPlugin, SearchPlugin};
use thairag_core::types::SearchResult;

// ── MetadataStripPlugin (DocumentPlugin) ─────────────────────────────

/// Strips HTML/XML metadata tags (`<meta ...>`, `<link ...>`, `<style>...</style>`,
/// `<script>...</script>`) from document content before further processing.
pub struct MetadataStripPlugin;

impl DocumentPlugin for MetadataStripPlugin {
    fn name(&self) -> &str {
        "metadata-strip"
    }

    fn description(&self) -> &str {
        "Strips HTML/XML metadata, style, and script tags from document content"
    }

    fn supported_mime_types(&self) -> Vec<String> {
        vec![
            "text/html".to_string(),
            "application/xhtml+xml".to_string(),
            "application/xml".to_string(),
            "text/xml".to_string(),
        ]
    }

    fn process(&self, content: &str, _mime_type: &str) -> Result<String> {
        let mut result = content.to_string();

        // Remove <script>...</script> blocks (case-insensitive, non-greedy)
        result = regex_replace_all(&result, r"(?is)<script[^>]*>.*?</script>");
        // Remove <style>...</style> blocks
        result = regex_replace_all(&result, r"(?is)<style[^>]*>.*?</style>");
        // Remove <meta .../> and <meta ...> tags
        result = regex_replace_all(&result, r"(?i)<meta\s[^>]*/?>");
        // Remove <link .../> tags (stylesheets, icons, etc.)
        result = regex_replace_all(&result, r"(?i)<link\s[^>]*/?>");

        Ok(result)
    }
}

fn regex_replace_all(input: &str, pattern: &str) -> String {
    match regex::Regex::new(pattern) {
        Ok(re) => re.replace_all(input, "").to_string(),
        Err(_) => input.to_string(),
    }
}

// ── QueryExpansionPlugin (SearchPlugin) ──────────────────────────────

/// Expands search queries with common synonyms and related terms.
/// This is a simple rule-based expansion; production systems would use
/// an LLM or thesaurus API.
pub struct QueryExpansionPlugin;

impl SearchPlugin for QueryExpansionPlugin {
    fn name(&self) -> &str {
        "query-expansion"
    }

    fn description(&self) -> &str {
        "Expands search queries with common synonyms and related terms"
    }

    fn pre_search(&self, query: &str) -> String {
        let lower = query.to_lowercase();
        let mut expanded = query.to_string();

        // Simple synonym map for demonstration
        let synonyms: &[(&str, &[&str])] = &[
            ("error", &["issue", "problem", "bug"]),
            ("fix", &["resolve", "repair", "patch"]),
            ("create", &["make", "build", "generate"]),
            ("delete", &["remove", "drop", "clear"]),
            ("update", &["modify", "change", "edit"]),
            ("search", &["find", "lookup", "query"]),
            ("config", &["configuration", "settings", "setup"]),
            ("auth", &["authentication", "login", "authorization"]),
            ("doc", &["document", "documentation"]),
            ("api", &["endpoint", "interface"]),
        ];

        for (term, syns) in synonyms {
            if lower.contains(term) {
                // Append first two synonyms
                let additions: Vec<&str> = syns.iter().take(2).copied().collect();
                expanded = format!("{expanded} {}", additions.join(" "));
                break; // Only expand one term to avoid query explosion
            }
        }

        expanded
    }

    fn post_search(&self, results: Vec<SearchResult>) -> Vec<SearchResult> {
        // Pass through — this plugin only transforms the query
        results
    }
}

// ── SummaryChunkPlugin (ChunkPlugin) ─────────────────────────────────

/// Prepends a one-line summary header to each chunk, derived from the first
/// sentence of the chunk content. This helps embedding models better understand
/// each chunk's topic at a glance.
pub struct SummaryChunkPlugin;

impl ChunkPlugin for SummaryChunkPlugin {
    fn name(&self) -> &str {
        "summary-chunk"
    }

    fn description(&self) -> &str {
        "Prepends a one-line summary header extracted from each chunk's first sentence"
    }

    fn transform_chunk(&self, chunk: &str) -> String {
        let trimmed = chunk.trim();
        if trimmed.is_empty() {
            return chunk.to_string();
        }

        // Extract first sentence (up to first period, question mark, or newline)
        let first_sentence = trimmed
            .find(['.', '?', '!', '\n'])
            .map(|pos| &trimmed[..=pos])
            .unwrap_or_else(|| {
                // No sentence-ending punctuation — take first 80 chars
                if trimmed.len() > 80 {
                    &trimmed[..80]
                } else {
                    trimmed
                }
            })
            .trim();

        // Don't add a header if the chunk IS just one sentence
        if first_sentence.len() >= trimmed.len() - 1 {
            return chunk.to_string();
        }

        format!("[Summary: {first_sentence}]\n{chunk}")
    }
}

/// Register all built-in plugins into the given registry.
pub fn register_builtin_plugins(registry: &crate::plugin_registry::PluginRegistry) {
    use std::sync::Arc;

    registry.register_document_plugin(Arc::new(MetadataStripPlugin));
    registry.register_search_plugin(Arc::new(QueryExpansionPlugin));
    registry.register_chunk_plugin(Arc::new(SummaryChunkPlugin));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_strip_removes_script_and_style() {
        let plugin = MetadataStripPlugin;
        let html = r#"<html><head><meta charset="utf-8"><style>body{}</style><script>alert(1)</script></head><body>Hello</body></html>"#;
        let result = plugin.process(html, "text/html").unwrap();
        assert!(!result.contains("<script"));
        assert!(!result.contains("<style"));
        assert!(!result.contains("<meta"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn query_expansion_adds_synonyms() {
        let plugin = QueryExpansionPlugin;
        let expanded = plugin.pre_search("how to fix this error");
        assert!(expanded.contains("fix"));
        // Should have added synonyms ("error" matches first in the synonym table)
        assert!(expanded.contains("issue") || expanded.contains("resolve"));
    }

    #[test]
    fn query_expansion_no_match_passthrough() {
        let plugin = QueryExpansionPlugin;
        let query = "hello world";
        let expanded = plugin.pre_search(query);
        assert_eq!(expanded, "hello world");
    }

    #[test]
    fn summary_chunk_prepends_header() {
        let plugin = SummaryChunkPlugin;
        let chunk = "This is about Rust programming. It covers ownership, borrowing, and lifetimes in great detail.";
        let result = plugin.transform_chunk(chunk);
        assert!(result.starts_with("[Summary:"));
        assert!(result.contains("This is about Rust programming."));
    }

    #[test]
    fn summary_chunk_skips_single_sentence() {
        let plugin = SummaryChunkPlugin;
        let chunk = "Just one sentence.";
        let result = plugin.transform_chunk(chunk);
        assert_eq!(result, chunk);
    }

    #[test]
    fn summary_chunk_handles_empty() {
        let plugin = SummaryChunkPlugin;
        assert_eq!(plugin.transform_chunk(""), "");
    }
}
