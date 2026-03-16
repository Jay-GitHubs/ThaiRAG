use std::sync::Arc;

use async_trait::async_trait;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{LlmProvider, QualityChecker};
use thairag_core::types::{
    ChatMessage, ConvertedDocument, ImageContent, QualityReport, VisionMessage,
};
use tracing::warn;

use super::analyzer::strip_json_fences;
use super::prompts;

/// LLM-powered quality checker.
/// Compares original text with converted markdown to score coherence,
/// completeness, and formatting.
pub struct LlmQualityChecker {
    llm: Arc<dyn LlmProvider>,
    quality_threshold: f32,
    max_tokens: u32,
    /// Max chars to sample from head/tail for comparison.
    sample_chars: usize,
    prompts: Arc<PromptRegistry>,
}

impl LlmQualityChecker {
    pub fn new(llm: Arc<dyn LlmProvider>, quality_threshold: f32, max_tokens: u32) -> Self {
        Self {
            llm,
            quality_threshold,
            max_tokens,
            sample_chars: 1500,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        quality_threshold: f32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            quality_threshold,
            max_tokens,
            sample_chars: 1500,
            prompts,
        }
    }

    /// Whether the underlying LLM supports vision.
    pub fn supports_vision(&self) -> bool {
        self.llm.supports_vision()
    }

    /// Vision-based quality check: compares original document image against converted Markdown.
    pub async fn check_with_vision(
        &self,
        raw_bytes: &[u8],
        mime_type: &str,
        original_text: &str,
        converted: &ConvertedDocument,
    ) -> Result<QualityReport> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw_bytes);
        let media_type = mime_type.to_string();

        let converted_sample = sample_head_tail(&converted.markdown, self.sample_chars);
        let prompt =
            prompts::quality_checker_vision_prompt(&self.prompts, &converted_sample, original_text);
        let messages = vec![VisionMessage {
            role: "user".into(),
            text: prompt,
            images: vec![ImageContent {
                base64_data: b64,
                media_type,
            }],
        }];

        let response = self
            .llm
            .generate_vision(&messages, Some(self.max_tokens))
            .await?;
        let json_str = super::analyzer::strip_json_fences(response.content.trim());

        match serde_json::from_str::<RawQualityScores>(json_str) {
            Ok(scores) => {
                let overall = 0.4 * scores.coherence_score
                    + 0.4 * scores.completeness_score
                    + 0.2 * scores.formatting_score;

                Ok(QualityReport {
                    overall_score: overall,
                    coherence_score: scores.coherence_score,
                    completeness_score: scores.completeness_score,
                    formatting_score: scores.formatting_score,
                    issues: scores.issues,
                    passed: overall >= self.quality_threshold,
                })
            }
            Err(e) => {
                warn!(error = %e, "Failed to parse vision quality report, assuming pass");
                Ok(QualityReport {
                    overall_score: 0.75,
                    coherence_score: 0.75,
                    completeness_score: 0.75,
                    formatting_score: 0.75,
                    issues: vec!["Vision quality check response was unparseable".into()],
                    passed: true,
                })
            }
        }
    }
}

#[async_trait]
impl QualityChecker for LlmQualityChecker {
    async fn check(
        &self,
        original_text: &str,
        converted: &ConvertedDocument,
    ) -> Result<QualityReport> {
        let original_sample = sample_head_tail(original_text, self.sample_chars);
        let converted_sample = sample_head_tail(&converted.markdown, self.sample_chars);

        let prompt =
            prompts::quality_checker_prompt(&self.prompts, &original_sample, &converted_sample);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let json_str = strip_json_fences(response.content.trim());

        match serde_json::from_str::<RawQualityScores>(json_str) {
            Ok(scores) => {
                let overall = 0.4 * scores.coherence_score
                    + 0.4 * scores.completeness_score
                    + 0.2 * scores.formatting_score;

                Ok(QualityReport {
                    overall_score: overall,
                    coherence_score: scores.coherence_score,
                    completeness_score: scores.completeness_score,
                    formatting_score: scores.formatting_score,
                    issues: scores.issues,
                    passed: overall >= self.quality_threshold,
                })
            }
            Err(e) => {
                warn!(error = %e, "Failed to parse quality report, assuming pass");
                // If we can't parse, assume quality is acceptable to avoid
                // discarding potentially good AI output
                Ok(QualityReport {
                    overall_score: 0.75,
                    coherence_score: 0.75,
                    completeness_score: 0.75,
                    formatting_score: 0.75,
                    issues: vec!["Quality check response was unparseable".into()],
                    passed: true,
                })
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct RawQualityScores {
    coherence_score: f32,
    completeness_score: f32,
    formatting_score: f32,
    #[serde(default)]
    issues: Vec<String>,
}

/// Sample head + tail of text for comparison (keeps cost low).
fn sample_head_tail(text: &str, max_per_side: usize) -> String {
    if text.len() <= max_per_side * 2 {
        return text.to_string();
    }

    let head_end = super::floor_char_boundary(text, max_per_side);
    let tail_start = super::floor_char_boundary(text, text.len().saturating_sub(max_per_side));

    format!(
        "{}\n\n[... middle section omitted ...]\n\n{}",
        &text[..head_end],
        &text[tail_start..]
    )
}
