//! Document-level operations detected BEFORE retrieval.
//!
//! A request like "สรุปเอกสารนี้ให้หน่อย" is not a content question — it is an
//! operation on a document. Chunk retrieval cannot serve it: the query shares
//! no vocabulary with any content chunk (measured top dense cosine ≈ 0.03 vs
//! ≈ 0.99 for a real content question on the same corpus), so the pipeline
//! retrieves noise and the relevance guard refuses with "rephrase your
//! question" — a dead end, because no rephrasing of a summary request will
//! ever retrieve well.
//!
//! This module recognizes that class *cheaply* (no LLM call) and resolves
//! which document the user means, so the pipeline can answer from the stored
//! document text instead of retrieval:
//!
//! * query names a document (facet/title match) → summarize that one;
//! * bare operation + exactly one document in scope → summarize it;
//! * bare operation + several documents → ask which one, listing titles;
//! * bare operation + empty scope → say there are no documents yet.
//!
//! Detection is deliberately conservative: it requires a summarize-family
//! marker AND (a named document OR a "bare" query — one that is nothing but
//! operation/referent/politeness tokens). "สรุปอัตราภาษีจากเอกสาร" carries real
//! content tokens, is not bare, names nothing → normal RAG handles it.

use thairag_core::types::{ChatMessage, DocId};

use crate::doc_selector::{CatalogEntry, select_docs};

/// What the pipeline should do for a recognized document operation.
pub enum DocOpOutcome {
    /// A complete textual answer (clarification / empty-scope notice) — no
    /// LLM call needed; stream it as a plain message.
    Answer(String),
    /// Summarize this document from its stored converted text.
    Summarize { doc_id: DocId, title: String },
}

/// Summarize-family markers. `summar` covers summarize/summarise/summary.
/// "เกี่ยวกับอะไร" is the Thai "what is (it) about" — an about-request folded
/// into summarize. Matched case-insensitively on the whitespace-collapsed query.
const OP_MARKERS: &[&str] = &[
    "สรุป",
    "สาระสำคัญ",
    "ใจความ",
    "เกี่ยวกับอะไร",
    "summar",
    "overview",
    "tl;dr",
    "tldr",
];

/// Tokens that a *bare* document operation is allowed to consist of: the
/// operation itself, document referents, demonstratives, politeness particles
/// and question fillers. Stripped longest-first; whatever alphanumeric residue
/// remains is content the user actually asked about.
const STRIP_PHRASES: &[&str] = &[
    // operation markers (longest first within family)
    "เกี่ยวกับอะไร",
    "สาระสำคัญ",
    "ใจความ",
    "สรุป",
    "summarize",
    "summarise",
    "summary",
    "overview",
    "tl;dr",
    "tldr",
    // document referents
    "เอกสาร",
    "ไฟล์",
    "ฉบับ",
    "document",
    "file",
    "docs",
    "doc",
    "pdf",
    // demonstratives / articles
    "ดังกล่าว",
    "นี้",
    "นี่",
    "นั้น",
    "this",
    "that",
    "these",
    "the",
    // politeness / filler (Thai)
    "ช่วย",
    "กรุณา",
    "ให้หน่อย",
    "ให้ที",
    "ให้ฟัง",
    "ให้ด้วย",
    "หน่อย",
    "ให้",
    "ด้วย",
    "ครับ",
    "ค่ะ",
    "คะ",
    "นะ",
    "จ้า",
    "สั้นๆ",
    "สั้น",
    "ย่อ",
    "พอสังเขป",
    "คร่าวๆ",
    "โดยรวม",
    "ทั้งหมด",
    "เนื้อหา",
    "แบบ",
    "ขอ",
    "อะไร",
    "เกี่ยวกับ",
    "คือ",
    // politeness / filler (English)
    "please",
    "kindly",
    "could you",
    "can you",
    "would you",
    "for me",
    "give me",
    "what",
    "is",
    "are",
    "it",
    "about",
    "contents",
    "content",
    "briefly",
    "short",
    "whole",
    "all",
    "of",
    "me",
    "a",
    "an",
];

fn normalize(q: &str) -> String {
    q.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the query carries a summarize-family marker at all. Cheap
/// pre-filter — without a marker the module never engages.
pub fn has_op_marker(query: &str) -> bool {
    let q = normalize(query);
    OP_MARKERS.iter().any(|m| q.contains(m))
}

/// True when the query is *nothing but* operation/referent/politeness tokens —
/// i.e. "summarize this document please" with no content terms. Thai has no
/// word spacing, so this strips known phrases as substrings (longest first)
/// and checks the alphanumeric residue.
pub fn is_bare_doc_op(query: &str) -> bool {
    let mut q = normalize(query);
    let mut phrases: Vec<&str> = STRIP_PHRASES.to_vec();
    phrases.sort_by_key(|p| std::cmp::Reverse(p.chars().count()));
    for p in &phrases {
        q = q.replace(p, " ");
    }
    q.chars().filter(|c| c.is_alphanumeric()).count() <= 3
}

/// Recognize a document operation and resolve its target against the scope's
/// document catalogue. `None` → not a document operation (or ambiguous in a
/// way ordinary retrieval serves better); the pipeline proceeds unchanged.
pub fn resolve(query: &str, catalog: &[CatalogEntry], max_catalog: usize) -> Option<DocOpOutcome> {
    if !has_op_marker(query) {
        return None;
    }
    let thai = crate::confidence::detect_lang(query) == crate::confidence::Lang::Th;

    // "สรุปเอกสาร ภ.ง.ด.53" — the query names one document: summarize it even
    // though the query is not bare.
    let named = select_docs(query, catalog, max_catalog);
    if named.len() == 1 {
        let e = catalog.iter().find(|e| e.doc_id == named[0])?;
        return Some(DocOpOutcome::Summarize {
            doc_id: e.doc_id,
            title: e.title.clone(),
        });
    }

    // Anything with real content tokens beyond the operation itself is left to
    // normal RAG ("สรุปอัตราภาษีจากเอกสาร" answers fine from chunks).
    if !is_bare_doc_op(query) {
        return None;
    }

    match catalog.len() {
        0 => Some(DocOpOutcome::Answer(no_documents_message(thai))),
        1 => Some(DocOpOutcome::Summarize {
            doc_id: catalog[0].doc_id,
            title: catalog[0].title.clone(),
        }),
        _ => Some(DocOpOutcome::Answer(clarify_message(
            thai,
            &catalog.iter().map(|e| e.title.clone()).collect::<Vec<_>>(),
        ))),
    }
}

/// Cap on titles listed in a clarification answer.
const MAX_LISTED_TITLES: usize = 10;

/// "Which document do you mean?" — lists what IS available, turning the old
/// dead-end refusal into a one-step next action.
pub fn clarify_message(thai: bool, titles: &[String]) -> String {
    // The same file ingested into several workspaces is several documents but
    // ONE choice to the user — dedupe the display list, order preserved.
    let mut seen = std::collections::HashSet::new();
    let titles: Vec<String> = titles
        .iter()
        .filter(|t| seen.insert(t.as_str().to_owned()))
        .cloned()
        .collect();
    let shown = &titles[..titles.len().min(MAX_LISTED_TITLES)];
    let list = shown
        .iter()
        .map(|t| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    let more = titles.len().saturating_sub(MAX_LISTED_TITLES);
    if thai {
        let tail = if more > 0 {
            format!("\n…และอีก {more} ฉบับ")
        } else {
            String::new()
        };
        format!(
            "คุณต้องการให้สรุปเอกสารฉบับไหน? เอกสารที่มีอยู่:\n{list}{tail}\n\n\
             พิมพ์ชื่อเอกสารที่ต้องการ เช่น \"สรุปเอกสาร {}\"",
            shown[0]
        )
    } else {
        let tail = if more > 0 {
            format!("\n…and {more} more")
        } else {
            String::new()
        };
        format!(
            "Which document would you like me to summarize? Available documents:\n{list}{tail}\n\n\
             Name the one you mean, e.g. \"summarize {}\"",
            shown[0]
        )
    }
}

/// Bare summarize request against an empty scope: say so directly instead of
/// the generic low-relevance refusal.
pub fn no_documents_message(thai: bool) -> String {
    if thai {
        "ยังไม่มีเอกสารในคลังความรู้ที่คุณเข้าถึงได้ \
         กรุณาอัปโหลดเอกสารก่อน แล้วลองขอสรุปอีกครั้ง"
            .to_string()
    } else {
        "There are no documents in your accessible knowledge base yet. \
         Please upload a document first, then ask for a summary again."
            .to_string()
    }
}

/// Build the answer-LLM messages for a document summary: the full stored text
/// (token-capped) as system context plus the user's original request. The
/// model answers in the user's language per the instruction.
pub fn build_summarize_messages(
    title: &str,
    content: &str,
    user_query: &str,
    max_context_tokens: usize,
) -> Vec<ChatMessage> {
    let (body, truncated) = truncate_to_tokens(content, max_context_tokens);
    let note = if truncated {
        "\n\n[NOTE: the document was truncated to fit the context window — \
         say so if the user asks about completeness.]"
    } else {
        ""
    };
    let system = format!(
        "You are a document assistant. The full converted text of the document \
         \"{title}\" is provided below. Fulfill the user's request (e.g. a summary) \
         using ONLY this document — do not add outside knowledge. Respond in the \
         same language as the user's request. Keep key figures, obligations and \
         dates exact.{note}\n\n--- DOCUMENT: {title} ---\n{body}\n--- END DOCUMENT ---"
    );
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
            images: vec![],
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_query.to_string(),
            images: vec![],
        },
    ]
}

/// Head-truncate `content` to approximately `max_tokens` using the same
/// Thai-aware estimator as context curation. Returns `(text, was_truncated)`.
fn truncate_to_tokens(content: &str, max_tokens: usize) -> (String, bool) {
    if crate::context_curator::estimate_tokens(content) <= max_tokens {
        return (content.to_string(), false);
    }
    // Proportional char cut, then trim to a char boundary via chars().
    let total_chars = content.chars().count();
    let est = crate::context_curator::estimate_tokens(content);
    let keep = (total_chars * max_tokens / est.max(1)).max(1);
    let cut: String = content.chars().take(keep).collect();
    (cut, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::DocId;

    fn entry(title: &str, facets: &[&str]) -> CatalogEntry {
        CatalogEntry {
            doc_id: DocId::new(),
            title: title.to_string(),
            facets: facets.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn bare_thai_summarize_requests_are_detected() {
        for q in [
            "สรุปเอกสารนี้ให้หน่อย",
            "ช่วยสรุปเอกสารให้หน่อยครับ",
            "สรุปเอกสาร",
            "สรุปให้หน่อย",
            "เอกสารนี้เกี่ยวกับอะไร",
            "สรุปสาระสำคัญของเอกสารนี้",
            "ขอสรุปเนื้อหาทั้งหมดแบบสั้นๆ",
        ] {
            assert!(has_op_marker(q), "marker: {q}");
            assert!(is_bare_doc_op(q), "bare: {q}");
        }
    }

    #[test]
    fn bare_english_summarize_requests_are_detected() {
        for q in [
            "summarize this document please",
            "Summarize this file",
            "give me a summary",
            "tl;dr",
            "can you give me an overview of this document",
        ] {
            assert!(has_op_marker(q), "marker: {q}");
            assert!(is_bare_doc_op(q), "bare: {q}");
        }
    }

    #[test]
    fn content_questions_are_not_bare() {
        for q in [
            "สรุปอัตราภาษีจากเอกสาร",    // summarize THE TAX RATES → content
            "สรุปเงื่อนไขการกู้เงินให้หน่อย", // loan conditions → content
            "summarize the withholding tax rates",
            "อัตราภาษีร้อยละของ ภ.ง.ด.53 คือเท่าใด", // plain content question
        ] {
            assert!(!is_bare_doc_op(q), "should not be bare: {q}");
        }
    }

    #[test]
    fn no_marker_short_circuits() {
        assert!(resolve("อัตราภาษีเท่าไหร่", &[entry("a", &[])], 30).is_none());
    }

    #[test]
    fn single_doc_scope_resolves_to_that_doc() {
        let cat = [entry("รายงานประจำปี 2568", &[])];
        match resolve("สรุปเอกสารนี้ให้หน่อย", &cat, 30) {
            Some(DocOpOutcome::Summarize { doc_id, title }) => {
                assert_eq!(doc_id, cat[0].doc_id);
                assert_eq!(title, "รายงานประจำปี 2568");
            }
            _ => panic!("expected Summarize"),
        }
    }

    #[test]
    fn multi_doc_scope_asks_which_one_listing_titles() {
        let cat = [entry("Doc A", &[]), entry("Doc B", &[])];
        match resolve("สรุปเอกสารนี้ให้หน่อย", &cat, 30) {
            Some(DocOpOutcome::Answer(msg)) => {
                assert!(msg.contains("Doc A") && msg.contains("Doc B"), "{msg}");
                assert!(
                    msg.contains("ฉบับไหน"),
                    "clarify in Thai for Thai query: {msg}"
                );
            }
            _ => panic!("expected clarify Answer"),
        }
    }

    #[test]
    fn empty_scope_says_no_documents() {
        match resolve("summarize this document", &[], 30) {
            Some(DocOpOutcome::Answer(msg)) => {
                assert!(msg.contains("no documents"), "{msg}");
            }
            _ => panic!("expected no-documents Answer"),
        }
    }

    #[test]
    fn named_document_is_summarized_even_when_not_bare() {
        // Facet value tokens make "กล้าสู้" distinguishing for doc 2.
        let cat = [
            entry("สินเชื่อ SME โตไว", &["program: โตไว"]),
            entry("สินเชื่อ SME กล้าสู้", &["program: กล้าสู้"]),
        ];
        match resolve("สรุปเอกสารสินเชื่อ SME กล้าสู้ ให้หน่อย", &cat, 30)
        {
            Some(DocOpOutcome::Summarize { doc_id, .. }) => assert_eq!(doc_id, cat[1].doc_id),
            _ => panic!("expected named-doc Summarize"),
        }
    }

    #[test]
    fn clarify_caps_listed_titles() {
        let titles: Vec<String> = (0..25).map(|i| format!("Doc {i}")).collect();
        let msg = clarify_message(false, &titles);
        assert!(msg.contains("Doc 9") && !msg.contains("Doc 10\n"), "{msg}");
        assert!(msg.contains("15 more"), "{msg}");
    }

    #[test]
    fn clarify_dedupes_same_title_across_workspaces() {
        let titles: Vec<String> = ["gazette.pdf", "gazette.pdf", "survey.pdf", "gazette.pdf"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let msg = clarify_message(false, &titles);
        // Once as a list bullet (the example line at the end also names it).
        assert_eq!(msg.matches("- gazette.pdf").count(), 1, "{msg}");
        assert!(msg.contains("survey.pdf"));
    }

    #[test]
    fn truncation_marks_and_bounds() {
        let long = "ก".repeat(100_000);
        let msgs = build_summarize_messages("t", &long, "สรุป", 1000);
        assert!(msgs[0].content.contains("truncated"));
        assert!(msgs[0].content.chars().count() < 10_000);
    }
}
