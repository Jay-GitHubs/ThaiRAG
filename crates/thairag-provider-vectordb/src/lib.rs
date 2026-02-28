pub mod in_memory;
pub mod qdrant;

use thairag_config::schema::VectorStoreConfig;
use thairag_core::traits::VectorStore;
use thairag_core::types::VectorStoreKind;

pub fn create_vector_store(config: &VectorStoreConfig) -> Box<dyn VectorStore> {
    match config.kind {
        VectorStoreKind::InMemory => Box::new(in_memory::InMemoryVectorStore::new()),
        VectorStoreKind::Qdrant => {
            Box::new(qdrant::QdrantVectorStore::new(&config.url, &config.collection))
        }
    }
}
