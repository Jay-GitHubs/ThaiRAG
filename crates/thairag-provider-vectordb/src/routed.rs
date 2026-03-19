use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use thairag_config::schema::VectorStoreConfig;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{
    DocId, DocumentChunk, OrgId, SearchQuery, SearchResult, VectorIsolation, WorkspaceId,
};
use tracing::{debug, info};

/// A VectorStore wrapper that routes operations to different collections
/// based on the configured isolation strategy.
pub struct RoutedVectorStore {
    isolation: VectorIsolation,
    base_config: VectorStoreConfig,
    /// Cache of collection_name → store instance.
    stores: RwLock<HashMap<String, Arc<dyn VectorStore>>>,
    /// Lookup: workspace_id → org_id (populated during upsert).
    ws_to_org: RwLock<HashMap<String, String>>,
}

impl RoutedVectorStore {
    pub fn new(config: VectorStoreConfig) -> Self {
        Self {
            isolation: config.isolation.clone(),
            base_config: config,
            stores: RwLock::new(HashMap::new()),
            ws_to_org: RwLock::new(HashMap::new()),
        }
    }

    /// Set the org_id mapping for a workspace (called by the ingestion layer).
    pub fn set_workspace_org(&self, workspace_id: &WorkspaceId, org_id: &OrgId) {
        let mut map = self.ws_to_org.write().unwrap();
        map.insert(workspace_id.to_string(), org_id.to_string());
    }

    fn collection_for_workspace(&self, workspace_id: &WorkspaceId) -> String {
        let base = if self.base_config.collection.is_empty() {
            "thairag_chunks"
        } else {
            &self.base_config.collection
        };

        match self.isolation {
            VectorIsolation::Shared => base.to_string(),
            VectorIsolation::PerOrganization => {
                let map = self.ws_to_org.read().unwrap();
                if let Some(org_id) = map.get(&workspace_id.to_string()) {
                    format!("{base}_org_{org_id}")
                } else {
                    // Fallback: use workspace_id as org proxy
                    debug!(%workspace_id, "No org mapping found, using workspace_id as fallback");
                    format!("{base}_org_{workspace_id}")
                }
            }
            VectorIsolation::PerWorkspace => {
                format!("{base}_ws_{workspace_id}")
            }
        }
    }

    fn get_or_create_store(&self, collection: &str) -> Arc<dyn VectorStore> {
        // Fast path: read lock
        {
            let stores = self.stores.read().unwrap();
            if let Some(store) = stores.get(collection) {
                return Arc::clone(store);
            }
        }

        // Slow path: write lock + create
        let mut stores = self.stores.write().unwrap();
        // Double-check after acquiring write lock
        if let Some(store) = stores.get(collection) {
            return Arc::clone(store);
        }

        info!(collection, isolation = ?self.isolation, "Creating vector store for collection");
        let mut config = self.base_config.clone();
        config.collection = collection.to_string();
        let store: Arc<dyn VectorStore> = Arc::from(super::create_raw_vector_store(&config));
        stores.insert(collection.to_string(), Arc::clone(&store));
        store
    }

    /// Get all known store instances (for broadcast operations like delete_by_doc).
    fn all_stores(&self) -> Vec<Arc<dyn VectorStore>> {
        let stores = self.stores.read().unwrap();
        stores.values().cloned().collect()
    }

    /// Resolve which collections to search based on workspace_ids.
    fn collections_for_query(&self, query: &SearchQuery) -> Vec<String> {
        match self.isolation {
            VectorIsolation::Shared => {
                let base = if self.base_config.collection.is_empty() {
                    "thairag_chunks".to_string()
                } else {
                    self.base_config.collection.clone()
                };
                vec![base]
            }
            VectorIsolation::PerOrganization => {
                if query.unrestricted {
                    // Search all known org collections
                    let stores = self.stores.read().unwrap();
                    stores.keys().cloned().collect()
                } else {
                    // Deduplicate org collections from workspace_ids
                    let mut collections = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for ws_id in &query.workspace_ids {
                        let col = self.collection_for_workspace(ws_id);
                        if seen.insert(col.clone()) {
                            collections.push(col);
                        }
                    }
                    collections
                }
            }
            VectorIsolation::PerWorkspace => {
                if query.unrestricted {
                    let stores = self.stores.read().unwrap();
                    stores.keys().cloned().collect()
                } else {
                    query
                        .workspace_ids
                        .iter()
                        .map(|ws_id| self.collection_for_workspace(ws_id))
                        .collect()
                }
            }
        }
    }
}

#[async_trait]
impl VectorStore for RoutedVectorStore {
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        match self.isolation {
            VectorIsolation::Shared => {
                let col = self.collection_for_workspace(&chunks[0].workspace_id);
                let store = self.get_or_create_store(&col);
                store.upsert(chunks).await
            }
            _ => {
                // Group chunks by collection
                let mut groups: HashMap<String, Vec<&DocumentChunk>> = HashMap::new();
                for chunk in chunks {
                    let col = self.collection_for_workspace(&chunk.workspace_id);
                    groups.entry(col).or_default().push(chunk);
                }

                for (col, group) in &groups {
                    let store = self.get_or_create_store(col);
                    let owned: Vec<DocumentChunk> = group.iter().map(|c| (*c).clone()).collect();
                    store.upsert(&owned).await?;
                }
                Ok(())
            }
        }
    }

    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let collections = self.collections_for_query(query);

        if collections.is_empty() {
            return Ok(vec![]);
        }

        if collections.len() == 1 {
            let store = self.get_or_create_store(&collections[0]);
            return store.search(embedding, query).await;
        }

        // Fan-out search across multiple collections
        let mut handles = Vec::new();
        for col in collections {
            let store = self.get_or_create_store(&col);
            let emb = embedding.to_vec();
            let q = query.clone();
            handles.push(tokio::spawn(async move { store.search(&emb, &q).await }));
        }

        let mut all_results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(results)) => all_results.extend(results),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "Search failed in one collection, continuing");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Search task panicked, continuing");
                }
            }
        }

        // Sort by score descending and truncate to top_k
        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_results.truncate(query.top_k);
        Ok(all_results)
    }

    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        // Broadcast delete to all known stores
        let stores = self.all_stores();
        if stores.is_empty() {
            // If no stores created yet, create default and try
            let base = if self.base_config.collection.is_empty() {
                "thairag_chunks"
            } else {
                &self.base_config.collection
            };
            let store = self.get_or_create_store(base);
            return store.delete_by_doc(doc_id).await;
        }

        for store in stores {
            let id = doc_id;
            if let Err(e) = store.delete_by_doc(id).await {
                tracing::warn!(error = %e, "delete_by_doc failed in one collection, continuing");
            }
        }
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let stores = self.all_stores();
        for store in stores {
            if let Err(e) = store.delete_all().await {
                tracing::warn!(error = %e, "delete_all failed in one collection, continuing");
            }
        }
        Ok(())
    }
}
