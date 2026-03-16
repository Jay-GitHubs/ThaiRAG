pub mod chromadb;
pub mod in_memory;
pub mod milvus;
pub mod personal_memory_inmemory;
pub mod personal_memory_qdrant;
pub mod pgvector;
pub mod pinecone;
pub mod qdrant;
pub mod routed;
pub mod weaviate;

use std::sync::Arc;

use thairag_config::schema::VectorStoreConfig;
use thairag_core::traits::{PersonalMemoryStore, VectorStore};
use thairag_core::types::{VectorIsolation, VectorStoreKind};

/// Create the underlying vector store without routing.
/// Used internally by `RoutedVectorStore` to create per-collection instances.
pub fn create_raw_vector_store(config: &VectorStoreConfig) -> Box<dyn VectorStore> {
    match config.kind {
        VectorStoreKind::InMemory => Box::new(in_memory::InMemoryVectorStore::new()),
        VectorStoreKind::Qdrant => Box::new(qdrant::QdrantVectorStore::new(
            &config.url,
            &config.collection,
        )),
        VectorStoreKind::Pgvector => Box::new(pgvector::PgvectorStore::new(
            &config.url,
            &config.collection,
        )),
        VectorStoreKind::ChromaDb => Box::new(chromadb::ChromaDbVectorStore::new(
            &config.url,
            &config.collection,
        )),
        VectorStoreKind::Pinecone => Box::new(pinecone::PineconeVectorStore::new(
            &config.url,
            &config.api_key,
        )),
        VectorStoreKind::Weaviate => Box::new(weaviate::WeaviateVectorStore::new(
            &config.url,
            &config.collection,
            &config.api_key,
        )),
        VectorStoreKind::Milvus => Box::new(milvus::MilvusVectorStore::new(
            &config.url,
            &config.collection,
        )),
    }
}

/// Create a vector store with isolation routing.
/// For `Shared` isolation, this is functionally equivalent to `create_raw_vector_store`.
/// For `PerOrganization` or `PerWorkspace`, wraps in a `RoutedVectorStore`.
pub fn create_vector_store(config: &VectorStoreConfig) -> Box<dyn VectorStore> {
    match config.isolation {
        VectorIsolation::Shared => create_raw_vector_store(config),
        _ => Box::new(routed::RoutedVectorStore::new(config.clone())),
    }
}

/// Create a personal memory store backed by the same vector database provider.
pub fn create_personal_memory_store(
    config: &VectorStoreConfig,
    embedding_dimension: usize,
) -> Arc<dyn PersonalMemoryStore> {
    match config.kind {
        VectorStoreKind::Qdrant => {
            Arc::new(personal_memory_qdrant::QdrantPersonalMemoryStore::new(
                &config.url,
                embedding_dimension,
            ))
        }
        // All other backends use the in-memory implementation
        _ => Arc::new(personal_memory_inmemory::InMemoryPersonalMemoryStore::new()),
    }
}
