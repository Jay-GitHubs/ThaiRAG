use std::sync::{Arc, RwLock};

use serde::Serialize;
use thairag_core::traits::{ChunkPlugin, DocumentPlugin, SearchPlugin};

/// Metadata about a registered plugin, returned by the list API.
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    pub plugin_type: PluginType,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    Document,
    Search,
    Chunk,
}

/// Thread-safe registry for document, search, and chunk plugins.
///
/// Plugins are registered at startup. Enable/disable state is tracked
/// in the KV settings store via the `plugin.enabled.<name>` key pattern,
/// and cached in a local set for fast lookups.
pub struct PluginRegistry {
    document_plugins: RwLock<Vec<Arc<dyn DocumentPlugin>>>,
    search_plugins: RwLock<Vec<Arc<dyn SearchPlugin>>>,
    chunk_plugins: RwLock<Vec<Arc<dyn ChunkPlugin>>>,
    /// Set of enabled plugin names.
    enabled: RwLock<std::collections::HashSet<String>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            document_plugins: RwLock::new(Vec::new()),
            search_plugins: RwLock::new(Vec::new()),
            chunk_plugins: RwLock::new(Vec::new()),
            enabled: RwLock::new(std::collections::HashSet::new()),
        }
    }

    // ── Registration ──────────────────────────────────────────────────

    pub fn register_document_plugin(&self, plugin: Arc<dyn DocumentPlugin>) {
        let name = plugin.name().to_string();
        self.document_plugins.write().unwrap().push(plugin);
        // Enable by default on registration
        self.enabled.write().unwrap().insert(name);
    }

    pub fn register_search_plugin(&self, plugin: Arc<dyn SearchPlugin>) {
        let name = plugin.name().to_string();
        self.search_plugins.write().unwrap().push(plugin);
        self.enabled.write().unwrap().insert(name);
    }

    pub fn register_chunk_plugin(&self, plugin: Arc<dyn ChunkPlugin>) {
        let name = plugin.name().to_string();
        self.chunk_plugins.write().unwrap().push(plugin);
        self.enabled.write().unwrap().insert(name);
    }

    // ── Enable / Disable ──────────────────────────────────────────────

    /// Enable a plugin by name. Returns `true` if the plugin exists.
    pub fn enable(&self, name: &str) -> bool {
        if self.plugin_exists(name) {
            self.enabled.write().unwrap().insert(name.to_string());
            true
        } else {
            false
        }
    }

    /// Disable a plugin by name. Returns `true` if the plugin exists.
    pub fn disable(&self, name: &str) -> bool {
        if self.plugin_exists(name) {
            self.enabled.write().unwrap().remove(name);
            true
        } else {
            false
        }
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.read().unwrap().contains(name)
    }

    /// Bulk-set the enabled plugin list (used at startup from config/KV store).
    pub fn set_enabled_plugins(&self, names: &[String]) {
        let mut enabled = self.enabled.write().unwrap();
        enabled.clear();
        for name in names {
            if self.plugin_exists(name) {
                enabled.insert(name.clone());
            }
        }
    }

    fn plugin_exists(&self, name: &str) -> bool {
        self.document_plugins
            .read()
            .unwrap()
            .iter()
            .any(|p| p.name() == name)
            || self
                .search_plugins
                .read()
                .unwrap()
                .iter()
                .any(|p| p.name() == name)
            || self
                .chunk_plugins
                .read()
                .unwrap()
                .iter()
                .any(|p| p.name() == name)
    }

    // ── Queries ───────────────────────────────────────────────────────

    /// List all registered plugins with their enabled status.
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        let enabled = self.enabled.read().unwrap();
        let mut out = Vec::new();

        for p in self.document_plugins.read().unwrap().iter() {
            out.push(PluginInfo {
                name: p.name().to_string(),
                description: p.description().to_string(),
                plugin_type: PluginType::Document,
                enabled: enabled.contains(p.name()),
            });
        }
        for p in self.search_plugins.read().unwrap().iter() {
            out.push(PluginInfo {
                name: p.name().to_string(),
                description: p.description().to_string(),
                plugin_type: PluginType::Search,
                enabled: enabled.contains(p.name()),
            });
        }
        for p in self.chunk_plugins.read().unwrap().iter() {
            out.push(PluginInfo {
                name: p.name().to_string(),
                description: p.description().to_string(),
                plugin_type: PluginType::Chunk,
                enabled: enabled.contains(p.name()),
            });
        }
        out
    }

    /// Get the first enabled document plugin that supports the given MIME type.
    pub fn get_document_plugin(&self, mime_type: &str) -> Option<Arc<dyn DocumentPlugin>> {
        let enabled = self.enabled.read().unwrap();
        self.document_plugins
            .read()
            .unwrap()
            .iter()
            .find(|p| {
                enabled.contains(p.name())
                    && p.supported_mime_types()
                        .iter()
                        .any(|m| m == mime_type || m == "*/*")
            })
            .cloned()
    }

    /// Get all enabled search plugins (applied in registration order).
    pub fn get_search_plugins(&self) -> Vec<Arc<dyn SearchPlugin>> {
        let enabled = self.enabled.read().unwrap();
        self.search_plugins
            .read()
            .unwrap()
            .iter()
            .filter(|p| enabled.contains(p.name()))
            .cloned()
            .collect()
    }

    /// Get all enabled chunk plugins (applied in registration order).
    pub fn get_chunk_plugins(&self) -> Vec<Arc<dyn ChunkPlugin>> {
        let enabled = self.enabled.read().unwrap();
        self.chunk_plugins
            .read()
            .unwrap()
            .iter()
            .filter(|p| enabled.contains(p.name()))
            .cloned()
            .collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::SearchResult;

    struct TestDocPlugin;
    impl DocumentPlugin for TestDocPlugin {
        fn name(&self) -> &str {
            "test-doc"
        }
        fn description(&self) -> &str {
            "Test document plugin"
        }
        fn supported_mime_types(&self) -> Vec<String> {
            vec!["text/html".to_string()]
        }
        fn process(&self, content: &str, _mime_type: &str) -> thairag_core::error::Result<String> {
            Ok(content.replace("<meta", ""))
        }
    }

    struct TestSearchPlugin;
    impl SearchPlugin for TestSearchPlugin {
        fn name(&self) -> &str {
            "test-search"
        }
        fn description(&self) -> &str {
            "Test search plugin"
        }
        fn pre_search(&self, query: &str) -> String {
            format!("{query} expanded")
        }
        fn post_search(&self, results: Vec<SearchResult>) -> Vec<SearchResult> {
            results
        }
    }

    struct TestChunkPlugin;
    impl ChunkPlugin for TestChunkPlugin {
        fn name(&self) -> &str {
            "test-chunk"
        }
        fn description(&self) -> &str {
            "Test chunk plugin"
        }
        fn transform_chunk(&self, chunk: &str) -> String {
            format!("[HEADER] {chunk}")
        }
    }

    #[test]
    fn register_and_list() {
        let reg = PluginRegistry::new();
        reg.register_document_plugin(Arc::new(TestDocPlugin));
        reg.register_search_plugin(Arc::new(TestSearchPlugin));
        reg.register_chunk_plugin(Arc::new(TestChunkPlugin));

        let plugins = reg.list_plugins();
        assert_eq!(plugins.len(), 3);
        assert!(plugins.iter().all(|p| p.enabled));
    }

    #[test]
    fn enable_disable() {
        let reg = PluginRegistry::new();
        reg.register_document_plugin(Arc::new(TestDocPlugin));

        assert!(reg.is_enabled("test-doc"));
        reg.disable("test-doc");
        assert!(!reg.is_enabled("test-doc"));
        assert!(reg.get_document_plugin("text/html").is_none());

        reg.enable("test-doc");
        assert!(reg.is_enabled("test-doc"));
        assert!(reg.get_document_plugin("text/html").is_some());
    }

    #[test]
    fn mime_type_matching() {
        let reg = PluginRegistry::new();
        reg.register_document_plugin(Arc::new(TestDocPlugin));

        assert!(reg.get_document_plugin("text/html").is_some());
        assert!(reg.get_document_plugin("text/plain").is_none());
    }

    #[test]
    fn nonexistent_plugin_returns_false() {
        let reg = PluginRegistry::new();
        assert!(!reg.enable("nonexistent"));
        assert!(!reg.disable("nonexistent"));
    }
}
