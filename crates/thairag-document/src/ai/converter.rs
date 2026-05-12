use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{AiDocumentConverter, LlmProvider};
use thairag_core::types::{
    ChatMessage, ConvertedDocument, DocumentAnalysis, ImageContent, VisionMessage,
};
use tracing::{info, warn};

use super::prompts;

/// LLM-powered document converter.
/// Converts raw text to clean Markdown, processing in segments for large docs.
/// Supports a vision path for OCR documents when the LLM supports vision.
pub struct LlmDocumentConverter {
    llm: Arc<dyn LlmProvider>,
    max_input_chars: usize,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl LlmDocumentConverter {
    pub fn new(llm: Arc<dyn LlmProvider>, max_input_chars: usize, max_tokens: u32) -> Self {
        Self {
            llm,
            max_input_chars,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_input_chars: usize,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_input_chars,
            max_tokens,
            prompts,
        }
    }

    /// Whether this converter can use vision (image) input for OCR documents.
    pub fn supports_vision(&self) -> bool {
        self.llm.supports_vision()
    }

    /// Convert a document using vision — sends the raw document image/PDF to the vision model.
    /// This produces much better results for scanned/OCR documents than text-only conversion.
    pub async fn convert_with_vision(
        &self,
        raw_bytes: &[u8],
        mime_type: &str,
        raw_text: &str,
        analysis: &DocumentAnalysis,
    ) -> Result<ConvertedDocument> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw_bytes);
        let content_type = format!("{:?}", analysis.content_type).to_lowercase();

        let prompt = prompts::converter_vision_prompt(
            &self.prompts,
            &analysis.primary_language,
            &content_type,
            analysis.has_headers_footers,
            raw_text,
        );

        let messages = vec![VisionMessage {
            role: "user".into(),
            text: prompt,
            images: vec![ImageContent {
                base64_data: b64,
                media_type: mime_type.to_string(),
            }],
        }];

        info!(
            model = self.llm.model_name(),
            mime_type,
            doc_size = raw_bytes.len(),
            "Converting document with vision model"
        );

        let response = self
            .llm
            .generate_vision(&messages, Some(self.max_tokens))
            .await?;

        Ok(ConvertedDocument {
            markdown: response.content,
            analysis: analysis.clone(),
        })
    }

    /// Convert a single page using vision — sends the full document + page context.
    pub async fn convert_page_with_vision(
        &self,
        raw_bytes: &[u8],
        mime_type: &str,
        page_text: &str,
        analysis: &DocumentAnalysis,
        page_num: usize,
        total_pages: usize,
    ) -> Result<String> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw_bytes);
        let content_type = format!("{:?}", analysis.content_type).to_lowercase();

        let prompt = prompts::converter_vision_page_prompt(
            &self.prompts,
            &analysis.primary_language,
            &content_type,
            analysis.has_headers_footers,
            page_text,
            page_num,
            total_pages,
        );

        let messages = vec![VisionMessage {
            role: "user".into(),
            text: prompt,
            images: vec![ImageContent {
                base64_data: b64,
                media_type: mime_type.to_string(),
            }],
        }];

        let response = self
            .llm
            .generate_vision(&messages, Some(self.max_tokens))
            .await?;
        Ok(response.content)
    }
}

impl LlmDocumentConverter {
    /// Re-convert with quality feedback from a previous attempt.
    pub async fn convert_with_feedback(
        &self,
        raw_text: &str,
        analysis: &DocumentAnalysis,
        previous_markdown: &str,
        quality_issues: &[String],
    ) -> Result<ConvertedDocument> {
        let content_type = format!("{:?}", analysis.content_type).to_lowercase();
        let segments = split_at_paragraphs(raw_text, self.max_input_chars);
        let total = segments.len();

        info!(
            segments = total,
            retry = true,
            "Re-converting document with quality feedback"
        );

        let mut converted_parts = Vec::with_capacity(total);

        for (i, segment) in segments.iter().enumerate() {
            let prompt = prompts::converter_feedback_prompt(
                &self.prompts,
                segment,
                &analysis.primary_language,
                &content_type,
                analysis.needs_ocr_correction,
                analysis.has_headers_footers,
                None,
                previous_markdown,
                quality_issues,
            );

            let messages = vec![ChatMessage {
                role: "user".into(),
                content: prompt,
                images: vec![],
            }];

            match self.llm.generate(&messages, Some(self.max_tokens)).await {
                Ok(response) => {
                    converted_parts.push(response.content);
                }
                Err(e) => {
                    warn!(segment = i + 1, total, error = %e, "Feedback segment conversion failed, using raw text");
                    converted_parts.push(segment.to_string());
                }
            }
        }

        let markdown = converted_parts.join("\n\n");

        Ok(ConvertedDocument {
            markdown,
            analysis: analysis.clone(),
        })
    }

    /// Re-convert a single page with quality feedback.
    pub async fn convert_page_with_feedback(
        &self,
        page_text: &str,
        analysis: &DocumentAnalysis,
        page_num: usize,
        total_pages: usize,
        previous_markdown: &str,
        quality_issues: &[String],
    ) -> Result<String> {
        let content_type = format!("{:?}", analysis.content_type).to_lowercase();
        let segments = split_at_paragraphs(page_text, self.max_input_chars);
        let total_segments = segments.len();

        let mut converted_parts = Vec::with_capacity(total_segments);

        for (i, segment) in segments.iter().enumerate() {
            let prompt = prompts::converter_feedback_prompt(
                &self.prompts,
                segment,
                &analysis.primary_language,
                &content_type,
                analysis.needs_ocr_correction,
                analysis.has_headers_footers,
                Some((page_num, total_pages)),
                previous_markdown,
                quality_issues,
            );

            let messages = vec![ChatMessage {
                role: "user".into(),
                content: prompt,
                images: vec![],
            }];

            match self.llm.generate(&messages, Some(self.max_tokens)).await {
                Ok(response) => {
                    converted_parts.push(response.content);
                }
                Err(e) => {
                    warn!(page = page_num, segment = i + 1, total_segments, error = %e,
                        "Feedback page segment conversion failed, using raw text");
                    converted_parts.push(segment.to_string());
                }
            }
        }

        Ok(converted_parts.join("\n\n"))
    }

    /// Convert a single page with page context in the prompt.
    pub async fn convert_page(
        &self,
        page_text: &str,
        analysis: &DocumentAnalysis,
        page_num: usize,
        total_pages: usize,
    ) -> Result<String> {
        let content_type = format!("{:?}", analysis.content_type).to_lowercase();
        let segments = split_at_paragraphs(page_text, self.max_input_chars);
        let total_segments = segments.len();

        let mut converted_parts = Vec::with_capacity(total_segments);

        for (i, segment) in segments.iter().enumerate() {
            let prompt = prompts::converter_prompt_with_page(
                &self.prompts,
                segment,
                &analysis.primary_language,
                &content_type,
                analysis.needs_ocr_correction,
                analysis.has_headers_footers,
                Some((page_num, total_pages)),
            );

            let messages = vec![ChatMessage {
                role: "user".into(),
                content: prompt,
                images: vec![],
            }];

            match self.llm.generate(&messages, Some(self.max_tokens)).await {
                Ok(response) => {
                    converted_parts.push(response.content);
                }
                Err(e) => {
                    warn!(page = page_num, segment = i + 1, total_segments, error = %e,
                        "Page segment conversion failed, using raw text");
                    converted_parts.push(segment.to_string());
                }
            }
        }

        Ok(converted_parts.join("\n\n"))
    }
}

#[async_trait]
impl AiDocumentConverter for LlmDocumentConverter {
    async fn convert(
        &self,
        raw_text: &str,
        analysis: &DocumentAnalysis,
    ) -> Result<ConvertedDocument> {
        let content_type = format!("{:?}", analysis.content_type).to_lowercase();
        let segments = split_at_paragraphs(raw_text, self.max_input_chars);
        let total = segments.len();

        info!(segments = total, "Converting document with AI");

        let mut converted_parts = Vec::with_capacity(total);

        for (i, segment) in segments.iter().enumerate() {
            let prompt = prompts::converter_prompt(
                &self.prompts,
                segment,
                &analysis.primary_language,
                &content_type,
                analysis.needs_ocr_correction,
                analysis.has_headers_footers,
            );

            let messages = vec![ChatMessage {
                role: "user".into(),
                content: prompt,
                images: vec![],
            }];

            match self.llm.generate(&messages, Some(self.max_tokens)).await {
                Ok(response) => {
                    converted_parts.push(response.content);
                }
                Err(e) => {
                    warn!(segment = i + 1, total, error = %e, "Segment conversion failed, using raw text");
                    converted_parts.push(segment.to_string());
                }
            }
        }

        let markdown = converted_parts.join("\n\n");

        Ok(ConvertedDocument {
            markdown,
            analysis: analysis.clone(),
        })
    }
}

/// Split text into segments at paragraph boundaries, respecting max_chars.
fn split_at_paragraphs(text: &str, max_chars: usize) -> Vec<&str> {
    if text.len() <= max_chars {
        return vec![text];
    }

    let mut segments = Vec::new();
    let mut start = 0;

    while start < text.len() {
        if start + max_chars >= text.len() {
            segments.push(&text[start..]);
            break;
        }

        // Find the last paragraph break within the limit
        let search_end = super::floor_char_boundary(text, start + max_chars);
        let search_region = &text[start..search_end];

        let split_pos = if let Some(pos) = search_region.rfind("\n\n") {
            start + pos + 2 // after the double newline
        } else if let Some(pos) = search_region.rfind('\n') {
            start + pos + 1
        } else {
            search_end
        };

        segments.push(&text[start..split_pos]);
        start = split_pos;
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_text() {
        let result = split_at_paragraphs("hello world", 100);
        assert_eq!(result, vec!["hello world"]);
    }

    #[test]
    fn split_at_paragraph_boundary() {
        let text = "AAA\n\nBBB\n\nCCC";
        let result = split_at_paragraphs(text, 6);
        assert!(result.len() >= 2);
        assert!(result[0].starts_with("AAA"));
        // All segments together should cover the full text
        let reassembled: String = result.concat();
        assert_eq!(reassembled, text);
    }
}
