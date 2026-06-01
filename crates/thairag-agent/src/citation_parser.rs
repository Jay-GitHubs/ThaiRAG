//! Deterministic structured-citation parsing.
//!
//! The response LLM is already instructed to cite context chunks with inline
//! `[N]` markers (see `response_generator.rs`). This module parses those
//! markers — no extra LLM call — and maps each one back to the curated chunk
//! at 1-based index `N`, producing per-claim [`Citation`] records.
//!
//! The parser is liberal in what it accepts: `[1]`, `[1][2]`, `[1, 2]` and
//! `[1-3]` ranges all work. Markers that reference a chunk index outside the
//! curated context are dropped (a malformed answer yields no citations
//! rather than an error).

use thairag_core::types::Citation;

use crate::context_curator::CuratedContext;

/// Parse `[N]` citation markers in `answer` against the curated context.
///
/// Each marker becomes one [`Citation`] tying the enclosing sentence/claim
/// to the chunk at 1-based index `N`.
pub fn parse_citations(answer: &str, context: &CuratedContext) -> Vec<Citation> {
    if answer.is_empty() || context.chunks.is_empty() {
        return Vec::new();
    }

    let markers = scan_markers(answer);
    if markers.is_empty() {
        return Vec::new();
    }
    let claims = claim_spans(answer);

    let mut citations = Vec::new();
    let mut seen: Vec<(usize, u32)> = Vec::new(); // (claim_idx, marker) dedupe
    for marker in &markers {
        let claim_idx = claims
            .iter()
            .position(|&(s, e)| marker.offset >= s && marker.offset < e)
            .unwrap_or(claims.len().saturating_sub(1));
        let (cs, ce) = claims.get(claim_idx).copied().unwrap_or((0, answer.len()));
        let claim_text = clean_claim(&answer[cs..ce]);

        for &n in &marker.numbers {
            if seen.contains(&(claim_idx, n)) {
                continue;
            }
            // 1-based marker → 0-based index into the curated chunks.
            let Some(idx0) = (n as usize).checked_sub(1) else {
                continue;
            };
            let Some(chunk) = context.chunks.get(idx0) else {
                continue; // marker out of range — drop it
            };
            seen.push((claim_idx, n));
            citations.push(Citation {
                claim: claim_text.clone(),
                marker: n,
                chunk_id: chunk.source_chunk_id.to_string(),
                doc_id: chunk.source_doc_id.to_string(),
                doc_title: chunk.source_doc_title.clone(),
                score: chunk.relevance_score,
            });
        }
    }
    citations
}

/// A parsed `[...]` marker group: its byte offset in the answer and the
/// (range-expanded) chunk numbers it references.
struct Marker {
    offset: usize,
    numbers: Vec<u32>,
}

/// Find every `[...]` group whose interior is only digits, spaces, commas
/// and hyphens, and expand it to a list of referenced chunk numbers.
fn scan_markers(answer: &str) -> Vec<Marker> {
    let bytes = answer.as_bytes();
    let mut markers = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(close) = answer[i + 1..].find(']')
        {
            let interior = &answer[i + 1..i + 1 + close];
            if !interior.is_empty()
                && interior
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ' ' | ',' | '-'))
                && interior.chars().any(|c| c.is_ascii_digit())
            {
                let numbers = expand_numbers(interior);
                if !numbers.is_empty() {
                    markers.push(Marker { offset: i, numbers });
                }
            }
            i += close + 2;
            continue;
        }
        i += 1;
    }
    markers
}

/// Expand a marker interior like `1, 2-4` into `[1, 2, 3, 4]`.
fn expand_numbers(interior: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for token in interior.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some((a, b)) = token.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                for n in a.min(b)..=a.max(b) {
                    out.push(n);
                }
            }
        } else if let Ok(n) = token.parse::<u32>() {
            out.push(n);
        }
    }
    out
}

/// Partition `answer` into contiguous claim spans (byte ranges).
///
/// Splits on `.!?\n`, then absorbs any trailing whitespace and `[...]`
/// markers into the claim they follow — so a marker written right after a
/// full stop is attributed to the sentence it ends, not the next one.
fn claim_spans(answer: &str) -> Vec<(usize, usize)> {
    let bytes = answer.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if matches!(c, b'.' | b'!' | b'?' | b'\n') {
            let mut end = i + 1;
            // Absorb trailing whitespace + bracket marker groups.
            loop {
                while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                    end += 1;
                }
                if end < bytes.len()
                    && bytes[end] == b'['
                    && let Some(close) = answer[end..].find(']')
                {
                    end += close + 1;
                    continue;
                }
                break;
            }
            spans.push((start, end));
            start = end;
            i = end;
            continue;
        }
        i += 1;
    }
    if start < answer.len() {
        spans.push((start, answer.len()));
    }
    if spans.is_empty() {
        spans.push((0, answer.len()));
    }
    spans
}

/// Trim a claim span and strip its inline `[...]` markers for display.
fn clean_claim(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c == '['
            && let Some(close) = raw[idx + 1..].find(']')
        {
            let interior = &raw[idx + 1..idx + 1 + close];
            if !interior.is_empty()
                && interior
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ' ' | ',' | '-'))
            {
                // Skip the whole marker.
                while let Some(&(j, _)) = chars.peek() {
                    if j <= idx + 1 + close {
                        chars.next();
                    } else {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{ChunkId, DocId};

    use crate::context_curator::CuratedChunk;

    fn ctx(n: usize) -> CuratedContext {
        let chunks = (1..=n)
            .map(|i| CuratedChunk {
                index: i,
                content: format!("chunk {i}"),
                relevance_score: 0.9 - (i as f32) * 0.1,
                source_doc_id: DocId::new(),
                source_chunk_id: ChunkId::new(),
                source_doc_title: Some(format!("Doc {i}")),
                image_blob_id: None,
                images: Vec::new(),
            })
            .collect();
        CuratedContext {
            chunks,
            total_tokens_est: 0,
        }
    }

    #[test]
    fn parses_single_marker() {
        let c = parse_citations("The sky is blue [1].", &ctx(2));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].marker, 1);
        assert_eq!(c[0].doc_title.as_deref(), Some("Doc 1"));
        assert!(c[0].claim.contains("sky is blue"));
        assert!(!c[0].claim.contains('['), "markers stripped from claim");
    }

    #[test]
    fn parses_multiple_markers_one_claim() {
        let c = parse_citations("X happened [1][2].", &ctx(3));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].marker, 1);
        assert_eq!(c[1].marker, 2);
    }

    #[test]
    fn parses_comma_list() {
        let c = parse_citations("Combined evidence [1, 3].", &ctx(3));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].marker, 1);
        assert_eq!(c[1].marker, 3);
    }

    #[test]
    fn parses_range_markers() {
        let c = parse_citations("Broad support [1-3].", &ctx(4));
        let markers: Vec<u32> = c.iter().map(|x| x.marker).collect();
        assert_eq!(markers, vec![1, 2, 3]);
    }

    #[test]
    fn out_of_range_marker_skipped() {
        let c = parse_citations("Claim [9].", &ctx(2));
        assert!(c.is_empty());
    }

    #[test]
    fn no_markers_returns_empty() {
        assert!(parse_citations("Just prose, no citations.", &ctx(2)).is_empty());
    }

    #[test]
    fn empty_context_returns_empty() {
        assert!(parse_citations("Claim [1].", &ctx(0)).is_empty());
    }

    #[test]
    fn english_multi_sentence_buckets_correctly() {
        let c = parse_citations("First fact [1]. Second fact [2].", &ctx(2));
        assert_eq!(c.len(), 2);
        assert!(c[0].claim.contains("First fact"));
        assert_eq!(c[0].marker, 1);
        assert!(c[1].claim.contains("Second fact"));
        assert_eq!(c[1].marker, 2);
    }

    #[test]
    fn marker_after_terminator_attaches_to_preceding_claim() {
        // Marker written right after the full stop.
        let c = parse_citations("The earth is round.[1] It orbits the sun.[2]", &ctx(2));
        assert_eq!(c.len(), 2);
        assert!(c[0].claim.contains("earth is round"));
        assert_eq!(c[0].marker, 1);
        assert!(c[1].claim.contains("orbits the sun"));
        assert_eq!(c[1].marker, 2);
    }

    #[test]
    fn thai_text_claim_captured() {
        let c = parse_citations("ภาษีเงินได้คือภาษีที่เก็บจากรายได้ [1]", &ctx(2));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].marker, 1);
        assert!(c[0].claim.contains("ภาษี"));
    }

    #[test]
    fn duplicate_marker_in_same_claim_deduped() {
        let c = parse_citations("Repeated [1] claim [1].", &ctx(2));
        assert_eq!(c.len(), 1);
    }
}
