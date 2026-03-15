use std::sync::Arc;

use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::query_analyzer::QueryLanguage;

/// Agent 6: Language Adapter.
/// Ensures response language matches the user's input language.
pub struct LanguageAdapter {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
}

impl LanguageAdapter {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self { llm, max_tokens }
    }

    /// Check if the response matches the expected language; if not, adapt it.
    pub async fn adapt(
        &self,
        response: &str,
        expected_language: &QueryLanguage,
    ) -> Result<String> {
        // Quick heuristic: check if the response is already in the right language
        if matches_language(response, expected_language) {
            return Ok(response.to_string());
        }

        debug!(
            expected = ?expected_language,
            "Response language mismatch detected, adapting"
        );

        let target = match expected_language {
            QueryLanguage::Thai => "Thai (ภาษาไทย)",
            QueryLanguage::English => "English",
            QueryLanguage::Mixed => return Ok(response.to_string()), // Mixed is acceptable as-is
        };

        let system = ChatMessage {
            role: "system".into(),
            content: format!(
                "Translate/rewrite the following response into {target}.\n\n\
                Rules:\n\
                - Preserve all [1], [2] citation markers exactly\n\
                - Preserve technical terms that don't have good translations\n\
                - Maintain the same structure and meaning\n\
                - Output ONLY the translated response, nothing else"
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: response.to_string(),
        };

        match self.llm.generate(&[system, user], Some(self.max_tokens)).await {
            Ok(resp) => {
                let adapted = resp.content.trim().to_string();
                if adapted.is_empty() {
                    Ok(response.to_string())
                } else {
                    Ok(adapted)
                }
            }
            Err(e) => {
                warn!(error = %e, "Language adaptation failed, returning original");
                Ok(response.to_string())
            }
        }
    }
}

/// Heuristic check: does the response match the expected language?
fn matches_language(text: &str, expected: &QueryLanguage) -> bool {
    let thai_chars = text.chars().filter(|c| ('\u{0E01}'..='\u{0E5B}').contains(c)).count();
    let total_alpha: usize = text.chars().filter(|c| c.is_alphabetic()).count();

    if total_alpha == 0 {
        return true; // No text to check
    }

    let thai_ratio = thai_chars as f32 / total_alpha as f32;

    match expected {
        QueryLanguage::Thai => thai_ratio > 0.3, // At least 30% Thai
        QueryLanguage::English => thai_ratio < 0.1, // Less than 10% Thai
        QueryLanguage::Mixed => true, // Mixed is always acceptable
    }
}
