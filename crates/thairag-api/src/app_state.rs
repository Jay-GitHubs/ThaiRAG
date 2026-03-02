use std::sync::Arc;

use thairag_agent::{QueryOrchestrator, RagEngine};
use thairag_auth::JwtService;
use thairag_config::AppConfig;
use thairag_core::traits::{EmbeddingModel, LlmProvider, Reranker, TextSearch, VectorStore};
use thairag_document::DocumentPipeline;
use thairag_search::HybridSearchEngine;

use thairag_provider_embedding::create_embedding_provider;
use thairag_provider_llm::create_llm_provider;
use thairag_provider_reranker::create_reranker;
use thairag_provider_search::create_text_search;
use thairag_provider_vectordb::create_vector_store;

use crate::store::KmStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub jwt: Option<Arc<JwtService>>,
    pub orchestrator: Arc<QueryOrchestrator>,
    pub document_pipeline: Arc<DocumentPipeline>,
    pub search_engine: Arc<HybridSearchEngine>,
    pub km_store: Arc<KmStore>,
}

impl AppState {
    pub fn build(config: AppConfig) -> Self {
        // Create providers
        let llm: Arc<dyn LlmProvider> = Arc::from(create_llm_provider(&config.providers.llm));
        let embedding: Arc<dyn EmbeddingModel> =
            Arc::from(create_embedding_provider(&config.providers.embedding));
        let vector_store: Arc<dyn VectorStore> =
            Arc::from(create_vector_store(&config.providers.vector_store));
        let text_search: Arc<dyn TextSearch> =
            Arc::from(create_text_search(&config.providers.text_search));
        let reranker: Arc<dyn Reranker> =
            Arc::from(create_reranker(&config.providers.reranker));

        // Build hybrid search
        let search_engine = Arc::new(HybridSearchEngine::new(
            embedding,
            vector_store,
            text_search,
            reranker,
            config.search.clone(),
        ));

        // Build agents
        let rag_engine = Arc::new(RagEngine::new(Arc::clone(&llm), Arc::clone(&search_engine)));
        let orchestrator = Arc::new(QueryOrchestrator::new(Arc::clone(&llm), rag_engine));

        // Build document pipeline
        let document_pipeline = Arc::new(DocumentPipeline::new(
            config.document.max_chunk_size,
            config.document.chunk_overlap,
        ));

        // JWT service (only if auth enabled)
        let jwt = if config.auth.enabled {
            Some(Arc::new(JwtService::new(
                &config.auth.jwt_secret,
                config.auth.token_expiry_hours,
            )))
        } else {
            None
        };

        let km_store = Arc::new(KmStore::new());

        Self {
            config: Arc::new(config),
            jwt,
            orchestrator,
            document_pipeline,
            search_engine,
            km_store,
        }
    }
}
