use std::sync::Arc;

use async_trait::async_trait;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{Chunker, LlmProvider, SmartChunker};
use thairag_core::types::{ChatMessage, ConvertedDocument, EnrichedChunk};
use tracing::warn;

use super::analyzer::strip_json_fences;
use super::prompts;
use crate::chunker::MarkdownChunker;

/// LLM-powered semantic chunker.
/// Identifies logical section boundaries and assigns topic metadata per chunk.
pub struct LlmSmartChunker {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    /// Fallback mechanical chunker for when LLM fails or chunks are too large.
    fallback_chunker: MarkdownChunker,
    prompts: Arc<PromptRegistry>,
}

impl LlmSmartChunker {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            fallback_chunker: MarkdownChunker::new(),
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            fallback_chunker: MarkdownChunker::new(),
            prompts,
        }
    }

    /// Re-chunk with feedback about previous chunking issues.
    pub async fn chunk_with_feedback(
        &self,
        converted: &ConvertedDocument,
        max_chunk_size: usize,
        issues: &[String],
    ) -> Result<Vec<EnrichedChunk>> {
        let markdown = &converted.markdown;
        let lines: Vec<&str> = markdown.lines().collect();

        if lines.is_empty() {
            return Ok(vec![]);
        }

        let numbered: String = lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}: {}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = prompts::smart_chunker_feedback_prompt(&self.prompts, &numbered, issues);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let json_str = strip_json_fences(response.content.trim());

        let sections: Vec<SectionBoundary> = match serde_json::from_str(json_str) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to parse chunker feedback response, falling back");
                return Ok(mechanical_fallback(
                    markdown,
                    max_chunk_size,
                    &self.fallback_chunker,
                ));
            }
        };

        if sections.is_empty() {
            return Ok(mechanical_fallback(
                markdown,
                max_chunk_size,
                &self.fallback_chunker,
            ));
        }

        let page_map = build_page_map(&lines);
        let mut enriched = Vec::new();

        for section in &sections {
            let start = section.start_line.saturating_sub(1).min(lines.len());
            let end = section.end_line.min(lines.len());
            if start >= end {
                continue;
            }

            let section_pages = pages_for_range(&page_map, start, end);

            let content: String = lines[start..end]
                .iter()
                .filter(|line| !is_page_marker(line))
                .copied()
                .collect::<Vec<_>>()
                .join("\n");

            if content.len() > max_chunk_size {
                let sub_chunks = self.fallback_chunker.chunk(&content, max_chunk_size, 0);
                for sub in sub_chunks {
                    enriched.push(EnrichedChunk {
                        content: sub,
                        topic: section.topic.clone(),
                        section_title: section.section_title.clone(),
                        language: Some(converted.analysis.primary_language.clone()),
                        chunk_type: section.chunk_type.clone(),
                        page_numbers: section_pages.clone(),
                    });
                }
            } else if !content.trim().is_empty() {
                enriched.push(EnrichedChunk {
                    content,
                    topic: section.topic.clone(),
                    section_title: section.section_title.clone(),
                    language: Some(converted.analysis.primary_language.clone()),
                    chunk_type: section.chunk_type.clone(),
                    page_numbers: section_pages,
                });
            }
        }

        if enriched.is_empty() {
            return Ok(mechanical_fallback(
                markdown,
                max_chunk_size,
                &self.fallback_chunker,
            ));
        }

        Ok(enriched)
    }
}

/// Validate chunks and return issues for retry. Empty vec = all good.
pub fn validate_chunks(
    chunks: &[EnrichedChunk],
    original_markdown: &str,
    max_chunk_size: usize,
) -> Vec<String> {
    let mut issues = Vec::new();

    // Check for empty chunks
    let empty_count = chunks
        .iter()
        .filter(|c| c.content.trim().is_empty())
        .count();
    if empty_count > 0 {
        issues.push(format!("{empty_count} chunk(s) are empty"));
    }

    // Check coverage
    let total_chunk_len: usize = chunks.iter().map(|c| c.content.len()).sum();
    // Strip page markers from original for fair comparison
    let original_content_len: usize = original_markdown
        .lines()
        .filter(|l| !is_page_marker(l))
        .map(|l| l.len() + 1)
        .sum();
    if original_content_len > 0 {
        let coverage = total_chunk_len as f64 / original_content_len as f64;
        if coverage < 0.8 {
            issues.push(format!(
                "Only {:.0}% of content covered ({} of {} chars)",
                coverage * 100.0,
                total_chunk_len,
                original_content_len,
            ));
        }
    }

    // Check oversized chunks
    let oversized: Vec<usize> = chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.content.len() > max_chunk_size)
        .map(|(i, _)| i + 1)
        .collect();
    if !oversized.is_empty() {
        issues.push(format!(
            "Chunk(s) {} exceed max size of {} chars",
            oversized
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            max_chunk_size,
        ));
    }

    issues
}

#[async_trait]
impl SmartChunker for LlmSmartChunker {
    async fn chunk(
        &self,
        converted: &ConvertedDocument,
        max_chunk_size: usize,
    ) -> Result<Vec<EnrichedChunk>> {
        let markdown = &converted.markdown;
        let lines: Vec<&str> = markdown.lines().collect();

        if lines.is_empty() {
            return Ok(vec![]);
        }

        // Number lines for the LLM
        let numbered: String = lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}: {}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = prompts::smart_chunker_prompt(&self.prompts, &numbered);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let json_str = strip_json_fences(response.content.trim());

        let sections: Vec<SectionBoundary> = match serde_json::from_str(json_str) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to parse chunker response, falling back to mechanical");
                return Ok(mechanical_fallback(
                    markdown,
                    max_chunk_size,
                    &self.fallback_chunker,
                ));
            }
        };

        if sections.is_empty() {
            return Ok(mechanical_fallback(
                markdown,
                max_chunk_size,
                &self.fallback_chunker,
            ));
        }

        // Build a line_number → page_number map from page markers
        let page_map = build_page_map(&lines);

        let mut enriched = Vec::new();

        for section in &sections {
            let start = section.start_line.saturating_sub(1).min(lines.len());
            let end = section.end_line.min(lines.len());
            if start >= end {
                continue;
            }

            // Collect page numbers for this section's line range
            let section_pages = pages_for_range(&page_map, start, end);

            // Filter out page marker lines from content
            let content: String = lines[start..end]
                .iter()
                .filter(|line| !is_page_marker(line))
                .copied()
                .collect::<Vec<_>>()
                .join("\n");

            // If section is too large, sub-split mechanically
            if content.len() > max_chunk_size {
                let sub_chunks = self.fallback_chunker.chunk(&content, max_chunk_size, 0);
                for sub in sub_chunks {
                    enriched.push(EnrichedChunk {
                        content: sub,
                        topic: section.topic.clone(),
                        section_title: section.section_title.clone(),
                        language: Some(converted.analysis.primary_language.clone()),
                        chunk_type: section.chunk_type.clone(),
                        page_numbers: section_pages.clone(),
                    });
                }
            } else if !content.trim().is_empty() {
                enriched.push(EnrichedChunk {
                    content,
                    topic: section.topic.clone(),
                    section_title: section.section_title.clone(),
                    language: Some(converted.analysis.primary_language.clone()),
                    chunk_type: section.chunk_type.clone(),
                    page_numbers: section_pages,
                });
            }
        }

        if enriched.is_empty() {
            return Ok(mechanical_fallback(
                markdown,
                max_chunk_size,
                &self.fallback_chunker,
            ));
        }

        Ok(enriched)
    }
}

#[derive(serde::Deserialize)]
struct SectionBoundary {
    start_line: usize,
    end_line: usize,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    section_title: Option<String>,
    #[serde(default)]
    chunk_type: Option<String>,
}

fn mechanical_fallback(
    text: &str,
    max_chunk_size: usize,
    chunker: &MarkdownChunker,
) -> Vec<EnrichedChunk> {
    chunker
        .chunk(text, max_chunk_size, 0)
        .into_iter()
        .map(|content| EnrichedChunk {
            content,
            topic: None,
            section_title: None,
            language: None,
            chunk_type: None,
            page_numbers: None,
        })
        .collect()
}

/// Page marker format used by the AI pipeline: `<!-- page:N -->`
const PAGE_MARKER_PREFIX: &str = "<!-- page:";

fn is_page_marker(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with(PAGE_MARKER_PREFIX) && trimmed.ends_with("-->")
}

/// Parse page markers in lines to build a map: line_index → page_number.
/// Lines before the first marker have no page info.
fn build_page_map(lines: &[&str]) -> Vec<Option<usize>> {
    let mut page_map = vec![None; lines.len()];
    let mut current_page: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        if is_page_marker(line) {
            let trimmed = line.trim();
            if let Some(num_str) = trimmed
                .strip_prefix(PAGE_MARKER_PREFIX)
                .and_then(|s| s.strip_suffix("-->"))
            {
                if let Ok(page) = num_str.trim().parse::<usize>() {
                    current_page = Some(page);
                }
            }
        }
        page_map[i] = current_page;
    }

    page_map
}

/// Collect unique sorted page numbers for a line range.
fn pages_for_range(page_map: &[Option<usize>], start: usize, end: usize) -> Option<Vec<usize>> {
    let mut pages: Vec<usize> = page_map[start..end].iter().filter_map(|p| *p).collect();
    pages.sort_unstable();
    pages.dedup();
    if pages.is_empty() { None } else { Some(pages) }
}
