# Document Complexity Routing — Design & Roadmap

Status: **Largely shipped** (PRs #219–#235) · Owner: Document pipeline · Related: `docs/DOCUMENT_PROCESSING_FLOW.md`, `docs/OCR_VS_VLM_SPIKE.md`, `docs/ARCHITECTURE.md`

> **Implementation status (2026-06):** The core of this design is now built and
> merged. The region router (`crates/thairag-document/src/region_router.rs`), the
> deterministic OCR tier (`ocr.rs` + `services/paddleocr-sidecar/`), per-document
> handling-mode overrides, the pre-ingest dry-run preview API + admin UI, and
> extraction provenance all ship. The design below is preserved as the rationale;
> per-item status is tracked in §4 (reuse table) and §7 (phased roadmap). One
> naming divergence from the final code is noted in §4.

## 1. Motivation

Real-world corpora are heterogeneous and arrive at unpredictable quality. A single
DOCX can contain clean body text, an embedded scanned table, and a diagram — each
needing a *different* extraction method. PDFs range from clean digital text to
PowerPoint exports, to scans, to files with a **corrupted ToUnicode CMap** whose
text layer is garbage in every extractor (the `เรืĻอง` class of bug).

Today the pipeline handles much of this, but the logic is **scattered and
format-specific**:

- PDFs get a rich per-page strategy selector (`semantic::select_page_strategy`).
- DOCX/XLSX/HTML go through a separate, less sophisticated embedded-media path.
- Signals (garble detection, table-likeness, image coverage, OCR-quality) live in
  different modules with per-feature thresholds that were set independently.

We keep bolting on point fixes (a garble detector here, a retry there). This doc
proposes consolidating that into one **document triage layer** that profiles a
document's complexity and routes each *region* to the best-fit algorithm, with
thresholds that are **calibrated against a labeled corpus**, not guessed.

This is a generalization of where the code already is — `select_page_strategy` is
the seed of exactly this idea, applied only to PDF pages today.

## 2. Core principle: a fidelity ladder, decided per region

Two ideas drive the whole design.

**(a) The unit of decision is a *region*, not a document.** A region is a PDF page,
a DOCX block/section, an XLSX sheet, an HTML node subtree, or an embedded image.
Each region is profiled and routed independently, then results are reassembled in
reading order.

**(b) Each region climbs down a fidelity ladder** and stops at the first method
that can handle it:

```
1. Native structured extraction  — DOCX XML, XLSX cells, PDF text layer,
                                    deterministic lattice/stream tables
                                    → EXACT. Always preferred.
2. Deterministic OCR             — PaddleOCR / EasyOCR / Typhoon (pixels → text)
                                    → no hallucination, fast, local.
3. Vision LLM                    — figure/diagram description, last-resort OCR
                                    → probabilistic. Only when forced.
```

**The golden rule that falls out of this:** never use a probabilistic method
(OCR or VLM) when a deterministic one is available for that region. You must
*never* OCR a DOCX table — its XML carries the exact cells. This single rule
eliminates a large class of real-world errors and directly attacks the measured
table-accuracy bottleneck (Thai tables ~33–50%).

Corollary (already a design value in `smart_pdf`): a deterministic table that
can't be confidently reconstructed keeps its **raw text verbatim** rather than
being handed to a VLM that may fabricate Thai numerals.

## 3. The triage stage

A new profiling stage runs before extraction and emits a structured
`DocumentProfile`, then a router maps each region to a handler.

### 3.1 Signals to compute (the measurable inputs)

Per document and per region, cheaply (no LLM unless necessary):

| Signal | Source | Purpose |
|---|---|---|
| Source format | MIME | Pick the native extractor tier |
| Text-layer char density | pdfium / XML / DOM | Digital vs scanned |
| Image coverage ratio | pdfium page render | Image-heavy vs text |
| Embedded-image count/size | pdfium / office media | Mixed vs text-only |
| Table-likeness | `table_extractor::looks_like_table`, geometry | Tabular routing |
| Lattice/stream reconstructability | `table_lattice`, `table_stream` | Deterministic table vs OCR |
| **Text-layer corruption** | `text_utils::text_layer_garbled` | CMap garble → OCR |
| Script / language mix | unicode histogram | OCR model choice, Thai handling |
| Skew / contrast (scans) | image stats | Pre-OCR deskew/denoise need |
| OCR-quality estimate | `DocumentAnalysis.needs_ocr_correction` | Confirm scan handling |

### 3.2 Complexity classes (per region)

Generalize the current PDF `PageStrategy` into a format-agnostic set:

- `NativeText` — clean structured text (text layer / XML / DOM).
- `NativeTable` — deterministically reconstructable table (lattice/stream/XML/cells).
- `CorruptedText` — text present but untrustworthy (garbled CMap) → OCR.
- `Scanned` — no usable text layer → OCR.
- `ImageHeavy` — text + dominant imagery → OCR + description.
- `Mixed` — text + embedded figures → native text + figure description.
- `Degraded` — low-quality scan (skew/noise) → preprocess → OCR.

### 3.3 Router → handler mapping

| Class | Handler (fidelity tier) |
|---|---|
| NativeText | Native extraction (tier 1) |
| NativeTable | Deterministic table reconstruction (tier 1) |
| CorruptedText | Deterministic OCR (tier 2), VLM fallback |
| Scanned | Deterministic OCR (tier 2), VLM fallback |
| Degraded | Preprocess → Deterministic OCR (tier 2) |
| ImageHeavy | OCR (tier 2) for text + VLM (tier 3) for description |
| Mixed | Native text (tier 1) + VLM (tier 3) for figures only |

## 4. Mapping onto the existing codebase (reuse, don't rebuild)

The machinery is ~60% present; the work is unification and a new OCR tier.

| Need | Already exists | Status |
|---|---|---|
| Per-region classifier | `semantic::select_page_strategy` (PDF pages) | **SHIPPED** — `region_router::classify` generalizes it; non-PDF docs route by format to `NativeStruct`/`DirectImage`/`NativeText` |
| Document-level analysis | `ai/analyzer.rs::DocumentAnalysis` | Available as a signal source; not yet folded into the router's `RegionSignals` |
| Corruption signal | `text_utils::text_layer_garbled` | **SHIPPED** — drives the `CorruptedText` class |
| Deterministic tables | `table_lattice`, `table_stream` | **SHIPPED** — a reconstructed table forces `NativeTable` (golden rule) |
| Format extraction | `converter.rs`, office extractors, `conversion_fidelity.rs` | Tier-1 native path, in place (not refactored behind a new trait — see note) |
| Vision description | `image::describe_image_with_prompt*`, `smart_pdf` | **SHIPPED** as the `VisionLlm` tier; `Mixed` regions flag `needs_figure_description` |
| **Deterministic OCR** | — | **SHIPPED** — tier-2 `OcrProvider` trait + `SidecarOcrProvider` (PaddleOCR sidecar). EasyOCR/Typhoon were evaluated but PaddleOCR was chosen |
| Region orchestration | `smart_pdf::render_to_document` (concurrent, ordered) | **SHIPPED** — the OCR/vision tiers are wired into the existing concurrent/ordered `render_to_document` per page |

**Naming divergence from the design (code wins):** the shipped code does *not* use a
`RegionHandler` trait with `NativeExtract`/`DeterministicTable`/`DeterministicOcr`/
`VisionDescribe` implementations. Instead the decision and execution layers are split:

- **Decision** — `region_router.rs` is pure and IO-free: `RegionClass` + `FidelityTier`
  (`Native` / `DeterministicOcr` / `VisionLlm`) + `classify()`/`plan()` over
  `RegionSignals`, with `RegionClass::tier()` encoding the golden rule and
  `RegionClass::from_page_strategy` bridging the PDF taxonomy.
- **Execution** — `smart_pdf::render_to_document` runs the selected tiers per page
  (native extraction / `OcrProvider::ocr` / vision LLM), rather than dispatching to
  per-region handler objects.

The original proposed abstraction, for the record:

```rust
trait RegionHandler {
    async fn handle(&self, region: &Region, profile: &RegionProfile) -> RegionResult;
}
// implementations: NativeExtract, DeterministicTable, DeterministicOcr, VisionDescribe
```

## 5. Thresholds: calibrate, never guess

Every routing threshold — image-coverage cutoff, min chars/page, garble ratio,
"is this a table" confidence, skew angle, OCR-quality floor — is a tunable that
**must be set from data**. This mirrors how the rest of the repo was built
(temp-0 numeric-aware eval harness, measured table/near-clone findings). Guessed
thresholds silently mis-route documents.

Required before tuning:

1. **Labeled eval corpus** spanning the real classes: clean digital PDF, corrupted
   CMap, scanned, PowerPoint export, DOCX-with-tables, XLSX, HTML, photographed/
   skewed scans — with ground-truth text/tables.
2. **Two-level scoring harness** (extend `scripts/bench/`):
   - *Triage accuracy*: did the router pick the right class/handler?
   - *Extraction accuracy*: Thai-correct text, exact table cells, correct order.
3. Tune per class against numbers; record in `docs/BENCHMARK_RESULTS.md`.

## 6. Deterministic OCR tier — open evaluation

The tier-2 OCR engine is unproven for Thai on our corpus and must be benchmarked
before integration. Candidates:

- **PaddleOCR / PP-Structure** — strong layout + table structure; generic Thai is
  variable.
- **EasyOCR** — lighter (PyTorch), Thai supported.
- **Typhoon OCR** — Thai-focused; potentially best Thai accuracy.

Integration shape (decided after the bench): a **PaddleOCR sidecar microservice**
(`services/paddleocr-sidecar/`, FastAPI, `th_PP-OCRv5`). ONNX-via-`ort` was the
alternative but was not pursued — ThaiRAG is Rust, PaddleOCR is Python, so this is a
real new component rather than a config flag (run as an opt-in Docker Compose profile).

Why this matters: a dedicated OCR engine is *deterministic* (transcribes, doesn't
generate), **fast and local**, and **not bottlenecked on the gateway** — which
removes the issues observed with VLM OCR (a single heavy VLM instance is slow,
fabricates/repeats, and doesn't parallelize behind a shared gateway). The VLM
stays for what only it can do: semantic description of figures/diagrams.

## 7. Phased roadmap

Each phase ships independently and is gated on measured improvement.

- **Phase 1 — Profiler + eval harness.** ✅ **SHIPPED.** `region_router.rs` is the
  pure profiler/classifier (`classify`/`plan` over `RegionSignals`). The OCR
  evaluation harness (`scripts/bench/ocr_vs_vlm.py`, `ocr_eval_cer.py`) produced the
  graded Thai CER numbers in `docs/OCR_VS_VLM_SPIKE.md`. *Note:* a fully labeled,
  multi-class triage-accuracy corpus across every class is not yet assembled — the
  shipped eval focused on the OCR-vs-VLM transcription decision (Phase 3 gate).
- **Phase 2 — Unified region router + fidelity ladder.** ✅ **SHIPPED.**
  `select_page_strategy` is promoted into the format-agnostic `region_router`
  (`RegionClass` + `FidelityTier`), tier-1 (native) and tier-3 (vision) paths run in
  `smart_pdf::render_to_document`, and the golden rule is enforced in
  `RegionClass::tier()` (table tests prove a reconstructed table is never OCR'd).
  Shipped as a decision/execution split rather than the `RegionHandler` trait — see §4.
- **Phase 3 — Deterministic OCR provider.** ✅ **SHIPPED.** PaddleOCR `th_PP-OCRv5`
  beat the VLM on Thai CER (94.5% vs 90.1%) and reliability, so it was integrated as
  the tier-2 `OcrProvider` via an HTTP **sidecar** (`services/paddleocr-sidecar/`),
  preferred over the vision LLM when configured. **Default-off / opt-in** via
  `document.ocr_sidecar_url`; ONNX-via-`ort` was not pursued.
- **Phase 4 — Threshold calibration.** ⏳ **Partial / future.** The router runs on
  `StrategyThresholds::default()` (the inherited smart-PDF defaults), with
  per-document overrides exposed (`ChunkOverrides.image_coverage_threshold`,
  `min_chars_per_page`) and the `HandlingMode` escape hatches. A systematic
  data-driven re-tune of every routing threshold per class against a labeled corpus
  is **not yet done**.
- **Phase 5 — Format coverage.** ⏳ **Partial / future.** Non-PDF formats
  (DOCX/XLSX/HTML) are routed at *document* granularity to `NativeStruct` (native
  extraction). True per-*region* profiling *inside* an office doc — so the golden
  rule fires on, e.g., an embedded scanned table within a DOCX — is **not yet shipped**;
  PDF pages remain the only sub-document regions the router classifies individually.

## 8. Risks & open questions

- **Thai OCR accuracy** is the make-or-break for Phase 3 and is unproven — Phase 1
  must produce the bench before committing.
- **Sidecar complexity**: adds a Python/ONNX component to a Rust stack
  (deployment, health, versioning). Justify only if the bench wins decisively.
- **Spurious-space residue (`ความสา คญั`)**: present in *clean-but-mangled* text
  layers that won't trip the garble detector. OCR-from-pixels fixes it, but
  blanket Thai space-normalization is unsafe (Thai uses real phrase spaces) — keep
  it a routing decision, not a global post-process.
- **Reading order** for complex multi-column / diagram pages (PP-Structure may
  help) — measure before relying on it.
- **Backward compatibility**: existing PDFs already work via `smart_pdf`; the
  router must preserve or beat current behavior on every class (regression-gated
  by the Phase 1 harness).

## 9. Recommendation

*(Original recommendation, now superseded by the shipped work above.)* Start with
**Phase 1**. It is low-risk, makes the corpus's complexity *visible*, establishes the
baseline, and turns every later threshold/algorithm choice into a measured decision
instead of a guess.

**Update:** Phases 1–3 shipped (PRs #219–#235) along the path this recommendation set —
the deterministic OCR decision was made on measured Thai CER, not a guess. The
remaining open work is the data-driven threshold calibration (Phase 4) and
sub-document region profiling for office formats (Phase 5).
