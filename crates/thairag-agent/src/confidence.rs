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
//!    *non*-answer: it is unscored (a "No answer" state).
//! 2. **Citation coverage** — the share of the answer's claims that carry a
//!    resolvable `[N]` marker. The dominant grounding signal. Claims are
//!    counted the way a reader would, not per raw punctuation mark: decimals
//!    (`3.5`), Thai abbreviations (`พ.ศ.`), and list numbering (`1.`) don't
//!    split a claim; markdown headings and rules aren't claims; a whole table
//!    counts once (cited via the adjacent sentence that introduces it); and a
//!    citation closing a paragraph or list item covers that whole block.
//! 3. **Corroboration** — a fully cited answer reaches 10 on its own; citing
//!    ≥2 distinct documents is a +1 nudge for partially covered answers, not a
//!    gate. (Near-clone corpora are deliberately single-document scopes, so a
//!    correct answer often *should* cite exactly one document.)
//! 4. **Retrieval** — whether any context was retrieved at all (a hard cap when
//!    nothing was).
//!
//! Each contributing signal is returned as a [`ConfidenceFactor`] so the UI can
//! show the breakdown behind the number. No LLM call, no added latency.
//!
//! All user-facing strings (summary, factor labels/details, and the trailing
//! "what this score means" explainer) are localized to the answer's language —
//! Thai answers get Thai explanations. Detection is script-based on the answer
//! text itself, so it needs no query-analysis plumbing and matches what the
//! user is actually reading.

use thairag_core::types::ConfidenceFactor;

use crate::citation_parser::{clean_claim, is_refusal, parse_citations, scan_markers};
use crate::context_curator::CuratedContext;

/// The deterministic confidence verdict: a 1–10 score, a one-line rationale,
/// and the per-factor breakdown behind it. `score` is `None` for a refusal —
/// a non-answer isn't scored on the 1–10 scale; the UI shows a neutral "No
/// answer" marker instead (matching the no-context gate's refusal state).
#[derive(Debug, Clone, PartialEq)]
pub struct ConfidenceAssessment {
    pub score: Option<u8>,
    pub summary: String,
    pub factors: Vec<ConfidenceFactor>,
}

/// Language for the user-facing strings, detected from the answer itself.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Lang {
    En,
    Th,
}

/// Thai wins when the answer has more Thai letters than Latin ones — answers
/// over Thai corpora routinely mix scripts (numbers, product names, units).
pub(crate) fn detect_lang(answer: &str) -> Lang {
    let thai = answer
        .chars()
        .filter(|c| ('\u{0E00}'..='\u{0E7F}').contains(c))
        .count();
    let latin = answer.chars().filter(char::is_ascii_alphabetic).count();
    if thai > latin { Lang::Th } else { Lang::En }
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    Text,
    Table,
}

/// Partition the answer into claim blocks: a paragraph, a list item, or a
/// contiguous markdown table. Markdown headings, horizontal rules, and blank
/// lines are scaffolding, not claims — they close the current block and are
/// excluded. Byte ranges of adjacent blocks touch only when no blank/scaffold
/// line separates them (used for table↔intro citation adjacency).
fn claim_blocks(answer: &str) -> Vec<(usize, usize, BlockKind)> {
    let mut blocks: Vec<(usize, usize, BlockKind)> = Vec::new();
    let mut cur: Option<(usize, usize, BlockKind)> = None;
    let mut pos = 0;
    for line in answer.split_inclusive('\n') {
        let start = pos;
        let end = pos + line.len();
        pos = end;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if let Some(b) = cur.take() {
                blocks.push(b);
            }
            continue;
        }
        // A table line (including its |---| separator row) extends the table.
        if trimmed.starts_with('|') {
            match &mut cur {
                Some((_, e, BlockKind::Table)) => *e = end,
                _ => {
                    if let Some(b) = cur.take() {
                        blocks.push(b);
                    }
                    cur = Some((start, end, BlockKind::Table));
                }
            }
            continue;
        }
        // Headings and horizontal rules are layout, not claims.
        if trimmed.starts_with('#')
            || trimmed
                .chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '=' | ' '))
        {
            if let Some(b) = cur.take() {
                blocks.push(b);
            }
            continue;
        }
        // Each list item is its own block; plain lines extend the paragraph.
        if is_list_item(trimmed) {
            if let Some(b) = cur.take() {
                blocks.push(b);
            }
            cur = Some((start, end, BlockKind::Text));
            continue;
        }
        match &mut cur {
            Some((_, e, BlockKind::Text)) => *e = end,
            _ => {
                if let Some(b) = cur.take() {
                    blocks.push(b);
                }
                cur = Some((start, end, BlockKind::Text));
            }
        }
    }
    if let Some(b) = cur.take() {
        blocks.push(b);
    }
    blocks
}

/// A bulleted (`- `, `* `, `• `) or numbered (`1. `, `1) `) list item line.
fn is_list_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
        return true;
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") "))
}

/// A span is a claim only if it has real content once markers are stripped.
fn substantive(text: &str) -> bool {
    clean_claim(text)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .count()
        >= 3
}

/// Count (cited, total) claim units. A unit is one sentence of a paragraph /
/// list item, or one whole table. A unit is cited when a marker resolving to a
/// real context chunk falls inside it; a citation on a block's final sentence
/// covers the whole block (the common one-citation-per-paragraph style), and a
/// table with no marker of its own inherits one from the touching intro or
/// caption block.
fn count_claim_units(answer: &str, valid_offsets: &[usize]) -> (usize, usize) {
    let blocks = claim_blocks(answer);
    let cited_in = |s: usize, e: usize| valid_offsets.iter().any(|&o| o >= s && o < e);

    let mut total = 0;
    let mut cited = 0;
    for (bi, &(bs, be, kind)) in blocks.iter().enumerate() {
        match kind {
            BlockKind::Table => {
                if !substantive(&answer[bs..be]) {
                    continue;
                }
                total += 1;
                let neighbor_cited = || {
                    let prev = bi
                        .checked_sub(1)
                        .and_then(|i| blocks.get(i))
                        .is_some_and(|&(ps, pe, _)| pe == bs && cited_in(ps, pe));
                    let next = blocks
                        .get(bi + 1)
                        .is_some_and(|&(ns, ne, _)| ns == be && cited_in(ns, ne));
                    prev || next
                };
                if cited_in(bs, be) || neighbor_cited() {
                    cited += 1;
                }
            }
            BlockKind::Text => {
                let flags: Vec<bool> = crate::citation_parser::claim_spans(&answer[bs..be])
                    .into_iter()
                    .map(|(s, e)| (bs + s, bs + e))
                    .filter(|&(s, e)| substantive(&answer[s..e]))
                    .map(|(s, e)| cited_in(s, e))
                    .collect();
                if flags.is_empty() {
                    continue;
                }
                total += flags.len();
                cited += if flags.last() == Some(&true) {
                    flags.len() // trailing citation grounds the whole block
                } else {
                    flags.iter().filter(|&&f| f).count()
                };
            }
        }
    }
    (cited, total)
}

/// Compute deterministic answer confidence. Returns `None` only when there is
/// nothing to judge (empty answer), so the caller leaves the score unset rather
/// than reporting a misleading number.
pub fn assess(answer: &str, context: &CuratedContext) -> Option<ConfidenceAssessment> {
    if answer.trim().is_empty() {
        return None;
    }

    let lang = detect_lang(answer);

    // 1. Refusal: a non-answer isn't scored — surface a "No answer" state (no
    //    number), consistent with how the no-context gate marks a refusal.
    if is_refusal(answer) {
        let (summary, label, detail) = match lang {
            Lang::Th => (
                "คำตอบระบุว่าไม่พบข้อมูลในแหล่งที่มา",
                "ไม่มีคำตอบ",
                "ปฏิเสธการตอบ / ไม่พบข้อมูลที่เกี่ยวข้องในแหล่งที่มาที่ค้นคืนได้",
            ),
            Lang::En => (
                "The answer reports the information wasn't found in the sources",
                "No answer",
                "Declined / no relevant information in the retrieved sources",
            ),
        };
        return Some(ConfidenceAssessment {
            score: None,
            summary: summary.to_string(),
            factors: vec![ConfidenceFactor {
                label: label.to_string(),
                detail: detail.to_string(),
            }],
        });
    }

    let mut factors = Vec::new();

    // 2. Citation coverage — the share of claim units grounded by a marker
    //    that resolves to a real context chunk.
    let valid_offsets: Vec<usize> = scan_markers(answer)
        .into_iter()
        .filter(|m| {
            m.numbers
                .iter()
                .any(|&n| (1..=context.chunks.len()).contains(&(n as usize)))
        })
        .map(|m| m.offset)
        .collect();
    let (cited_claims, claims) = count_claim_units(answer, &valid_offsets);
    let claims = claims.max(1);
    let coverage = cited_claims as f32 / claims as f32;

    // 3. Corroboration — distinct source documents the answer draws on.
    let citations = parse_citations(answer, context);
    let distinct_sources = {
        let mut docs: Vec<&str> = citations.iter().map(|c| c.doc_id.as_str()).collect();
        docs.sort_unstable();
        docs.dedup();
        docs.len()
    };

    // Map signals → 1–10. An uncited-but-substantive answer lands mid-scale
    // (it may be correct, just unverifiable); a fully cited answer reaches 10
    // even from a single document — multi-doc corroboration is a nudge for
    // partial coverage, not a requirement for the top score.
    let mut score = 4.0 + coverage * 6.0;
    factors.push(ConfidenceFactor {
        label: match lang {
            Lang::Th => "การอ้างอิงแหล่งที่มา",
            Lang::En => "Citation coverage",
        }
        .to_string(),
        detail: match lang {
            Lang::Th => format!("{cited_claims} จาก {claims} ข้อความอ้างอิงแหล่งที่มา"),
            Lang::En => format!("{cited_claims} of {claims} claims cite a source"),
        },
    });

    if distinct_sources >= 2 {
        score += 1.0;
    }
    if !citations.is_empty() {
        factors.push(ConfidenceFactor {
            label: match lang {
                Lang::Th => "เอกสารอ้างอิง",
                Lang::En => "Corroboration",
            }
            .to_string(),
            detail: match lang {
                Lang::Th => format!("อ้างอิงเอกสาร {distinct_sources} ฉบับ"),
                Lang::En => format!(
                    "{distinct_sources} distinct {} cited",
                    if distinct_sources == 1 {
                        "document"
                    } else {
                        "documents"
                    }
                ),
            },
        });
    }

    // 4. Retrieval — nothing retrieved means nothing to ground against; cap low.
    let retrieval_label = match lang {
        Lang::Th => "การค้นคืนข้อมูล",
        Lang::En => "Retrieval",
    };
    if context.chunks.is_empty() {
        score = score.min(2.0);
        factors.push(ConfidenceFactor {
            label: retrieval_label.to_string(),
            detail: match lang {
                Lang::Th => "ไม่พบเนื้อหาสนับสนุนจากการค้นคืน".to_string(),
                Lang::En => "No supporting context was retrieved".to_string(),
            },
        });
    } else {
        factors.push(ConfidenceFactor {
            label: retrieval_label.to_string(),
            detail: match lang {
                Lang::Th => format!("มีเนื้อหาสนับสนุน {} ส่วนในบริบท", context.chunks.len()),
                Lang::En => format!("{} supporting chunks in context", context.chunks.len()),
            },
        });
    }

    // Trailing explainer so the number itself is understandable: what the
    // scale measures (source grounding) and what it does NOT (correctness).
    factors.push(ConfidenceFactor {
        label: match lang {
            Lang::Th => "ความหมายของคะแนน",
            Lang::En => "About this score",
        }
        .to_string(),
        detail: match lang {
            Lang::Th => "คะแนน 1–10 วัดว่าคำตอบอ้างอิงแหล่งที่มาครบถ้วนเพียงใด (10 = ทุกข้อความมีแหล่งอ้างอิง, \
                         4 = ไม่มีการอ้างอิง, 1–2 = ไม่พบข้อมูลสนับสนุน) ไม่ได้วัดความถูกต้องของเนื้อหา"
                .to_string(),
            Lang::En => "The 1–10 score measures how completely the answer cites its sources \
                         (10 = every statement cited, 4 = no citations, 1–2 = nothing \
                         retrieved) — not whether the content is factually correct"
                .to_string(),
        },
    });

    let score = score.round().clamp(1.0, 10.0) as u8;
    let summary = confidence_summary(lang, score, cited_claims, claims, distinct_sources, context);

    Some(ConfidenceAssessment {
        score: Some(score),
        summary,
        factors,
    })
}

/// Build the one-line rationale shown next to the score.
fn confidence_summary(
    lang: Lang,
    score: u8,
    cited_claims: usize,
    claims: usize,
    distinct_sources: usize,
    context: &CuratedContext,
) -> String {
    if context.chunks.is_empty() {
        return match lang {
            Lang::Th => "ไม่พบเนื้อหาสนับสนุนสำหรับคำตอบนี้",
            Lang::En => "No supporting context was retrieved for this answer",
        }
        .to_string();
    }
    if cited_claims == 0 {
        return match lang {
            Lang::Th => "คำตอบไม่มีการอ้างอิงแหล่งที่มา จึงไม่สามารถตรวจสอบข้อความได้",
            Lang::En => "The answer cites no sources, so its claims couldn't be verified",
        }
        .to_string();
    }
    let band = match (lang, score) {
        (Lang::Th, 8..) => "อ้างอิงแหล่งที่มาครบถ้วน",
        (Lang::Th, 5..) => "อ้างอิงแหล่งที่มาบางส่วน",
        (Lang::Th, _) => "อ้างอิงแหล่งที่มาน้อย",
        (Lang::En, 8..) => "Well grounded",
        (Lang::En, 5..) => "Partly grounded",
        (Lang::En, _) => "Weakly grounded",
    };
    match lang {
        Lang::Th => format!(
            "{band}: {cited_claims} จาก {claims} ข้อความอ้างอิงแหล่งที่มา จากเอกสาร {distinct_sources} ฉบับ"
        ),
        Lang::En => {
            let src = if distinct_sources == 1 {
                "1 document".to_string()
            } else {
                format!("{distinct_sources} documents")
            };
            format!("{band}: {cited_claims} of {claims} claims cite a source across {src}")
        }
    }
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
    fn refusal_is_unscored_no_answer() {
        let a = assess(
            "There is no information about that in the context.",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        // A refusal is a "No answer" state — no 1–10 number.
        assert_eq!(a.score, None);
        assert_eq!(a.factors[0].label, "No answer");
    }

    #[test]
    fn fully_cited_multi_source_scores_top() {
        let d1 = DocId::new();
        let d2 = DocId::new();
        // Two claims, both cited, from two distinct documents.
        let a = assess(
            "North Q1 was 100 [1]. South Q2 was 200 [2].",
            &ctx(&[d1, d2]),
        )
        .unwrap();
        assert_eq!(a.score, Some(10));
        assert!(a.summary.contains("2 documents"));
    }

    #[test]
    fn fully_cited_single_source_scores_top() {
        // A correct, fully cited answer from ONE document must reach 10 —
        // single-doc scopes are the deliberate deployment shape, not a defect.
        let a = assess("วงเงินกู้สูงสุด 100 ล้านบาท [1]", &ctx(&[DocId::new()])).unwrap();
        assert_eq!(a.score, Some(10));
    }

    #[test]
    fn thai_decimal_does_not_shred_coverage() {
        // Previously "3.5" split into two claims (one uncited) → score 6.
        let a = assess(
            "อัตราดอกเบี้ยคงที่ 3.5% ต่อปี ระยะเวลา 7 ปี [1]",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, Some(10));
    }

    #[test]
    fn thai_abbreviations_do_not_shred_coverage() {
        let a = assess(
            "โครงการเริ่มปี พ.ศ. 2560 ดำเนินการโดย ธ.ก.ส. [1]",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, Some(10));
    }

    #[test]
    fn cited_list_items_score_top() {
        let d = DocId::new();
        let a = assess(
            "เงื่อนไขโครงการ:\n1. วงเงินกู้สูงสุด 100 ล้านบาท [1]\n2. อัตราดอกเบี้ย 3.5% ต่อปี [1]\n- ระยะเวลา 7 ปี [1]",
            &ctx(&[d]),
        )
        .unwrap();
        // The `:`-intro line is part of the answer's framing; all fact-bearing
        // items are cited, and the uncited intro is the block whose trailing
        // list absorbs... intro is its own paragraph claim here, so 3 of 4.
        assert!(a.score.unwrap() >= 8, "got {:?}", a.score);
    }

    #[test]
    fn trailing_citation_covers_paragraph() {
        // One citation closing a multi-sentence paragraph grounds all of it.
        let a = assess(
            "The program started in 2017. It offers loans up to 100M. The rate is fixed [1].",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, Some(10));
    }

    #[test]
    fn markdown_scaffolding_is_not_a_claim() {
        let a = assess(
            "## สรุปเงื่อนไข\n\nวงเงินกู้สูงสุด 100 ล้านบาท [1]\n\n---",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, Some(10));
    }

    #[test]
    fn table_counts_once_and_inherits_intro_citation() {
        let a = assess(
            "เงื่อนไขตามตารางนี้ [1]\n| รายการ | ค่า |\n|---|---|\n| วงเงิน | 100 ล้าน |\n| ดอกเบี้ย | 3.5% |",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, Some(10));
    }

    #[test]
    fn uncited_answer_lands_midscale() {
        let a = assess(
            "North Q1 was 100. South Q2 was 200.",
            &ctx(&[DocId::new(), DocId::new()]),
        )
        .unwrap();
        assert_eq!(a.score, Some(4));
        assert!(a.summary.contains("cites no sources"));
    }

    #[test]
    fn no_context_caps_low() {
        let a = assess("North Q1 was 100.", &CuratedContext::default()).unwrap();
        assert!(a.score.unwrap() <= 2);
    }

    #[test]
    fn partial_coverage_between() {
        let d1 = DocId::new();
        // Two claims, only the first cited (no trailing citation → no
        // paragraph-wide propagation).
        let a = assess("North Q1 was 100 [1]. South Q2 was unknown.", &ctx(&[d1])).unwrap();
        let s = a.score.unwrap();
        assert!((5..=7).contains(&s), "got {s}");
    }

    #[test]
    fn unresolvable_marker_is_not_coverage() {
        // [9] points outside the context — it must not count as grounding.
        let a = assess("North Q1 was 100 [9].", &ctx(&[DocId::new()])).unwrap();
        assert_eq!(a.score, Some(4));
    }

    #[test]
    fn empty_answer_is_none() {
        assert!(assess("   ", &ctx(&[DocId::new()])).is_none());
    }

    #[test]
    fn thai_answer_gets_thai_summary_factors_and_explainer() {
        let a = assess("วงเงินกู้สูงสุด 100 ล้านบาท [1]", &ctx(&[DocId::new()])).unwrap();
        assert!(a.summary.contains("อ้างอิงแหล่งที่มา"), "summary: {}", a.summary);
        assert!(a.factors.iter().any(|f| f.label == "การอ้างอิงแหล่งที่มา"));
        assert!(a.factors.iter().any(|f| f.label == "การค้นคืนข้อมูล"));
        // The scale explainer rides along so the number is self-explanatory.
        let about = a
            .factors
            .iter()
            .find(|f| f.label == "ความหมายของคะแนน")
            .expect("scale explainer present");
        assert!(about.detail.contains("10 ="));
    }

    #[test]
    fn english_answer_stays_english_with_explainer() {
        let a = assess("Max loan is 100M [1].", &ctx(&[DocId::new()])).unwrap();
        assert!(
            a.summary.starts_with("Well grounded"),
            "summary: {}",
            a.summary
        );
        assert!(a.factors.iter().any(|f| f.label == "Citation coverage"));
        assert!(a.factors.iter().any(|f| f.label == "About this score"));
    }

    #[test]
    fn thai_refusal_is_localized() {
        let a = assess("ไม่พบข้อมูลเกี่ยวกับเรื่องนี้ในเอกสาร", &ctx(&[DocId::new()])).unwrap();
        assert_eq!(a.score, None);
        assert_eq!(a.factors[0].label, "ไม่มีคำตอบ");
        assert!(a.summary.contains("ไม่พบข้อมูล"));
    }

    #[test]
    fn mixed_answer_mostly_thai_counts_as_thai() {
        // Thai sentence with Latin product name + digits — Thai letters dominate.
        let a = assess(
            "แบตเตอรี่ของ Nimbus Z9 มีความจุ 5,000 mAh [1]",
            &ctx(&[DocId::new()]),
        )
        .unwrap();
        assert!(a.factors.iter().any(|f| f.label == "การอ้างอิงแหล่งที่มา"));
    }
}
