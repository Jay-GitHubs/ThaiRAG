//! Integration hooks that apply registered plugins to document processing,
//! search, and chunking pipelines.

use thairag_core::types::{DocumentChunk, SearchResult};
use tracing::{debug, warn};

use crate::plugin_registry::PluginRegistry;

/// Apply document plugins to raw content before it enters the document pipeline.
///
/// If an enabled document plugin matches the MIME type, its `process()` is called
/// on the text content. The modified content is returned as bytes.
pub fn apply_document_plugins(registry: &PluginRegistry, raw: &[u8], mime_type: &str) -> Vec<u8> {
    let plugin = match registry.get_document_plugin(mime_type) {
        Some(p) => p,
        None => return raw.to_vec(),
    };

    // Convert bytes to text for plugin processing
    let text = match std::str::from_utf8(raw) {
        Ok(t) => t,
        Err(_) => {
            debug!(
                plugin = plugin.name(),
                "Skipping document plugin: content is not valid UTF-8"
            );
            return raw.to_vec();
        }
    };

    match plugin.process(text, mime_type) {
        Ok(processed) => {
            debug!(plugin = plugin.name(), mime_type, "Applied document plugin");
            processed.into_bytes()
        }
        Err(e) => {
            warn!(
                plugin = plugin.name(),
                error = %e,
                "Document plugin failed, using original content"
            );
            raw.to_vec()
        }
    }
}

/// Apply chunk plugins to chunks after splitting.
///
/// Each enabled chunk plugin's `transform_chunk()` is called on every chunk's
/// content. Plugins are applied in registration order.
pub fn apply_chunk_plugins(registry: &PluginRegistry, chunks: &mut [DocumentChunk]) {
    let plugins = registry.get_chunk_plugins();
    if plugins.is_empty() {
        return;
    }

    for chunk in chunks.iter_mut() {
        for plugin in &plugins {
            chunk.content = plugin.transform_chunk(&chunk.content);
        }
    }

    debug!(
        plugin_count = plugins.len(),
        chunk_count = chunks.len(),
        "Applied chunk plugins"
    );
}

/// Apply search plugin pre-search hooks to transform the query.
///
/// Each enabled search plugin's `pre_search()` is called in sequence,
/// each receiving the output of the previous plugin.
pub fn apply_pre_search(registry: &PluginRegistry, query: &str) -> String {
    let plugins = registry.get_search_plugins();
    if plugins.is_empty() {
        return query.to_string();
    }

    let mut q = query.to_string();
    for plugin in &plugins {
        q = plugin.pre_search(&q);
    }

    debug!(
        original = query,
        transformed = %q,
        "Applied search pre-processing plugins"
    );
    q
}

/// Apply search plugin post-search hooks to filter/re-rank results.
///
/// Each enabled search plugin's `post_search()` is called in sequence.
pub fn apply_post_search(
    registry: &PluginRegistry,
    results: Vec<SearchResult>,
) -> Vec<SearchResult> {
    let plugins = registry.get_search_plugins();
    if plugins.is_empty() {
        return results;
    }

    let mut r = results;
    for plugin in &plugins {
        r = plugin.post_search(r);
    }

    debug!(
        result_count = r.len(),
        "Applied search post-processing plugins"
    );
    r
}
