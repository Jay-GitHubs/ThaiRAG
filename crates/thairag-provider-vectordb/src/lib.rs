pub mod chromadb;
pub mod in_memory;
pub mod milvus;
pub mod pgvector;
pub mod pinecone;
pub mod qdrant;
pub mod routed;
pub mod weaviate;

use thairag_config::schema::VectorStoreConfig;
use thairag_core::traits::VectorStore;
use thairag_core::types::{VectorIsolation, VectorStoreKind};

/// Create the underlying vector store without routing.
/// Used internally by `RoutedVectorStore` to create per-collection instances.
pub fn create_raw_vector_store(config: &VectorStoreConfig) -> Box<dyn VectorStore> {
    match config.kind {
        VectorStoreKind::InMemory => Box::new(in_memory::InMemoryVectorStore::new()),
        VectorStoreKind::Qdrant => {
            Box::new(qdrant::QdrantVectorStore::new(&config.url, &config.collection))
        }
        VectorStoreKind::Pgvector => {
            Box::new(pgvector::PgvectorStore::new(&config.url, &config.collection))
        }
        VectorStoreKind::ChromaDb => {
            Box::new(chromadb::ChromaDbVectorStore::new(&config.url, &config.collection))
        }
        VectorStoreKind::Pinecone => {
            Box::new(pinecone::PineconeVectorStore::new(&config.url, &config.api_key))
        }
        VectorStoreKind::Weaviate => Box::new(weaviate::WeaviateVectorStore::new(
            &config.url,
            &config.collection,
            &config.api_key,
        )),
        VectorStoreKind::Milvus => {
            Box::new(milvus::MilvusVectorStore::new(&config.url, &config.collection))
        }
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
