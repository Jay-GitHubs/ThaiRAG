//! Deterministic document selection ("hybrid vectorless" retrieval).
//!
//! Vector + BM25 similarity cannot disambiguate corpora of near-identical
//! documents (14 variants of one loan program whose bodies differ in a few
//! numbers): the right document is retrieved but drowned by its siblings.
//! Measured on a real 14-clone Thai corpus, every *judgement*-based selector —
//! vector aggregation, BM25, IDF-on-titles, and an LLM picking from the
//! catalogue — landed at 4–40 % correct, because telling near-identical things
//! apart is exactly what LLMs and embeddings are unreliable at.
//!
//! What works (93 % correct in the same test, lifting end-to-end accuracy from
//! 0.44 to 0.79) is *exact matching*, not judgement: extract each document's
//! distinguishing facets once at ingest ("program: กล้าสู้", "collateral:
//! เงินฝาก", "limit: 10 ล้านบาท"), then at query time score each document by how
//! many of its facet *values* appear verbatim in the query, weighting rare
//! tokens higher. "เงินฝาก" is either in the query or it is not — no variance.
//! Self-gating: a query that names no facet value scores nothing and retrieval
//! stays unscoped, so it generalises to any corpus without per-corpus tuning.

use std::collections::HashMap;

use thairag_core::types::DocId;
use tracing::debug;

/// One catalogue entry: a document plus the facets extracted at ingest.
#[derive(Clone)]
pub struct CatalogEntry {
    pub doc_id: DocId,
    pub title: String,
    /// "key: value" facet strings from `ProcessingProvenance.facets`.
    pub facets: Vec<String>,
}

/// Minimum rare-token-weighted score for a document to be considered named by
/// the query. Below this the query names no document → no scoping.
const MIN_SCORE: f32 = 0.30;
/// Only documents within this (tiny) margin of the best score are scoped —
/// i.e. genuine ties on the distinguishing tokens. A wider net re-admits a
/// sibling that merely shares the same token, which dilutes the context back
/// to the unscoped failure mode (measured: a 0.75 fraction admitted a sibling
/// and erased the gain). Near-equal because scores are sums of 1/df rationals.
const TIE_MARGIN: f32 = 1e-4;

/// Alphanumeric (incl. Thai) tokens of length ≥ `min_len`, lowercased.
/// Thai combining marks (above/below vowels, tone marks — Unicode Mn, NOT
/// alphanumeric) must stay inside tokens: splitting on them fragments "ที่จ่าย"
/// into junk like "ที" that substring-matches almost any Thai query and
/// produces false near-ties between documents.
fn tokens(s: &str, min_len: usize) -> Vec<String> {
    fn in_token(c: char) -> bool {
        c.is_alphanumeric() || ('\u{0E31}'..='\u{0E4E}').contains(&c)
    }
    s.split(|c: char| !in_token(c))
        .filter(|w| w.chars().count() >= min_len)
        .map(|w| w.to_lowercase())
        .collect()
}

/// The distinguishing value tokens of a catalogue entry: tokens from each
/// facet's *value* (the part after `key:`) plus the title's longer tokens.
fn value_tokens(e: &CatalogEntry) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for f in &e.facets {
        let val = f.split_once(':').map(|(_, v)| v).unwrap_or(f);
        out.extend(tokens(val, 2));
    }
    out.extend(tokens(&e.title, 3));
    out
}

/// Pick the document(s) a query is about by exact facet-value matching.
/// Returns the chosen `DocId`s — empty when the query names no document (then
/// retrieval stays unscoped). No-op for a catalogue smaller than 2 (nothing to
/// disambiguate) or larger than `max_catalog` (a flat scan stops being cheap;
/// such workspaces are better served by ordinary search).
pub fn select_docs(query: &str, catalog: &[CatalogEntry], max_catalog: usize) -> Vec<DocId> {
    if catalog.len() < 2 || catalog.len() > max_catalog {
        return vec![];
    }
    // Per-document value-token sets, and document frequency of each token so a
    // token shared by every sibling (boilerplate) counts for almost nothing
    // while a token unique to one document dominates.
    let doc_tokens: Vec<(DocId, std::collections::HashSet<String>)> = catalog
        .iter()
        .map(|e| (e.doc_id, value_tokens(e)))
        .collect();
    let mut df: HashMap<&String, usize> = HashMap::new();
    for (_, toks) in &doc_tokens {
        for t in toks {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let n = doc_tokens.len();
    let q = query.to_lowercase();
    let scored: Vec<(DocId, f32)> = doc_tokens
        .iter()
        .map(|(id, toks)| {
            let s: f32 = toks
                .iter()
                // A token in EVERY document is pure boilerplate — it names no
                // particular document, so it contributes nothing. The rest are
                // weighted by rarity: a token unique to one document dominates.
                .filter(|t| df[t] < n && q.contains(t.as_str()))
                .map(|t| 1.0 / df[t] as f32)
                .sum();
            (*id, s)
        })
        .collect();
    let best = scored.iter().map(|(_, s)| *s).fold(0.0_f32, f32::max);
    if best < MIN_SCORE {
        return vec![];
    }
    let picked: Vec<DocId> = scored
        .iter()
        .filter(|(_, s)| *s >= best - TIE_MARGIN)
        .map(|(id, _)| *id)
        .collect();
    debug!(
        picked = picked.len(),
        catalog = catalog.len(),
        best,
        "Deterministic doc selector decided"
    );
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thai combining marks must not fragment tokens: "ที่จ่าย" splitting into
    /// "ที" made doc B substring-match ANY query containing "ที่…" and tie the
    /// scores — the selector then picked both docs and resolution failed.
    #[test]
    fn thai_combining_marks_do_not_fragment_tokens() {
        let cat = vec![
            entry("a.pdf", &["เรื่อง: สำรวจภาวะการทำงานของประชากร"]),
            entry("b.pdf", &["เรื่อง: ภาษีหัก ณ ที่จ่าย"]),
        ];
        let picked = select_docs(
            "ปีที่เผยแพร่ของเอกสาร สำรวจภาวะการทำงานของประชากร คือ พ.ศ. ใด",
            &cat,
            30,
        );
        assert_eq!(
            picked,
            vec![cat[0].doc_id],
            "must pick exactly the named doc"
        );
    }

    fn entry(title: &str, facets: &[&str]) -> CatalogEntry {
        CatalogEntry {
            doc_id: DocId::new(),
            title: title.into(),
            facets: facets.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn matches_distinctive_facet_value() {
        // Two near-identical loan docs differing only in collateral + limit.
        let cat = vec![
            entry(
                "SME กล้าสู้ loan",
                &["program: กล้าสู้", "collateral: เต็มจำนวน", "limit: 20"],
            ),
            entry(
                "SME กล้าสู้ loan",
                &["program: กล้าสู้", "collateral: เงินฝาก", "limit: 10"],
            ),
        ];
        let want = cat[1].doc_id;
        // "กล้าสู้" is shared (df=2 → low weight); "เงินฝาก" is unique (df=1).
        let got = select_docs("โครงการ กล้าสู้ หลักประกัน เงินฝาก วงเงินเท่าไร", &cat, 60);
        assert_eq!(got, vec![want]);
    }

    #[test]
    fn unnamed_query_does_not_scope() {
        let cat = vec![
            entry("Alpha", &["topic: alpha", "kind: widget"]),
            entry("Beta", &["topic: beta", "kind: gadget"]),
        ];
        // Query names no facet value.
        assert!(select_docs("please summarise everything", &cat, 60).is_empty());
    }

    #[test]
    fn self_gates_on_catalog_size() {
        let one = vec![entry("Solo", &["topic: solo"])];
        assert!(select_docs("about solo", &one, 60).is_empty());
        let many: Vec<CatalogEntry> = (0..80)
            .map(|i| entry(&format!("doc {i}"), &[&format!("n: {i}")]))
            .collect();
        assert!(select_docs("about doc 5", &many, 60).is_empty());
    }

    #[test]
    fn shared_boilerplate_does_not_trigger() {
        // Both docs share all tokens → df high → score below threshold, no scope.
        let cat = vec![
            entry("loan program document", &["kind: loan program"]),
            entry("loan program document", &["kind: loan program"]),
        ];
        assert!(select_docs("loan program", &cat, 60).is_empty());
    }
}
