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

    /// Name of the model backing this agent.
    pub fn model_name(&self) -> &str {
        self.llm.model_name()
    }

    /// Re-chunk with feedback about previous chunking issues.
    pub async fn chunk_with_feedback(
        &self,
        converted: &ConvertedDocument,
        max_chunk_size: usize,
        issues: &[String],
    ) -> Result<Vec<EnrichedChunk>> {
        Ok(self
            .chunk_windowed(converted, max_chunk_size, Some(issues))
            .await)
    }

    /// Shared chunking core. The line-numbered markdown is split into windows
    /// that each stay well under the upstream request-body limit: a large doc
    /// otherwise makes one oversized LLM request that gateways reject with 502,
    /// forcing a mechanical fallback for the *whole* document. Line numbers are
    /// GLOBAL across windows, so the boundaries the LLM returns slice the full
    /// line list directly. A window whose LLM call fails or returns nothing
    /// degrades to mechanical chunking for that window's lines only — content
    /// is never dropped, and one transient flap doesn't discard the rest of the
    /// document's enrichment.
    async fn chunk_windowed(
        &self,
        converted: &ConvertedDocument,
        max_chunk_size: usize,
        feedback: Option<&[String]>,
    ) -> Vec<EnrichedChunk> {
        let markdown = &converted.markdown;
        let lines: Vec<&str> = markdown.lines().collect();
        if lines.is_empty() {
            return vec![];
        }

        let page_map = build_page_map(&lines);
        let windows = window_line_ranges(&lines, MAX_CHUNKER_INPUT_CHARS);
        let mut enriched = Vec::new();

        for win in &windows {
            match self.sections_for_window(&lines, win, feedback).await {
                Some(sections) if !sections.is_empty() => {
                    for section in &sections {
                        push_section_chunks(
                            &mut enriched,
                            &lines,
                            &page_map,
                            section,
                            converted,
                            max_chunk_size,
                            &self.fallback_chunker,
                        );
                    }
                }
                _ => push_mechanical_window(
                    &mut enriched,
                    &lines,
                    &page_map,
                    win,
                    max_chunk_size,
                    &self.fallback_chunker,
                ),
            }
        }

        if enriched.is_empty() {
            return mechanical_fallback(markdown, max_chunk_size, &self.fallback_chunker);
        }
        enriched
    }

    /// Ask the LLM for section boundaries over one window's globally-numbered
    /// lines. `None` = the call or JSON parse failed; the caller then
    /// mechanically chunks the window so its content is preserved.
    async fn sections_for_window(
        &self,
        lines: &[&str],
        win: &LineWindow,
        feedback: Option<&[String]>,
    ) -> Option<Vec<SectionBoundary>> {
        let numbered: String = (win.start..win.end)
            .map(|i| format!("{}: {}", i + 1, lines[i]))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = match feedback {
            Some(issues) => {
                prompts::smart_chunker_feedback_prompt(&self.prompts, &numbered, issues)
            }
            None => prompts::smart_chunker_prompt(&self.prompts, &numbered),
        };
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
            images: vec![],
        }];

        let response = match self.llm.generate(&messages, Some(self.max_tokens)).await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, win_start = win.start, win_end = win.end,
                    "Chunker LLM call failed for window; mechanical fallback for this window");
                return None;
            }
        };
        let json_str = strip_json_fences(response.content.trim());
        match serde_json::from_str::<Vec<SectionBoundary>>(json_str) {
            Ok(sections) => Some(sections),
            Err(e) => {
                warn!(error = %e, "Failed to parse chunker window response; mechanical fallback for this window");
                None
            }
        }
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
        Ok(self.chunk_windowed(converted, max_chunk_size, None).await)
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

/// Max characters of line-numbered markdown sent to the chunker LLM per call.
/// Kept well under typical upstream request-body limits — large gateways return
/// 502 on oversized bodies — so the document is windowed to this budget. Line
/// numbers stay global across windows, so returned boundaries index the full
/// line list directly.
const MAX_CHUNKER_INPUT_CHARS: usize = 40_000;

/// A half-open `[start, end)` range of global line indices.
struct LineWindow {
    start: usize,
    end: usize,
}

/// Split the line list into windows whose numbered text stays under `budget`
/// characters. Always returns at least one window for a non-empty input; a
/// single line longer than `budget` becomes its own (oversized) window.
fn window_line_ranges(lines: &[&str], budget: usize) -> Vec<LineWindow> {
    let mut windows = Vec::new();
    let mut start = 0usize;
    let mut size = 0usize;
    for (i, line) in lines.iter().enumerate() {
        // Approximate the rendered "<n>: <line>\n" cost; the number width is
        // negligible next to the line content.
        let entry = line.len() + 8;
        if i > start && size + entry > budget {
            windows.push(LineWindow { start, end: i });
            start = i;
            size = 0;
        }
        size += entry;
    }
    windows.push(LineWindow {
        start,
        end: lines.len(),
    });
    windows
}

/// Append enriched chunk(s) for one LLM-identified section, sub-splitting
/// mechanically when the section exceeds `max_chunk_size`.
fn push_section_chunks(
    out: &mut Vec<EnrichedChunk>,
    lines: &[&str],
    page_map: &[Option<usize>],
    section: &SectionBoundary,
    converted: &ConvertedDocument,
    max_chunk_size: usize,
    fallback: &MarkdownChunker,
) {
    let start = section.start_line.saturating_sub(1).min(lines.len());
    let end = section.end_line.min(lines.len());
    if start >= end {
        return;
    }
    let section_pages = pages_for_range(page_map, start, end);
    let content: String = lines[start..end]
        .iter()
        .filter(|line| !is_page_marker(line))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");

    if content.len() > max_chunk_size {
        for sub in fallback.chunk(&content, max_chunk_size, 0) {
            out.push(EnrichedChunk {
                content: sub,
                topic: section.topic.clone(),
                section_title: section.section_title.clone(),
                language: Some(converted.analysis.primary_language.clone()),
                chunk_type: section.chunk_type.clone(),
                page_numbers: section_pages.clone(),
            });
        }
    } else if !content.trim().is_empty() {
        out.push(EnrichedChunk {
            content,
            topic: section.topic.clone(),
            section_title: section.section_title.clone(),
            language: Some(converted.analysis.primary_language.clone()),
            chunk_type: section.chunk_type.clone(),
            page_numbers: section_pages,
        });
    }
}

/// Mechanically chunk one window's lines (used when its LLM call/parse fails),
/// with no enrichment but preserving page numbers.
fn push_mechanical_window(
    out: &mut Vec<EnrichedChunk>,
    lines: &[&str],
    page_map: &[Option<usize>],
    win: &LineWindow,
    max_chunk_size: usize,
    fallback: &MarkdownChunker,
) {
    let content: String = lines[win.start..win.end]
        .iter()
        .filter(|line| !is_page_marker(line))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if content.trim().is_empty() {
        return;
    }
    let pages = pages_for_range(page_map, win.start, win.end);
    for sub in fallback.chunk(&content, max_chunk_size, 0) {
        out.push(EnrichedChunk {
            content: sub,
            topic: None,
            section_title: None,
            language: None,
            chunk_type: None,
            page_numbers: pages.clone(),
        });
    }
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
                && let Ok(page) = num_str.trim().parse::<usize>()
            {
                current_page = Some(page);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_input_is_a_single_window() {
        let lines = vec!["alpha", "beta", "gamma"];
        let w = window_line_ranges(&lines, 40_000);
        assert_eq!(w.len(), 1);
        assert_eq!((w[0].start, w[0].end), (0, 3));
    }

    #[test]
    fn large_input_partitions_contiguously_under_budget() {
        // ~50 chars/line * 5000 lines ≈ 250KB → several windows at a 40KB budget.
        let owned: Vec<String> = (0..5000)
            .map(|i| format!("line {i} {}", "x".repeat(40)))
            .collect();
        let lines: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        let budget = 40_000;
        let windows = window_line_ranges(&lines, budget);

        assert!(windows.len() > 1, "expected the doc to be split");
        // Contiguous partition: no gaps, no overlaps, full coverage.
        assert_eq!(windows.first().unwrap().start, 0);
        assert_eq!(windows.last().unwrap().end, lines.len());
        for pair in windows.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "windows must be contiguous");
        }
        // Each window (except a forced single oversized line) stays near budget.
        for win in &windows {
            let size: usize = (win.start..win.end).map(|i| lines[i].len() + 8).sum();
            assert!(
                size <= budget || win.end - win.start == 1,
                "window {}..{} size {size} exceeds budget {budget}",
                win.start,
                win.end
            );
        }
    }

    #[test]
    fn line_longer_than_budget_is_its_own_window() {
        let big = "z".repeat(100_000);
        let lines = vec!["short", big.as_str(), "short2"];
        let windows = window_line_ranges(&lines, 40_000);
        // Coverage preserved despite the oversized middle line.
        assert_eq!(windows.first().unwrap().start, 0);
        assert_eq!(windows.last().unwrap().end, 3);
    }
}
