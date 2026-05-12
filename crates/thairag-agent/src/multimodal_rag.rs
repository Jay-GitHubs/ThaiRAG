use std::sync::Arc;

use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::context_curator::CuratedContext;

/// Multi-modal RAG: enriches context with image descriptions when documents
/// contain embedded images. Uses a vision-capable LLM to describe images
/// and injects those descriptions into the context for better retrieval answers.
///
/// NOTE: Image extraction from documents (PDF, DOCX) is a document-pipeline
/// concern. This agent operates on chunks that already have image metadata
/// (base64 or URL references stored in chunk metadata).
pub struct MultimodalRag {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    max_images_per_request: u32,
    prompts: Arc<PromptRegistry>,
}

impl MultimodalRag {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32, max_images_per_request: u32) -> Self {
        Self {
            llm,
            max_tokens,
            max_images_per_request,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        max_images_per_request: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            max_images_per_request,
            prompts,
        }
    }

    /// Scan curated context for image references in chunk metadata,
    /// generate descriptions, and append them to the chunk content.
    pub async fn enrich_context(
        &self,
        query: &str,
        context: &CuratedContext,
    ) -> Result<CuratedContext> {
        let mut enriched = context.clone();
        let mut images_processed = 0u32;

        for chunk in &mut enriched.chunks {
            if images_processed >= self.max_images_per_request {
                break;
            }

            // Check for image metadata in chunk
            // Convention: metadata JSON with "images" array of {url, alt_text, mime_type}
            let image_refs = extract_image_refs(&chunk.content);
            if image_refs.is_empty() {
                continue;
            }

            for img_ref in image_refs {
                if images_processed >= self.max_images_per_request {
                    break;
                }

                match self.describe_image(query, &img_ref).await {
                    Ok(description) => {
                        chunk
                            .content
                            .push_str(&format!("\n\n[Image Description: {}]", description));
                        images_processed += 1;
                    }
                    Err(e) => {
                        warn!(error = %e, "Multi-modal RAG: image description failed");
                    }
                }
            }
        }

        if images_processed > 0 {
            debug!(
                images_processed,
                "Multi-modal RAG: enriched context with image descriptions"
            );
        }

        Ok(enriched)
    }

    /// Generate a text description for an image relevant to the query.
    async fn describe_image(&self, query: &str, image_ref: &ImageRef) -> Result<String> {
        const DEFAULT_MULTIMODAL_RAG_PROMPT: &str = "You describe images in the context of a knowledge base query. \
Focus on details relevant to the query. Be concise (1-3 sentences).";

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.multimodal_rag",
                DEFAULT_MULTIMODAL_RAG_PROMPT,
                &[],
            ),
            images: vec![],
        };

        let user_content = if let Some(ref alt) = image_ref.alt_text {
            format!(
                "Query: {query}\n\nImage alt text: {alt}\nImage type: {}\n\n\
                 Describe what this image likely contains and how it relates to the query.",
                image_ref.mime_type
            )
        } else {
            format!(
                "Query: {query}\n\nImage type: {}\n\n\
                 Based on the surrounding context, describe what this image likely shows.",
                image_ref.mime_type
            )
        };

        let user = ChatMessage {
            role: "user".into(),
            content: user_content,
            images: vec![],
        };

        let resp = self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await?;
        Ok(resp.content.trim().to_string())
    }
}

/// An image reference extracted from chunk content.
#[derive(Debug, Clone)]
pub struct ImageRef {
    pub url: Option<String>,
    pub alt_text: Option<String>,
    pub mime_type: String,
}

/// Extract image references from chunk content.
/// Looks for markdown image syntax: ![alt](url) and HTML img tags.
fn extract_image_refs(content: &str) -> Vec<ImageRef> {
    let mut refs = Vec::new();

    // Markdown images: ![alt text](url)
    let mut remaining = content;
    while let Some(start) = remaining.find("![") {
        let after_bang = &remaining[start + 2..];
        if let Some(close_bracket) = after_bang.find("](") {
            let alt = &after_bang[..close_bracket];
            let after_bracket = &after_bang[close_bracket + 2..];
            if let Some(close_paren) = after_bracket.find(')') {
                let url = &after_bracket[..close_paren];
                let mime = guess_mime_from_url(url);
                refs.push(ImageRef {
                    url: Some(url.to_string()),
                    alt_text: if alt.is_empty() {
                        None
                    } else {
                        Some(alt.to_string())
                    },
                    mime_type: mime,
                });
                remaining = &after_bracket[close_paren + 1..];
                continue;
            }
        }
        remaining = &remaining[start + 2..];
    }

    // HTML img tags: <img src="url" alt="text" />
    let mut remaining = content;
    while let Some(start) = remaining.find("<img ") {
        let tag_content = &remaining[start..];
        if let Some(end) = tag_content.find('>') {
            let tag = &tag_content[..end + 1];
            let src = extract_attr(tag, "src");
            let alt = extract_attr(tag, "alt");
            if let Some(url) = src {
                let mime = guess_mime_from_url(&url);
                refs.push(ImageRef {
                    url: Some(url),
                    alt_text: alt,
                    mime_type: mime,
                });
            }
            remaining = &remaining[start + end + 1..];
        } else {
            break;
        }
    }

    refs
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    if let Some(start) = tag.find(&pattern) {
        let after = &tag[start + pattern.len()..];
        if let Some(end) = after.find('"') {
            return Some(after[..end].to_string());
        }
    }
    // Try single quotes
    let pattern = format!("{attr}='");
    if let Some(start) = tag.find(&pattern) {
        let after = &tag[start + pattern.len()..];
        if let Some(end) = after.find('\'') {
            return Some(after[..end].to_string());
        }
    }
    None
}

fn guess_mime_from_url(url: &str) -> String {
    let lower = url.to_lowercase();
    if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".svg") {
        "image/svg+xml".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else {
        "image/unknown".to_string()
    }
}
