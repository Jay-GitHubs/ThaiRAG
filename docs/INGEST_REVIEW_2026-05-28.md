# Document Ingestion Pipeline Review — 2026-05-28

End-to-end review of the document processing path: upload → conversion → chunking → embedding → vector DB. Conducted after PRs #71 (adaptive ingestion / EmptyExtraction), #72 (char-boundary panic in builtin_plugins), #73 (char-boundary panic in knowledge_graph) shipped.

**25 findings: 4 Critical, 9 High, 7 Medium, 6 Low.** All findings cite `file:line` against `main` at `f376310`. Verify against current code before acting on long-lived items.

> **Status re-check (2026-07-12):** spot-verified — **C1 still open**
> (`documents.rs` ingest spawn has no panic guard) and **H2 still open**
> (`thairag-document/src/thai_chunker.rs:158` compares `chars().count()`
> against byte `len()`, so the Thai clause-split threshold misfires ~3× —
> behavior-affecting; fix needs its own measured PR since it shifts chunking).
> Much of the surrounding pipeline was rebuilt since (region router #219–#235,
> table campaign #331–#336, backpressure #338), so several other findings are
> superseded — re-verify each before acting.

Out of scope (already addressed): the three PRs above; the operational issue that `image_description_enabled` defaults to `false`; per-workspace ingestion config (acknowledged deferred).

---

## 🔴 Critical (will panic, lose data, or silently corrupt)

### C1. Tokio-spawned ingest tasks have no panic guard — documents wedge in `Processing` forever
`crates/thairag-api/src/routes/documents.rs:130-165`, `:869-897`, `:1170-1269`.

`process_document_inner` calls into the converter, AI pipeline, chunkers, plugins and remote services. Any panic in those code paths (out-of-bounds slice, `unwrap()` on a malformed PDF, arithmetic overflow in a third-party crate) propagates up to the spawned task and silently aborts it. There is no `catch_unwind`, no `JoinHandle::await`, and no `Drop` finalizer that writes a terminal status. The doc is left as `DocStatus::Processing` and the job as `JobStatus::Running` permanently. Reproduce by getting any pipeline component to panic (e.g. a malformed PDF that trips `pdf-extract`'s internal `unwrap`).

### C2. In-memory `JobQueue` + `Processing` status are lost on every restart
`crates/thairag-api/src/job_queue.rs:9-40` (`InMemoryJobQueue` is `DashMap`-only, never persisted) combined with `documents.rs:130` and `:441` (inserts row with `DocStatus::Processing` before spawning).

Restart the API mid-ingest and the doc row stays `Processing` forever; the matching job is gone so the admin UI shows orphans with no way to recover except manual SQL. There is no startup reconciliation pass that re-fails or re-queues stale `Processing` documents.

### C3. Reprocess pollutes the SQL `document_chunks` table (Tantivy rebuild double-indexes)
`crates/thairag-api/src/routes/documents.rs:837-867` (`reprocess_document`) and `:951-1018` (`reprocess_all_documents`).

Both call `search_engine.delete_doc()` (clears Tantivy + vector store) and then re-run `process_document_inner`, which calls `state.km_store.save_chunks(&chunks)` at `documents.rs:297`. Because every reprocess generates fresh `ChunkId::new()` UUIDs (`pipeline.rs:283`, `:564`, `:700`, `:733`), the `INSERT OR REPLACE` at `sqlite.rs:973` never overwrites old rows — they only cascade-delete when the document itself is deleted. On the next API restart, `load_all_chunks` (`sqlite.rs:989`) reads both old + new and feeds them all back into Tantivy via `reindex_text_search`, doubling/tripling BM25 hits for every reprocessed document. The `refresh_document_from_source` path (`documents.rs:1849`) has the same defect.

### C4. `save_document_blob` overwrites the original bytes on every reprocess/refresh
`crates/thairag-api/src/store/sqlite.rs:723-737`, called from `process_document_inner` at `documents.rs:220-232` on every run including reprocess.

The `ON CONFLICT(doc_id) DO UPDATE SET original_bytes = ?2` clobbers `original_bytes` with whatever bytes are being processed. On refresh-from-source (`documents.rs:1928`) the newly-fetched bytes overwrite the user's originally-uploaded file. There is no audit trail; downloads after a refresh return the new content, not what the user uploaded.

---

## 🟠 High (operational bug or significant gap)

### H1. `reqwest::Client::builder()…build().unwrap_or_default()` silently drops the 60s timeout on refresh
`crates/thairag-api/src/routes/documents.rs:1878-1881`. If the builder ever fails, the fallback `Client::default()` has no timeout, and a malicious or slow `source_url` will hang the refresh task indefinitely. Combined with C1 (no panic recovery), this can stall refresh slots permanently.

### H2. `ThaiAwareChunker::split_on_thai_boundaries` mixes char count and byte length
`crates/thairag-document/src/thai_chunker.rs:158`: `current.trim().chars().count() > token.trim().len()`. For Thai tokens (3 bytes/char), `token.trim().len()` is 3× its char count, so the guard rejects valid clause splits whenever `current` is short Thai text. Effect: Thai documents skip the intended `แต่/และ/หรือ` clause-boundary split and produce coarser chunks than configured, reducing retrieval recall. Silent correctness regression in the headline language path.

### H3. `MarkdownChunker::chunk` uses `current.len()` (bytes) against `max_size` (intended as chars)
`crates/thairag-document/src/chunker.rs:30`, mirrored in `thai_chunker.rs:271` (`chunk_standard`). For a Thai/CJK paragraph, `len()` is 3× the char count, so chunks split far earlier than configured. Combined with H2, free-tier users on Thai content get unpredictable chunk sizes that don't match `max_chunk_size`.

### H4. Oversized single paragraphs are emitted as one giant chunk without splitting
`crates/thairag-document/src/chunker.rs:81-86` is the *documented* behaviour. There is no hard-cap fallback that force-splits at character boundaries when a paragraph exceeds `max_chunk_size`. A 100 KB Wikipedia-style paragraph produces a single chunk that exceeds the embedding model's context window (Ollama silently truncates at ~8 KB), and BM25 indexes it as one document. Pipeline reports success but retrieval quality collapses for that doc.

### H5. `apply_chunk_plugins` has no panic isolation and no per-plugin error handling
`crates/thairag-api/src/plugin_hooks.rs:51-67`. The trait method is `fn transform_chunk(&self, chunk: &str) -> String` — no `Result`, no `catch_unwind`. A buggy or third-party `ChunkPlugin` that panics brings down the whole ingest task (then per C1, the doc is stuck in `Processing` forever). Document plugins at least have a `Result` and a `warn!` fallback (`plugin_hooks.rs:36`); chunk plugins do not.

### H6. `process_document_inner` discards every store error with `let _ =`
`crates/thairag-api/src/routes/documents.rs:208, 220, 230, 250, 262, 288, 297, 324, 334`. If SQLite is full, the disk is unmounted, or a constraint trips, the document silently keeps its previous status. Most damagingly, the final `update_document_status(..., Ready, ...)` at `:325` is fire-and-forget — failure leaves the doc `Processing` even though chunks were indexed.

### H7. No `save_current_version` on the refresh path
`crates/thairag-api/src/routes/documents.rs:1849-1957` (`refresh_document_from_source`) and the batch reprocess (`:951`) skip `save_current_version`, which the manual reprocess (`:824`) does call. Scheduled refreshes therefore destroy old version history without warning — manual reprocess gets v1/v2/v3 audit trail, but a 6h-refresh doc overwrites itself indefinitely.

### H8. `/chunks` preview rebuilds chunks from converted text with `mime_type="text/plain"`
`crates/thairag-api/src/routes/documents.rs:759-769`. The preview re-runs `document_pipeline.process(text, "text/plain", ...)` against the already-converted markdown. So the chunks shown in the admin UI are NOT what was actually indexed — different chunker path (Thai detection re-runs on converted text, PDF page metadata is dropped, table extraction may pick up different tables, summary plugin re-runs). Users debugging "why isn't this chunk findable?" see fabricated chunks.

### H9. Tantivy rebuild on startup uses chunks without metadata
`crates/thairag-api/src/store/sqlite.rs:1003-1012`: `load_all_chunks` returns `DocumentChunk` with `metadata: None` and `embedding: None`. When `reindex_text_search` (`hybrid.rs:97`) re-indexes these, the `enriched_text` (`hybrid.rs:102`) skips `context_prefix` / `keywords` / `hypothetical_queries` that were present at original index time. BM25 ranking changes after every restart for any doc indexed with AI enrichment.

---

## 🟡 Medium (correctness/UX issue with workaround)

### M1. `extract_table_chunks` concatenates chunk content with no separator before scanning
`crates/thairag-document/src/pipeline.rs:374`: `chunks.iter().map(|c| c.content.as_str()).collect()`. Two adjacent chunks ending/starting with `|` are glued together with no newline, producing spurious "tables" from text that never had a table boundary. Use `.collect::<Vec<_>>().join("\n")` instead.

### M2. `process_document` returns `chunk_count = 0` for background uploads while the doc may already be Ready
`crates/thairag-api/src/routes/documents.rs:166`. The HTTP response says `"chunks": 0, "status": "processing"` even for trivially-quick docs whose `process_document_inner` would have finished before the response is sent. Polling `/documents/{id}` is the only way to learn the truth.

### M3. `SummaryChunkPlugin` runs on every chunk including image-description and table chunks
`crates/thairag-api/src/plugin_hooks.rs:57-61` iterates over all chunks regardless of `chunk.metadata.content_type`. For an image-description chunk the first sentence becomes the "summary" prepended to itself, redundant header. For table chunks the plugin sees `| col | col |` markdown and prepends `[Summary: | col | col |]`, breaking the table structure. Skip non-text chunks or gate by `chunk_type`.

### M4. No ingestion metrics
`crates/thairag-api/src/metrics.rs:17-163`. The new `EmptyExtraction` reason codes (`pipeline.rs::empty_reason`) are perfect cardinality for a Prometheus counter — wire them up so operators can alert on `rate(empty_extraction_total{reason="no_text_vision_unavailable"}[5m]) > 0` instead of grepping logs. Same for `vision_pages_used` / `pages_over_budget` from `pipeline.rs:593-601`. No `document_ingest_total{result, mime}` counter, no `document_ingest_duration_seconds` histogram.

### M5. `save_document_blob` failure is swallowed during ingest
`crates/thairag-api/src/routes/documents.rs:220-234`. If the blob insert fails, `let _` discards the error; processing continues, chunks index, doc goes Ready, but `download_document` returns `NotFound("Original file not stored")` (`:709`) and `reprocess_document` returns the same (`:831-835`). A Ready doc that cannot be reprocessed is a foot-gun.

### M6. `upload_document` extension map narrower than `SUPPORTED_MIME_TYPES`
`crates/thairag-api/src/routes/documents.rs:1433-1448` is missing the four image types plus has no `.json` mapping (returns `application/json`, which is NOT in `SUPPORTED_MIME_TYPES` at `converter.rs:95`). A user uploading `data.json` with no content-type gets `Unsupported MIME type: application/json` from `validate_mime_type` even though the UI advertises `.json`. Fix by adding `application/json` to the supported list or remapping `.json` to `text/plain`.

### M7. `SummaryChunkPlugin` heuristic uses `first_sentence.len() >= trimmed.len() - 1`
`crates/thairag-api/src/builtin_plugins.rs:145`. Mixes byte length with character semantics — works on ASCII, but the "is this chunk one sentence?" check misfires for Thai content where the first punctuation lands very late but byte-length math is dominated by 3-byte chars. Effect: chunks that should skip the summary header still get one (mostly benign noise).

---

## 🟢 Low (nice-to-have, tech debt)

### L1. `pdf_rasterizer::page_count` is defined but never called from the pipeline
`crates/thairag-document/src/pdf_rasterizer.rs:172-194`. Could be used at upload time to refuse PDFs whose page count exceeds `pdf_max_vision_pages * some_factor` before any rasterization.

### L2. `pdf_empty_reason::pages_over_budget` arithmetic is misleading
`crates/thairag-document/src/pipeline.rs:635`: `used = self.pdf_max_vision_pages.saturating_sub(pages_over_budget)`. `pages_over_budget` is "pages we wanted to OCR but couldn't"; subtracting it from the cap does not yield "pages used" — the hint string prints an incorrect "used" number to the operator.

### L3. `stream_jobs` SSE loop has no client-disconnect detection
`crates/thairag-api/src/routes/documents.rs:1533-1558`. The 2s `sleep` + 15s keep-alive will detect dropped clients eventually, but each open SSE consumer holds a clone of `AppState` and walks the full job map every tick — many tabs × many workspaces = O(tabs × jobs) per 2s.

### L4. Admin UI hides the real upload error from the user
`admin-ui/src/components/documents/UploadModal.tsx:34-36` catches with `catch {}` and shows a generic `'Upload failed'`. API returns a structured `ApiError` with body containing the reason — surface it.

### L5. Admin UI hardcodes "Max 10MB"
`admin-ui/src/components/documents/UploadModal.tsx:64`. Actual limit is `state.config.document.max_upload_size_mb` (50MB after the recent nginx fix). Wire the real number from `/settings`.

### L6. `BACKGROUND_THRESHOLD = 1MB` is fixed at compile time
`crates/thairag-api/src/routes/documents.rs:374`. A 2 MB PDF blocks the HTTP request for seconds to minutes if AI preprocessing is enabled. Make it configurable, and drop the threshold when AI preprocessing is on.

---

## Suggested PR ordering

| PR | Scope | Effort |
|---|---|---|
| **A** | C1: `catch_unwind` wrapper around spawned ingest task + always-write terminal status | ~30 lines |
| **B** | C2: Startup reconciliation — find `Processing` docs older than N min on boot, mark Failed | ~50 lines |
| **C** | C3 + C4: Delete-old-chunks-before-reprocess + skip blob overwrite when bytes unchanged | ~40 lines |
| **D** | H2 + H3 + H4: ThaiAwareChunker char/byte fixes + oversized-paragraph hard-split fallback | ~80 lines + tests |
| **E** | H5: `Result`-returning ChunkPlugin trait + per-plugin `catch_unwind` | breaking change to plugin API |
| **F** | M3 + M4 + L4: Skip Summary plugin on non-text chunks, ingestion metrics, surface real upload errors in UI | ~100 lines |
