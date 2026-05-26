use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThaiRagError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("LLM provider error: {0}")]
    LlmProvider(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Vector store error: {0}")]
    VectorStore(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Authorization error: {0}")]
    Authorization(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal error: {0}")]
    Internal(String),

    /// Document ingestion produced no usable text content.
    /// `reason` is a stable short code for surfacing in UIs.
    /// `hint` is operator-facing guidance on how to fix it.
    #[error("Empty extraction [{reason}]: {hint}")]
    EmptyExtraction { reason: String, hint: String },
}

impl ThaiRagError {
    /// Construct an [`EmptyExtraction`] error with a stable reason code + hint.
    pub fn empty_extraction(reason: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::EmptyExtraction {
            reason: reason.into(),
            hint: hint.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, ThaiRagError>;
