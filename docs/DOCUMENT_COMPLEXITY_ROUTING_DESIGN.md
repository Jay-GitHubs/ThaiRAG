# Document Complexity Routing — Design & Roadmap

Status: **Proposed** · Owner: Document pipeline · Related: `docs/DOCUMENT_PROCESSING_FLOW.md`, `docs/ARCHITECTURE.md`

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

| Need | Already exists | Gap |
|---|---|---|
| Per-region classifier | `semantic::select_page_strategy` (PDF pages) | Generalize to DOCX/XLSX/HTML regions; emit a unified profile |
| Document-level analysis | `ai/analyzer.rs::DocumentAnalysis` | Fold into the profile as one signal source |
| Corruption signal | `text_utils::text_layer_garbled` | — (done) |
| Deterministic tables | `table_lattice`, `table_stream` | — |
| Format extraction | `converter.rs`, office extractors, `conversion_fidelity.rs` | Expose as tier-1 handlers behind a common trait |
| Vision description | `image::describe_image_with_prompt*`, `smart_pdf` | Reframe as tier-3 handler |
| **Deterministic OCR** | — | **New** tier-2 `OcrProvider` (PaddleOCR/EasyOCR/Typhoon) |
| Region orchestration | `smart_pdf::render_to_document` (concurrent, ordered) | Lift the concurrent/ordered pattern to the general router |

Proposed core abstraction:

```rust
trait RegionHandler {
    async fn handle(&self, region: &Region, profile: &RegionProfile) -> RegionResult;
}
// implementations: NativeExtract, DeterministicTable, DeterministicOcr, VisionDescribe
```

with the router selecting a handler per region from the profile + thresholds.

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

Integration shape (decided after the bench wins): a **sidecar microservice** or an
**ONNX-exported model via the Rust `ort` crate** — ThaiRAG is Rust, these engines
are Python, so this is a real new component, not a config flag.

Why this matters: a dedicated OCR engine is *deterministic* (transcribes, doesn't
generate), **fast and local**, and **not bottlenecked on the gateway** — which
removes the issues observed with VLM OCR (a single heavy VLM instance is slow,
fabricates/repeats, and doesn't parallelize behind a shared gateway). The VLM
stays for what only it can do: semantic description of figures/diagrams.

## 7. Phased roadmap

Each phase ships independently and is gated on measured improvement.

- **Phase 1 — Profiler + eval harness.** Compute the `DocumentProfile`/signals for
  all formats; assemble the labeled corpus; build triage + extraction scoring.
  Output: the corpus's real complexity distribution + a baseline of where
  extraction fails today. *Low risk, immediately useful, prerequisite for all
  threshold work.*
- **Phase 2 — Unified region router + fidelity ladder.** Promote
  `select_page_strategy` into the format-agnostic router behind `RegionHandler`;
  wire tier-1 (native) and tier-3 (VLM) handlers; enforce the golden rule.
- **Phase 3 — Deterministic OCR provider.** Bench candidates on the corpus; if a
  winner beats VLM OCR on Thai accuracy + speed, integrate it as the tier-2
  handler (sidecar/ONNX).
- **Phase 4 — Threshold calibration.** Tune every routing threshold per class
  against the harness; persist as config with sane defaults.
- **Phase 5 — Format coverage.** Full DOCX/XLSX/HTML region profiling so the
  golden rule (e.g. never OCR a DOCX table) holds end-to-end.

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

Start with **Phase 1**. It is low-risk, makes the corpus's complexity *visible*,
establishes the baseline, and turns every later threshold/algorithm choice into a
measured decision instead of a guess.
