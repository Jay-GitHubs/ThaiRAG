# Smart PDF pipeline: design

**Status:** Draft — open for review.

**Owner:** TBD
**Last updated:** 2026-05-28

---

## 1. Problem

ThaiRAG's PDF ingestion path is brittle for any document that isn't pure text:

| Document type | Current behaviour | Root cause |
|---|---|---|
| Pure-text PDF (e.g. exported from Word) | ✅ Works — text extracted, chunked, indexed | `pdf-extract` handles this fine |
| PowerPoint-exported PDF (slides rasterized) | ❌ Fails with `empty_extraction[no_text_vision_unavailable]` unless vision LLM is wired AND vision fallback gate passes | `pdf-extract` returns empty text; the whole-document gate (`pipeline.rs:344-352`) requires four flags to align |
| Mixed PDF (text + embedded diagrams) | ⚠️ Text indexed, images silently dropped | `pdf-extract` is text-only; embedded image objects are never enumerated |
| Scanned PDF (every page is a TIFF) | ❌ Same failure as PowerPoint export | Same root cause |
| PDF with embedded tables (image of a table) | ⚠️ Text-around-the-table indexed; the table itself is silently dropped | No table-aware OCR triggering |
| Direct image upload (PNG/JPG) | ✅ One chunk with vision description (PR #71); image bytes themselves discarded | `process_image` stores description but not the image for retrieval |

PR #71 (`EmptyExtraction` error) and PR #77 (dedicated `vision_llm` config) made the failure mode honest and configurable. But the underlying pipeline is still **"extract text, fail or fall back if empty"** — a whole-document binary decision, with `pdf-extract` (text-only) as the only PDF backend.

The proposal: replace this with a **per-page strategy selector** modeled on the patterns in `/Volumes/Jay-SSD/MyCodings/Jay-RAG-Tools/` (referred to throughout as "Jay-RAG-Tools"), which uses pdfium-render to enumerate page objects, computes per-page image coverage, and picks one of several extraction strategies *per page* rather than per document.

## 2. Goals / non-goals

In scope:
- Per-page strategy for PDF ingestion: text-only / image-heavy / mixed / table-detected.
- Adopt `pdfium-render` as the PDF backend, replacing or supplementing `pdf-extract`.
- Preserve embedded image bytes alongside chunk text so the admin UI / chat pipeline can surface images, not just descriptions.
- A graceful fallback when the pdfium binary isn't installed (degraded to current `pdf-extract` path with the existing fail-loud guard).
- Compatibility with PR #77's `[providers.vision_llm]` — the vision LLM is still the OCR engine; the smart pipeline decides *what* to send it.

Out of scope (future PRs / RFCs):
- DOCX / XLSX / HTML smart pipelines. PowerPoint-as-PPTX (rather than PowerPoint-as-PDF) is its own format and isn't supported by the converter today; that's a separate effort.
- Chat-pipeline support for sending extracted images directly to the answer LLM as context (the multimodal-retrieval story). For Phase 1–2 the image is searchable via its text description; Phase 3 would let vision-capable answer models actually see the image.
- Replacement of the current `ChunkMetadata` enrichment fields. Smart-PDF additions extend the metadata, they don't replace it.

## 3. Design — per-page strategy selector

After loading the PDF via pdfium, we walk pages and pick a `PageStrategy` for each:

```rust
enum PageStrategy {
    /// pdfium extracted meaningful text and the page is mostly text by area.
    TextOnly { text: String },
    /// Mixed: text + embedded images, both worth keeping.
    Mixed { text: String, images: Vec<ExtractedImage> },
    /// Image coverage ≥ threshold — render whole page as PNG; include any text as context for vision LLM.
    ImageHeavy { page_png: Vec<u8>, hint_text: String },
    /// pdfium extracted text that looks tabular (`crate::table::looks_like_table`); ALSO render the page so vision can read the actual table layout.
    Tabular { text: String, page_png: Vec<u8> },
    /// Fallback: pdfium returned nothing usable. Render whole page, no hint text. Same outcome as ImageHeavy but flagged for diagnostics.
    Scanned { page_png: Vec<u8> },
}
```

Selection logic (mirrors `Jay-RAG-Tools/crates/core/src/processor.rs:238-303`):

```
let coverage = PdfEngine::get_image_coverage(&page);  // image bounds ÷ page area
let text     = PdfEngine::extract_page_text(&page);
let text_len = meaningful_char_count(&text);  // reuses thairag-document::text_utils

match (coverage, text_len) {
    (c, t) if c >= 0.50  &&  t < 50  => Scanned    { page_png }
    (c, t) if c >= 0.50  &&  t >= 50 => ImageHeavy { page_png, hint_text: text }
    (_, t) if t < 50                 => Scanned    { page_png }   // text-poor & low coverage = empty/sparse page
    (_, t) if looks_like_table(text) => Tabular    { text, page_png }
    _                                => Mixed      { text, images }
}
```

Thresholds (`page_as_image_threshold = 0.50`, `min_text_meaningful_chars = 50`) live in `[document.smart_pdf]` config; defaults match Jay-RAG-Tools.

### 3.1 What gets emitted per strategy

| Strategy | Vision LLM call? | Chunk(s) produced |
|---|---|---|
| TextOnly | No | Standard text chunks via `chunk_with_strategy` |
| Mixed | One per image (if vision wired) | Text chunk(s) + one chunk per embedded image, each carrying `image_blob_id` |
| ImageHeavy | Yes — full page render + hint text as context | One chunk per page, content = vision description, `chunk_type = "pdf_page_ocr"`, `image_blob_id` set |
| Tabular | Yes — full page render, prompt asks for markdown table | One chunk = vision-extracted table (markdown), `chunk_type = "table_ocr"`, `image_blob_id` set |
| Scanned | Yes — same as ImageHeavy | Same as ImageHeavy but `chunk_type = "pdf_page_scanned"` for telemetry |

Vision LLM unavailability (no `providers.vision_llm`, no vision-capable primary) is handled per-page:
- `TextOnly` / `Mixed` (text part) → still produces text chunks
- `ImageHeavy` / `Tabular` / `Scanned` → produces a stub chunk with `chunk_type = "pdf_page_vision_unavailable"` containing the metadata placeholder (page number, dimensions, byte count) so the doc is still findable, AND surfaces a per-document `warning` (not a hard failure) listing the unprocessed pages
- The whole-document `EmptyExtraction` failure is reserved for the case where literally zero pages produced content. That should be unreachable in practice — even a scanned page produces at least a placeholder chunk.

### 3.2 Image storage

Two options:

**Option A — `document_blobs` reuse.** Add a sibling table `document_image_blobs(image_id PK, doc_id FK, blob BYTEA, mime TEXT, width INT, height INT, page_num INT, created_at)`. `ChunkMetadata` gets `image_blob_id: Option<ImageId>` which references this table.

**Option B — Filesystem.** Save PNGs under `data/images/{workspace_id}/{doc_id}/page_{N}_idx_{M}.png` and store the relative path in `ChunkMetadata.image_url`. Smaller DB, but introduces filesystem coupling that the current architecture has avoided.

**Recommendation: A.** DB-backed keeps backups simple (single `pg_dump` captures everything), and avoids the "blob walked away" failure mode. Storage cost is bounded — pdfium-rendered PNGs at 150 DPI are typically 100-300 KB; even a 100-page heavy document is < 30 MB and SQLite handles that. For workspaces with very high image volume we add an opt-in filesystem mode in a later PR.

### 3.3 Admin UI surface

- Chunk preview (`ChunksModal`) renders the inline image when `image_blob_id` is set, via a new `GET /api/km/workspaces/{ws}/documents/{doc}/images/{img}` endpoint.
- Per-document detail page shows a "Unprocessed pages" warning when any chunks have `chunk_type = "pdf_page_vision_unavailable"`, with a CTA to configure `vision_llm` (links to Providers settings).
- Search results that come from `chunk_type ∈ {pdf_page_ocr, pdf_page_scanned, table_ocr, image_description}` get a small icon badge so users know the source was OCR (helpful for debugging "why is this hit phrased oddly").

## 4. Why pdfium-render

`pdf-extract` (current) is text-only — it can't enumerate image objects, can't render pages, can't tell you image-coverage. Replacing it is a hard requirement for any per-page strategy.

| | `pdf-extract` | `pdfium-render` |
|---|---|---|
| Text extraction | ✅ Pure Rust | ✅ Via pdfium binary |
| Image object enumeration | ❌ | ✅ `page.objects().iter()` filtered by `PdfPageObjectType::Image` |
| Page rendering to PNG | ❌ | ✅ `render_with_config(width, height)` |
| Image bounds → coverage calc | ❌ | ✅ `object.bounds()` returns rect |
| Thai text fidelity | ⚠️ Sometimes mis-orders glyphs | ✅ pdfium preserves the layout engine's output |
| Dependency | Pure Rust, zero binary deps | Requires `libpdfium.dylib` / `.so` / `.dll` on the path |
| Build complexity | Minimal | Need to download pdfium binary per platform (CI matrix) |

The binary-dependency cost is real: CI needs a step to fetch the right pdfium artifact per OS, and Docker images need it baked in. Jay-RAG-Tools manages this with a `libpdfium.dylib` committed at the repo root for dev convenience plus README instructions for production.

### 4.1 Fallback

If pdfium fails to load at startup (binary missing, wrong arch), the smart PDF pipeline self-disables and PDF uploads route to the current `pdf-extract` path. The fallback writes a tracing warning on startup and a per-doc warning at upload time:

```
Smart PDF pipeline unavailable (pdfium not loaded); falling back to text-only extraction.
Image-only PDFs will fail with empty_extraction[no_text_vision_unavailable].
```

This means PR doesn't strictly *require* deployments to install pdfium — but advertises that they should.

## 5. Phasing

| Phase | Scope | LOC estimate | Blocks on |
|---|---|---|---|
| **Phase 1** | New `thairag-document::smart_pdf` module. Pdfium dep + binary loader with fallback. Per-page strategy selector. Vision LLM invocation per Image-heavy / Tabular / Scanned page. Image bytes saved to `document_image_blobs`. ChunkMetadata gets `image_blob_id`. Integration tests with sample PowerPoint-PDF and scanned-PDF fixtures. | ~600 | PR #77 (vision_llm config) |
| **Phase 2** | Admin UI: chunk preview shows inline image. New `GET /documents/{doc}/images/{img}` endpoint. Unprocessed-pages warning + CTA. Search result OCR-source badge. | ~300 | Phase 1 |
| **Phase 3** | Chat pipeline: retrieved chunks with `image_blob_id` pass the image bytes into the answer LLM's `VisionMessage` when the answer LLM supports vision. Multimodal retrieval. | ~250 | Phase 2 + multimodal_rag agent already in tree |

Phase 1 alone resolves the original user-reported issue ("Thai PowerPoint PDFs produce zero chunks") in a way that's robust to whatever PDF the user uploads next, instead of just enabling the existing narrow vision-fallback.

## 6. Configuration

```toml
[document.smart_pdf]
# Master switch. Default false initially; flip to true once integration tests cover
# enough PDF varieties and the pdfium binary is shipped in the Docker image.
enabled = false

# Per-page strategy thresholds (see §3)
page_as_image_threshold = 0.50    # coverage ≥ this → ImageHeavy or Scanned
min_text_meaningful_chars = 50    # reuses meaningful_char_count from PR #71

# Rendering
render_dpi = 150                  # 150 = good balance; Jay-RAG-Tools' Quality::High uses 300
enhance_images = true             # sharpen + contrast boost before PNG encode
min_embedded_image_size = 64      # ignore tiny embedded glyphs

# Budget caps (same spirit as existing pdf_max_vision_pages)
max_vision_pages_per_doc = 100    # hard cap so a 10k-page PDF can't blow up cost
max_image_blob_size_bytes = 5_242_880   # 5 MB per stored image
max_image_blobs_per_doc = 200     # cumulative cap

# Fallback
allow_pdfium_fallback_to_pdfextract = true
```

`vision_llm` for OCR is already configured under `[providers.vision_llm]` (PR #77).

## 7. Open questions for review

1. **Pdfium binary distribution.** Jay-RAG-Tools commits `libpdfium.dylib` at repo root for macOS dev. For ThaiRAG, do we want: (a) committed binaries per-arch (simple, repo bloat), (b) downloaded by `build.rs` from GitHub releases (clean repo, CI complications), (c) leave it to ops to install + document in OPERATOR_GUIDE (free tier docker image bakes it in)?
2. **Hint-text passing.** When ImageHeavy strategy fires, do we send the (sparse) extracted text as context to the vision LLM in the same VisionMessage, or as a separate ChatMessage? Jay-RAG-Tools embeds it in the prompt. Risk: long hint text + the page image could blow context window for small vision models.
3. **Per-image vision calls in Mixed strategy.** A page with 8 embedded screenshots → 8 vision calls. Budget vs accuracy. Should we cap at N images per page (and concatenate the rest) or process all?
4. **Image search ranking.** Image chunks have shorter text content (just the description). Should hybrid search downweight them, upweight them, or treat them identically? Current default treats them identically; this might cause OCR-derived chunks to dominate small KMs.
5. **Migration for existing documents.** When this PR merges, do existing documents (ingested via the dumb path) get reprocessed automatically, on next-reprocess only, or never? Recommendation: never (existing chunks remain valid); operators can hit "Reprocess All" if they want to upgrade. Document this in the PR description.

## 8. Acceptance criteria

The PR is ready to merge when:
- A test fixture of "Thai PowerPoint exported to PDF" (the original user-reported case) ingests to ≥ 1 chunk per page with `chunk_type = "pdf_page_ocr"` when `vision_llm` is wired.
- The same fixture, with `vision_llm` UNwired, ingests to ≥ 1 chunk per page with `chunk_type = "pdf_page_vision_unavailable"` AND the document goes Ready (not Failed) AND surfaces an admin-UI warning.
- A pure-text PDF (e.g. an arXiv paper) still routes to `TextOnly` for every page and produces the same chunks the current pipeline does (regression coverage).
- A PDF with one image-heavy cover page + 50 text pages routes the cover to `ImageHeavy` and the rest to `TextOnly`. **This is the test that the per-page selector actually works.**
- Pdfium binary missing → degrades to the current `pdf-extract` flow + tracing warning + same `empty_extraction` errors as today.
- All existing tests pass; new tests for `PageStrategy` selection, image-blob storage, and the fallback path.

## 9. Related work in the codebase

- `pipeline.rs::process_pdf_with_vision` (PR #68, PR #71) — the *current* vision fallback. The new smart pipeline supersedes it; this function gets deprecated and removed (or kept as a no-op alias for one release) once smart-PDF is on by default.
- `pipeline.rs::process_image` — handles direct image uploads. Already produces an image chunk; Phase 2 will add `image_blob_id` so the uploaded bytes are retained for display, matching the new PDF-image semantics.
- `text_utils::meaningful_char_count` (PR #71) — reused for the `min_text_meaningful_chars` threshold.
- `thairag-agent::multimodal_rag` — existing agent for multimodal retrieval. Phase 3 wires extracted images into the context it consumes.
- `ChunkMetadata` (`thairag-core::types`) — gets new optional fields: `image_blob_id`, `image_mime`, `image_width`, `image_height`, `page_strategy: Option<String>` (for telemetry).

## 10. References

- Jay-RAG-Tools project: `/Volumes/Jay-SSD/MyCodings/Jay-RAG-Tools/`
  - `crates/core/src/pdf.rs:60-200` — pdfium engine wrapper, `get_image_coverage`, `render_page_as_image`, `extract_page_images`
  - `crates/core/src/processor.rs:238-303` — `extract_page_data` with the per-page strategy selector that this design mirrors
  - `crates/core/src/table.rs::looks_like_table` — the tabular-text heuristic
- ThaiRAG ingestion review: `docs/INGEST_REVIEW_2026-05-28.md` (this design addresses finding C-PDF + H4 + M3)
- Existing design docs: `docs/ATTACHMENTS_DESIGN.md` (style reference)
