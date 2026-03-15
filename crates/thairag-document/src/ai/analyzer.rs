use std::sync::Arc;

use async_trait::async_trait;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{DocumentAnalyzer, LlmProvider};
use thairag_core::types::{ChatMessage, DocumentAnalysis, ImageContent, VisionMessage};
use tracing::warn;

use super::prompts;

/// LLM-powered document analyzer.
/// Detects language, structure, content type, and OCR artifacts.
pub struct LlmDocumentAnalyzer {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    /// Max chars to send as excerpt (analysis doesn't need the full doc).
    excerpt_chars: usize,
    prompts: Arc<PromptRegistry>,
}

impl LlmDocumentAnalyzer {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            excerpt_chars: 3000,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(llm: Arc<dyn LlmProvider>, max_tokens: u32, prompts: Arc<PromptRegistry>) -> Self {
        Self {
            llm,
            max_tokens,
            excerpt_chars: 3000,
            prompts,
        }
    }

    /// Default excerpt size used for first analysis attempt.
    pub fn default_excerpt_chars(&self) -> usize {
        self.excerpt_chars
    }

    /// Whether the underlying LLM supports vision.
    pub fn supports_vision(&self) -> bool {
        self.llm.supports_vision()
    }

    /// Vision-based analysis: sends the document image directly to the LLM.
    pub async fn analyze_with_vision(
        &self,
        raw_bytes: &[u8],
        mime_type: &str,
        raw_text: &str,
        doc_size_bytes: usize,
    ) -> Result<DocumentAnalysis> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw_bytes);
        let media_type = if mime_type.starts_with("image/") {
            mime_type.to_string()
        } else {
            // For PDFs and other docs, send as-is — vision models handle it
            mime_type.to_string()
        };

        let prompt = prompts::analyzer_vision_prompt(&self.prompts, mime_type, doc_size_bytes, raw_text);
        let messages = vec![VisionMessage {
            role: "user".into(),
            text: prompt,
            images: vec![ImageContent {
                base64_data: b64,
                media_type,
            }],
        }];

        let response = self.llm.generate_vision(&messages, Some(self.max_tokens)).await?;
        let json_str = strip_json_fences(response.content.trim());

        match serde_json::from_str::<DocumentAnalysis>(json_str) {
            Ok(analysis) => Ok(analysis),
            Err(e) => {
                warn!(error = %e, "Failed to parse vision analyzer response, using defaults");
                Ok(DocumentAnalysis::default())
            }
        }
    }

    /// Analyze with a specific excerpt size (for retry with larger excerpt).
    pub async fn analyze_with_excerpt_size(
        &self,
        raw_text: &str,
        mime_type: &str,
        doc_size_bytes: usize,
        excerpt_chars: usize,
    ) -> Result<DocumentAnalysis> {
        let excerpt = if raw_text.len() > excerpt_chars {
            &raw_text[..super::floor_char_boundary(raw_text, excerpt_chars)]
        } else {
            raw_text
        };

        let prompt = prompts::analyzer_prompt(&self.prompts, excerpt, mime_type, doc_size_bytes);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let content = response.content.trim();
        let json_str = strip_json_fences(content);

        match serde_json::from_str::<DocumentAnalysis>(json_str) {
            Ok(analysis) => Ok(analysis),
            Err(e) => {
                warn!(error = %e, "Failed to parse analyzer response, using defaults");
                Ok(DocumentAnalysis::default())
            }
        }
    }
}

#[async_trait]
impl DocumentAnalyzer for LlmDocumentAnalyzer {
    async fn analyze(&self, raw_text: &str, mime_type: &str, doc_size_bytes: usize) -> Result<DocumentAnalysis> {
        let excerpt = if raw_text.len() > self.excerpt_chars {
            &raw_text[..super::floor_char_boundary(raw_text, self.excerpt_chars)]
        } else {
            raw_text
        };

        let prompt = prompts::analyzer_prompt(&self.prompts, excerpt, mime_type, doc_size_bytes);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let content = response.content.trim();

        // Strip markdown fences if LLM wrapped it
        let json_str = strip_json_fences(content);

        match serde_json::from_str::<DocumentAnalysis>(json_str) {
            Ok(analysis) => Ok(analysis),
            Err(e) => {
                warn!(error = %e, "Failed to parse analyzer response, using defaults");
                Ok(DocumentAnalysis::default())
            }
        }
    }
}

/// Strip ```json ... ``` fences that LLMs sometimes add despite instructions.
pub(crate) fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = s.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        s
    }
}
