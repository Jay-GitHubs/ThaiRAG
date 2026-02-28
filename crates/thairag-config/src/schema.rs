use serde::Deserialize;
use thairag_core::types::{
    EmbeddingKind, LlmKind, RerankerKind, TextSearchKind, VectorStoreKind,
};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub providers: ProvidersConfig,
    pub search: SearchConfig,
    pub document: DocumentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub jwt_secret: String,
    pub token_expiry_hours: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProvidersConfig {
    pub llm: LlmConfig,
    pub embedding: EmbeddingConfig,
    pub vector_store: VectorStoreConfig,
    pub text_search: TextSearchConfig,
    pub reranker: RerankerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub kind: LlmKind,
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    pub kind: EmbeddingKind,
    pub model: String,
    pub dimension: usize,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VectorStoreConfig {
    pub kind: VectorStoreKind,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub collection: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextSearchConfig {
    pub kind: TextSearchKind,
    #[serde(default = "default_index_path")]
    pub index_path: String,
}

fn default_index_path() -> String {
    "./data/tantivy_index".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerankerConfig {
    pub kind: RerankerKind,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub top_k: usize,
    pub rerank_top_k: usize,
    pub rrf_k: usize,
    pub vector_weight: f32,
    pub text_weight: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocumentConfig {
    pub max_chunk_size: usize,
    pub chunk_overlap: usize,
}
