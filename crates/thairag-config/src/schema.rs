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

impl AppConfig {
    pub fn validate(&self) -> std::result::Result<(), String> {
        let require = |field: &str, value: &str| -> std::result::Result<(), String> {
            if value.trim().is_empty() {
                Err(format!("{field} must not be empty"))
            } else {
                Ok(())
            }
        };

        let p = &self.providers;

        // LLM
        match p.llm.kind {
            LlmKind::Ollama => require("providers.llm.base_url", &p.llm.base_url)?,
            LlmKind::Claude | LlmKind::OpenAi => {
                require("providers.llm.api_key", &p.llm.api_key)?
            }
        }

        // Embedding
        if p.embedding.kind == EmbeddingKind::OpenAi {
            require("providers.embedding.api_key", &p.embedding.api_key)?;
        }

        // Vector store
        if p.vector_store.kind == VectorStoreKind::Qdrant {
            require("providers.vector_store.url", &p.vector_store.url)?;
            require("providers.vector_store.collection", &p.vector_store.collection)?;
        }

        // Reranker
        if p.reranker.kind == RerankerKind::Cohere {
            require("providers.reranker.api_key", &p.reranker.api_key)?;
            require("providers.reranker.model", &p.reranker.model)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
}

fn default_shutdown_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_limit_enabled")]
    pub enabled: bool,
    #[serde(default = "default_requests_per_second")]
    pub requests_per_second: u64,
    #[serde(default = "default_burst_size")]
    pub burst_size: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_second: 10,
            burst_size: 20,
        }
    }
}

fn default_rate_limit_enabled() -> bool {
    true
}

fn default_requests_per_second() -> u64 {
    10
}

fn default_burst_size() -> u64 {
    20
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

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{
        EmbeddingKind, LlmKind, RerankerKind, TextSearchKind, VectorStoreKind,
    };

    fn free_tier_config() -> AppConfig {
        AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 3000,
                shutdown_timeout_secs: 30,
                rate_limit: RateLimitConfig::default(),
            },
            database: DatabaseConfig {
                url: "sqlite://data.db".into(),
                max_connections: 5,
            },
            auth: AuthConfig {
                enabled: false,
                jwt_secret: "secret".into(),
                token_expiry_hours: 24,
            },
            providers: ProvidersConfig {
                llm: LlmConfig {
                    kind: LlmKind::Ollama,
                    model: "llama3".into(),
                    base_url: "http://localhost:11434".into(),
                    api_key: String::new(),
                },
                embedding: EmbeddingConfig {
                    kind: EmbeddingKind::Fastembed,
                    model: "all-MiniLM-L6-v2".into(),
                    dimension: 384,
                    api_key: String::new(),
                },
                vector_store: VectorStoreConfig {
                    kind: VectorStoreKind::InMemory,
                    url: String::new(),
                    collection: String::new(),
                },
                text_search: TextSearchConfig {
                    kind: TextSearchKind::Tantivy,
                    index_path: "./data/tantivy_index".into(),
                },
                reranker: RerankerConfig {
                    kind: RerankerKind::Passthrough,
                    model: String::new(),
                    api_key: String::new(),
                },
            },
            search: SearchConfig {
                top_k: 5,
                rerank_top_k: 3,
                rrf_k: 60,
                vector_weight: 0.5,
                text_weight: 0.5,
            },
            document: DocumentConfig {
                max_chunk_size: 512,
                chunk_overlap: 64,
            },
        }
    }

    #[test]
    fn validate_free_tier_ok() {
        assert!(free_tier_config().validate().is_ok());
    }

    #[test]
    fn validate_missing_llm_api_key() {
        let mut cfg = free_tier_config();
        cfg.providers.llm.kind = LlmKind::Claude;
        cfg.providers.llm.api_key = String::new();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.llm.api_key"), "got: {err}");
    }

    #[test]
    fn validate_missing_ollama_base_url() {
        let mut cfg = free_tier_config();
        cfg.providers.llm.base_url = "  ".into();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.llm.base_url"), "got: {err}");
    }

    #[test]
    fn validate_missing_qdrant_fields() {
        let mut cfg = free_tier_config();
        cfg.providers.vector_store.kind = VectorStoreKind::Qdrant;
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.vector_store.url"), "got: {err}");
    }

    #[test]
    fn validate_missing_cohere_api_key() {
        let mut cfg = free_tier_config();
        cfg.providers.reranker.kind = RerankerKind::Cohere;
        cfg.providers.reranker.model = "rerank-v3".into();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.reranker.api_key"), "got: {err}");
    }
}
