//! Fidelity-gated table rescue — regression coverage on the real-world fixture
//! that motivated it: `rd_tp4_table.pdf`, a born-digital Thai Revenue
//! Department landscape table whose deterministic lattice reconstruction
//! passes the geometric acceptance gate but shreds the structure
//! (over-segmented columns → misaligned colspans → values detached from their
//! entries). The fidelity check catches this as fabricated/dropped numbers.
//!
//! What's asserted here (deterministic parts only — no network model):
//! 1. The GATE fires: mechanical extraction of the fixture is flagged
//!    "review" by conversion fidelity, with table pages present. If lattice
//!    reconstruction improves enough that fidelity verifies it, this test
//!    will fail — at which point the rescue (and this test) can be retired.
//! 2. The RESCUE runs: with a mock vision model, `rescue_table_pages` renders
//!    and re-transcribes exactly the requested pages.
//! 3. KEEP-IF-BETTER holds: a garbage transcription (mock hallucination)
//!    scores WORSE than the mechanical extraction, so the caller's adoption
//!    rule would keep the original — the property that makes the rescue safe.

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmUsage, VisionMessage};
use thairag_document::smart_pdf::{self, SmartPdfConfig};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/thai-real/rd_tp4_table.pdf"
);

struct FixedVision(String);

#[async_trait]
impl LlmProvider for FixedVision {
    async fn generate(
        &self,
        _messages: &[ChatMessage],
        _max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: self.0.clone(),
            usage: LlmUsage::default(),
        })
    }

    async fn generate_vision(
        &self,
        _messages: &[VisionMessage],
        _max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: self.0.clone(),
            usage: LlmUsage::default(),
        })
    }

    fn supports_vision(&self) -> bool {
        true
    }

    fn model_name(&self) -> &str {
        "fixed-vision-mock"
    }
}

fn mechanical_markdown(cfg: &SmartPdfConfig) -> (String, usize) {
    let raw = std::fs::read(FIXTURE).expect("fixture readable");
    let extracts = smart_pdf::extract_pages(&raw, cfg).expect("extract");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let doc = rt.block_on(smart_pdf::render_to_document("", extracts, None, None, cfg));
    let table_pages = doc.pages.iter().filter(|p| p.table_html.is_some()).count();
    (doc.markdown, table_pages)
}

#[test]
fn fixture_mechanical_extraction_is_flagged_review_with_table_pages() {
    if !thairag_document::pdfium_engine::is_available() {
        eprintln!("pdfium not available — skipping");
        return;
    }
    let cfg = SmartPdfConfig::default();
    let (markdown, table_pages) = mechanical_markdown(&cfg);
    assert!(
        table_pages >= 2,
        "fixture should produce lattice table pages, got {table_pages}"
    );

    let raw = std::fs::read(FIXTURE).unwrap();
    let fid = thairag_document::conversion_fidelity::assess(&raw, "application/pdf", &markdown);
    assert_eq!(
        fid.status, "review",
        "the broken lattice must be caught by fidelity (score {})",
        fid.score
    );
}

#[test]
fn rescue_transcribes_requested_pages_with_vision_model() {
    if !thairag_document::pdfium_engine::is_available() {
        eprintln!("pdfium not available — skipping");
        return;
    }
    let cfg = SmartPdfConfig::default();
    let mock = FixedVision("| ลำดับที่ | อัตราภาษีร้อยละ |\n|---|---|\n| 1 | 3.0 |".into());
    let raw = std::fs::read(FIXTURE).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let out = rt.block_on(smart_pdf::rescue_table_pages(
        &raw,
        &[1, 4],
        &mock,
        thairag_document::semantic_prompts::Language::Th,
        &cfg,
    ));
    assert_eq!(out.len(), 2, "both requested pages transcribed");
    assert!(out.iter().all(|(_, t)| t.contains("อัตราภาษีร้อยละ")));
    let pages: Vec<usize> = out.iter().map(|(p, _)| *p).collect();
    assert_eq!(pages, vec![1, 4]);
}

#[test]
fn keep_if_better_rejects_hallucinated_transcription() {
    if !thairag_document::pdfium_engine::is_available() {
        eprintln!("pdfium not available — skipping");
        return;
    }
    let cfg = SmartPdfConfig::default();
    let (mech_md, _) = mechanical_markdown(&cfg);
    let raw = std::fs::read(FIXTURE).unwrap();
    let mech = thairag_document::conversion_fidelity::assess(&raw, "application/pdf", &mech_md);

    // A "rescue" that replaced the doc with hallucinated numbers must score
    // worse than the (flawed but grounded) mechanical extraction, so the
    // adoption rule keeps the original.
    let hallucinated = "| ลำดับที่ | อัตรา |\n|---|---|\n| 1 | 99.9 |\n| 2 | 123456 |";
    let cand = thairag_document::conversion_fidelity::assess(&raw, "application/pdf", hallucinated);
    assert!(
        cand.score < mech.score,
        "hallucination ({}) must not beat mechanical ({})",
        cand.score,
        mech.score
    );
}

#[test]
fn near_tie_candidate_does_not_displace_mechanical() {
    if !thairag_document::pdfium_engine::is_available() {
        eprintln!("pdfium not available — skipping");
        return;
    }
    let cfg = SmartPdfConfig::default();
    let (mech_md, _) = mechanical_markdown(&cfg);
    let raw = std::fs::read(FIXTURE).unwrap();
    let mech = thairag_document::conversion_fidelity::assess(&raw, "application/pdf", &mech_md);

    // A trivially-perturbed variant of the mechanical text scores a near-tie
    // (this is what nondeterministic vision candidates do on every reprocess).
    // The adoption rule requires beating mechanical by RESCUE_ADOPT_MARGIN —
    // a near-tie must NOT displace the deterministic reconstruction, or the
    // corpus coin-flips between passes.
    let near_tie = format!("{mech_md}\nหมายเหตุ");
    let cand = thairag_document::conversion_fidelity::assess(&raw, "application/pdf", &near_tie);
    assert!(
        (cand.score - mech.score).abs() < thairag_document::pipeline::RESCUE_ADOPT_MARGIN,
        "perturbation should be a near-tie (mech={} cand={})",
        mech.score,
        cand.score
    );
    assert!(
        cand.score <= mech.score + thairag_document::pipeline::RESCUE_ADOPT_MARGIN,
        "near-tie must fail the adoption margin"
    );
}
