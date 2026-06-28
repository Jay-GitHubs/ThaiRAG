//! Deterministic, explainable answer confidence (1–10).
//!
//! Earlier this asked the response LLM to grade its own answer — opaque (a bare
//! number with no rationale) and poorly calibrated (a model grading itself is
//! optimistic and varies run-to-run). This replaces that with a deterministic
//! score derived from signals the pipeline already produced, so the same answer
//! always scores the same and every point is explainable.
//!
//! The score blends scale-free grounding signals (it deliberately avoids raw
//! vector/rerank scores, whose magnitude is config-dependent and would make the
//! number "not make sense"):
//!
//! 1. **Refusal** — an answer that reports it found no information is a confident
//!    *non*-answer: it scores the floor (1).
//! 2. **Citation coverage** — the share of the answer's claims that carry a
//!    resolvable `[N]` marker. The dominant grounding signal.
//! 3. **Corroboration** — how many distinct source documents the answer cites.
//! 4. **Retrieval** — whether any context was retrieved at all (a hard cap when
//!    nothing was).
//!
//! Each contributing signal is returned as a [`ConfidenceFactor`] so the UI can
//! show the breakdown behind the number. No LLM call, no added latency.

use thairag_core::types::ConfidenceFactor;

use crate::citation_parser::{is_refusal, parse_citations};
use crate::context_curator::CuratedContext;

/// The deterministic confidence verdict: a 1–10 score, a one-line rationale,
/// and the per-factor breakdown behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfidenceAssessment {
    pub score: u8,
    pub summary: String,
    pub factors: Vec<ConfidenceFactor>,
}

/// Count the answer's distinct claims — sentences with real content. Used as the
/// denominator for citation coverage. Mirrors the response generator's view of a
/// claim (sentence-ish), but we only need a count, so a light split suffices.
fn claim_count(answer: &str) -> usize {
    let n = answer
        .split(['.', '!', '?', '\n'])
        .filter(|s| s.trim().chars().filter(|c| c.is_alphanumeric()).count() >= 3)
        .count();
    n.max(1)
}

/// Compute deterministic answer confidence. Returns `None` only when there is
/// nothing to judge (empty answer), so the caller leaves the score unset rather
/// than reporting a misleading number.
pub fn assess(answer: &str, context: &CuratedContext) -> Option<ConfidenceAssessment> {
    if answer.trim().is_empty() {
        return None;
    }

    // 1. Refusal: a correct "I couldn't find that" is a confident non-answer.
    if is_refusal(answer) {
        return Some(ConfidenceAssessment {
            score: 1,
            summary: "The answer reports the information wasn't found in the sources".to_string(),
            factors: vec![ConfidenceFactor {
                label: "No answer".to_string(),
                detail: "Declined / no relevant information in the retrieved sources".to_string(),
            }],
        });
    }

    let mut factors = Vec::new();

    // 2. Citation coverage — the share of claims grounded by an [N] marker.
    let citations = parse_citations(answer, context);
    let claims = claim_count(answer);
    let cited_claims = {
        let mut texts: Vec<&str> = citations.iter().map(|c| c.claim.as_str()).collect();
        texts.sort_unstable();
        texts.dedup();
        texts.len().min(claims)
    };
    let coverage = if claims == 0 {
        0.0
    } else {
        cited_claims as f32 / claims as f32
    };

    // 3. Corroboration — distinct source documents the answer draws on.
    let distinct_sources = {
        let mut docs: Vec<&str> = citations.iter().map(|c| c.doc_id.as_str()).collect();
        docs.sort_unstable();
        docs.dedup();
        docs.len()
    };

    // Map signals → 1–10. An uncited-but-substantive answer lands mid-scale
    // (it may be correct, just unverifiable); full coverage reaches the top.
    let mut score = 3.0 + coverage * 6.0;
    factors.push(ConfidenceFactor {
        label: "Citation coverage".to_string(),
        detail: format!("{cited_claims} of {claims} claims cite a source"),
    });

    if distinct_sources >= 2 {
        score += 1.0;
    }
    if !citations.is_empty() {
        factors.push(ConfidenceFactor {
            label: "Corroboration".to_string(),
            detail: format!(
                "{distinct_sources} distinct {} cited",
                if distinct_sources == 1 {
                    "document"
                } else {
                    "documents"
                }
            ),
        });
    }

    // 4. Retrieval — nothing retrieved means nothing to ground against; cap low.
    if context.chunks.is_empty() {
        score = score.min(2.0);
        factors.push(ConfidenceFactor {
            label: "Retrieval".to_string(),
            detail: "No supporting context was retrieved".to_string(),
        });
    } else {
        factors.push(ConfidenceFactor {
            label: "Retrieval".to_string(),
            detail: format!("{} supporting chunks in context", context.chunks.len()),
        });
    }

    let score = score.round().clamp(1.0, 10.0) as u8;
    let summary = confidence_summary(score, cited_claims, claims, distinct_sources, context);

    Some(ConfidenceAssessment {
        score,
        summary,
        factors,
    })
}

/// Build the one-line rationale shown next to the score.
fn confidence_summary(
    score: u8,
    cited_claims: usize,
    claims: usize,
    distinct_sources: usize,
    context: &CuratedContext,
) -> String {
    if context.chunks.is_empty() {
        return "No supporting context was retrieved for this answer".to_string();
    }
    if cited_claims == 0 {
        return "The answer cites no sources, so its claims couldn't be verified".to_string();
    }
    let band = if score >= 7 {
        "Well grounded"
    } else if score >= 4 {
        "Partly grounded"
    } else {
        "Weakly grounded"
    };
    let src = if distinct_sources == 1 {
        "1 document".to_string()
    } else {
        format!("{distinct_sources} documents")
    };
    format!("{band}: {cited_claims} of {claims} claims cite a source across {src}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{ChunkId, DocId};

    use crate::context_curator::{CuratedChunk, CuratedContext};

    /// Build a context whose chunks share `docs[i]` doc-ids (so we can control
    /// distinct-source corroboration).
    fn ctx(docs: &[DocId]) -> CuratedContext {
        let chunks = docs
            .iter()
            .enumerate()
            .map(|(i, d)| CuratedChunk {
                index: i + 1,
                content: format!("chunk {}", i + 1),
                relevance_score: 0.9,
                vector_score: None,
                source_doc_id: *d,
                source_chunk_id: ChunkId::new(),
                source_doc_title: Some(format!("Doc {}", i + 1)),
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
    fn refusal_scores_floor() {
        let a = assess(
            "There is no information about that in the context.",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, 1);
        assert_eq!(a.factors[0].label, "No answer");
    }

    #[test]
    fn fully_cited_multi_source_scores_high() {
        let d1 = DocId::new();
        let d2 = DocId::new();
        // Two claims, both cited, from two distinct documents.
        let a = assess(
            "North Q1 was 100 [1]. South Q2 was 200 [2].",
            &ctx(&[d1, d2]),
        )
        .unwrap();
        assert!(a.score >= 9, "expected high, got {}", a.score);
        assert!(a.summary.contains("2 documents"));
    }

    #[test]
    fn uncited_answer_lands_midscale() {
        let a = assess(
            "North Q1 was 100. South Q2 was 200.",
            &ctx(&[DocId::new(), DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, 3);
        assert!(a.summary.contains("cites no sources"));
    }

    #[test]
    fn no_context_caps_low() {
        let a = assess("North Q1 was 100.", &CuratedContext::default()).unwrap();
        assert!(a.score <= 2);
    }

    #[test]
    fn partial_coverage_between() {
        let d1 = DocId::new();
        // Two claims, only one cited.
        let a = assess("North Q1 was 100 [1]. South Q2 was unknown.", &ctx(&[d1])).unwrap();
        assert!(a.score >= 4 && a.score <= 7, "got {}", a.score);
    }

    #[test]
    fn empty_answer_is_none() {
        assert!(assess("   ", &ctx(&[DocId::new()])).is_none());
    }
}
