# Configuration Guide — Choosing a Chat Pipeline

This guide recommends ThaiRAG chat-pipeline configurations for common deployment
scenarios. Recommendations are grounded in a controlled benchmark — see
[BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md) for the data and methodology.

All settings below are configured in the Admin UI under **Settings → Chat & Response
Pipeline**, or via `PUT /api/km/settings/chat-pipeline`. Changes hot-reload (no restart).

---

## The three knobs

1. **Model** (`llm`) — the LLM that answers. Bigger ≠ always better; `gemma4:e4b` beat
   the 35b on correctness here.
2. **Mode** (`llm_mode` + `orchestrator_enabled`):
   - **Lean** — orchestrator off. Fastest. Routes every query through a simple
     retrieve-then-answer path. *Note: the quality-guard stage does not run in lean mode.*
   - **Full** — orchestrator on. Runs query analysis + context curation (and any extra
     agents you enable). ~Slightly slower, meaningfully better for small models.
   - **shared** vs **per-agent** — one model for all stages, or a different model per
     stage. Per-agent tiering did **not** pay off in the benchmark.
3. **Feature agents** — `query_rewriter`, `quality_guard`, `self_rag`, etc. Each adds
   LLM calls (latency + cost). Enable only when it earns its keep.

---

## Recommended configurations

### Balanced (recommended default)

Best correctness-per-second for most deployments.

| Setting | Value |
|---|---|
| Model | `gemma4:e4b-it-bf16` |
| Mode | lean **shared** |
| Optional agents | off |

Why: top correctness in the grid (token 0.83, judge 0.55) and the strongest Thai score,
at a moderate p50 ≈ 27s — faster than the 35b. A small (e4b) model, so single-GPU friendly.

*Alternative if latency matters more than peak quality:* keep the current
**`chinda-qwen3-4b`** but turn the **full pipeline on** (`orchestrator_enabled: true`,
lean's other agents as-is). That lifted chinda from judge 0.35 → 0.52 / token 0.62 → 0.73
for only ~4s more median latency, while keeping a 4B model and a ~20s p50.

### Accuracy-first

When answer quality outranks speed (e.g. internal expert tooling).

| Setting | Value |
|---|---|
| Model | `gemma4:e4b-it-bf16` (or `qwen3.6:35b`) |
| Mode | **full** shared (`orchestrator_enabled: true`) |
| Optional agents | `+query_rewriter` if retrieval recall is weak |

Why: full pipeline + a strong model maximizes grounded correctness. `+query_rewriter`
gave the best token coverage (0.83) when the question wording doesn't match the source.
**Skip per-agent tiering** — swapping in the 35b only for response/guard did not beat
full-shared and roughly doubled latency.

### Fastest / lowest-RAM (single small GPU)

When throughput or memory is the constraint (e.g. free tier, edge box).

| Setting | Value |
|---|---|
| Model | `iapp/chinda-qwen3-4b` |
| Mode | lean **shared** |
| Optional agents | off |

Why: fastest in the grid (p50 16.5s) and a 4B model. Accept lower correctness
(judge 0.35) — and note the weak Thai performance below. If you have ~4s of headroom,
turning the full pipeline on is a strong upgrade for almost no extra latency.

---

## If your users ask in Thai — read this

The benchmark's most important finding: **every model answered Thai questions far worse
than English** (judge ≈ 0.04–0.24 Thai vs 0.61–0.79 English). The correct source chunk
was always retrieved; the models simply tended to write generic Thai advice instead of
extracting the specific facts from the table.

Practical guidance:
- Prefer **`gemma4:e4b-it-bf16`** (best Thai score, though only 0.22 and noisy across runs).
- The **full pipeline** (orchestrator on) does **not** reliably help here. A targeted
  follow-up — lean vs full on the Thai questions, both models — showed full matching gemma
  exactly and *hurting* chinda (0.24 → 0.12). Because the correct row is already retrieved,
  adding query-analysis/curation stages in front of retrieval can't fix the extraction gap.
- Other levers we tried did **not** close the gap either: an extraction-grounded system
  prompt and row-level chunking both moved scores only within run-to-run noise. The
  bottleneck is generation, not config — and we confirmed it by **reading all 30 Thai
  answers** rather than trusting the judge alone. The judge is roughly calibrated (on the
  loanshark question it gives credit only to answers that name the legal-rate fact, so low
  scores are genuine misses, not a scoring artifact). The failures fall into two modes:
  (1) *generic-essay drift* — the model writes a plausible Thai essay that omits the
  specific retrieved fact; and (2) *refusal-despite-context* — the model claims
  "ไม่มีข้อมูลเพียงพอ" (insufficient information) though the full table was retrieved
  (typhoon2.5-4b did this and twice replied in **English** asking for more detail). None of
  these config knobs touch either mode.
- Validate on your own Thai questions before rollout — this gap is the thing most likely
  to disappoint Thai-speaking users. The most promising lever in our probes was the **model
  choice itself**, so test candidate models on your real questions rather than relying on
  pipeline tuning.

---

## Features: when to enable

| Agent | Effect in benchmark | Enable when |
|---|---|---|
| `query_rewriter` | Token coverage 0.73 → **0.83**, +~9s | Question wording differs from source; retrieval recall is weak |
| `quality_guard` | +latency, **no** correctness gain; **silently inert in lean mode** | Full pipeline + you need a final safety/format check. Requires `orchestrator_enabled: true` |
| `self_rag` | +latency, no correctness gain here | Multi-hop / ambiguous queries needing retrieve-or-not decisions |
| per-agent tiering | Slower, no gain over full-shared | A specific stage genuinely needs a stronger model (validate first) |

> **Gotcha:** in **lean** mode (orchestrator off) the quality-guard stage never runs —
> toggling `quality_guard_enabled` is a no-op. Turn the orchestrator on to use it.

---

## Full flag reference

Every chat-pipeline flag, its default, and what it costs. Defaults are the
`ChatPipelineConfig` defaults in `crates/thairag-config/src/schema.rs`.

> ### ⚠️ Do not "enable everything"
> The Advanced and Next-Gen RAG features are **additive, default-off, and mostly
> unproven**. Each one adds LLM calls (latency + cost), and several of them **change
> how the answer is written**. Turning them all on at once is the most common cause of
> the *"the server replies with one word instead of a paragraph"* symptom — features
> like `structured_extraction`, `map_reduce`, and `compression` deliberately strip the
> answer down to extracted spans. **Enable one feature at a time and measure.** For a
> reliable baseline, leave the entire Experimental section off (this is the recommended
> default above). In the Admin UI these are marked with an **Experimental** tag.

### Core pipeline (safe to tune)

| Flag | Default | What it does |
|---|---|---|
| `enabled` | `false` | Master switch for the agentic pipeline. Off = plain retrieve-then-answer. |
| `orchestrator_enabled` | `false` | **Lean vs Full.** On = full pipeline (analysis + curation + any enabled agents). |
| `llm_mode` | `chat` | `chat` (agents share the main chat LLM), `shared` (one dedicated agent LLM), or `per-agent`. |
| `query_analyzer_enabled` | `true` | Classifies/normalizes the query. Core agent. |
| `query_rewriter_enabled` | `true` | Reformulates the query for better recall. Best single quality lever when wording differs from source. |
| `context_curator_enabled` | `true` | Trims/orders retrieved context before generation. Core agent. |
| `language_adapter_enabled` | `true` | Aligns response language. Skipped in streaming mode. |
| `quality_guard_enabled` | `false` | Post-generation safety/format check. **Inert in lean mode** — needs `orchestrator_enabled`. |
| `max_context_tokens` | `4096` | Token budget for the context passed to the generator. |
| `agent_max_tokens` | `2048` | Max output tokens per agent LLM call. |
| `max_llm_calls_per_request` | `12` | Hard ceiling on LLM calls per request (1–50). A budget guard, not a feature. |

### Output-shaping flags (these change the answer text)

| Flag | Default | What it does |
|---|---|---|
| `auto_summarize` | `true` | Summarizes long conversation history once it exceeds the threshold. |
| `source_footer_enabled` | `true` | Appends a markdown "Sources" footer. Cheap (no extra LLM call). |
| `structured_citations_enabled` | `true` | Parses `[N]` markers into per-claim attributions. Cheap (no extra LLM call). |
| `structured_extraction_enabled` | `false` | **⚠️ Experimental Thai lever — config-only, not in the UI.** Extract-then-answer: copies the verbatim answer span, then composes using *only* that span. **This is the most likely cause of terse, one-word answers.** The benchmark showed it *hurt* (0.22→0.08). Leave off. |

### Advanced features (off by default — enable selectively)

| Flag | Default | Cost | What it does |
|---|---|---|---|
| `conversation_memory_enabled` | `false` | +1 LLM call | Per-user cross-session conversation summaries. |
| `retrieval_refinement_enabled` | `false` | +retries | Retries search with reformulated queries when recall is weak. |
| `tool_use_enabled` | `false` | +N LLM calls | LLM picks which workspaces/strategies to search (multi-KB reasoning). |
| `adaptive_threshold_enabled` | `false` | low | Adjusts the quality-guard threshold from thumbs up/down feedback. |

### Next-Gen RAG (⚠️ experimental — unproven, costly, can degrade answers)

All default `false`. Each adds LLM calls; several rewrite or shorten the answer. Enable
**one at a time** and measure. These carry an **Experimental** tag in the Admin UI.

| Flag | What it does | Why it can hurt |
|---|---|---|
| `self_rag_enabled` | Decides whether to retrieve at all; skips search for greetings/general-knowledge. | A wrong skip answers from general knowledge instead of your docs. |
| `graph_rag_enabled` | Extracts entities → knowledge graph → traverses relationships at retrieval. | Heavy ingest + query cost; noisy on small corpora. |
| `crag_enabled` | Falls back to web search when local context is weak. Needs a web-search URL. | External dependency; off-corpus answers. |
| `speculative_rag_enabled` | Generates several candidate answers in parallel, ranks, picks best. | Several × the LLM calls per request. |
| `map_reduce_enabled` | Extracts per-chunk (MAP) then synthesizes (REDUCE) for many-doc queries. | Extraction step can produce terse, list-like answers. |
| `ragas_enabled` | Samples responses and scores faithfulness/relevancy for monitoring. | Pure overhead — eval only, no answer benefit. |
| `compression_enabled` | LLMLingua-style: drops low-importance content from context. | Aggressive compression strips facts → shorter/wrong answers. |
| `multimodal_enabled` | Generates text descriptions of embedded images so they're searchable. | Heavy; vision-LLM latency on image-bearing docs. |
| `raptor_enabled` | Builds a tree of recursive summaries over retrieved chunks. | Many extra summary calls; unproven gain. |
| `colbert_enabled` | Fine-grained LLM reranking of top results (late-interaction style). | One LLM call per reranked result. |
| `active_learning_enabled` | Boosts/penalizes chunks from feedback over time. | Slow-acting; needs feedback volume to matter. |
| `context_compaction_enabled` | Summarizes old turns near the context-window limit (like Claude Code). | Summarization can lose earlier detail. |
| `personal_memory_enabled` | Stores/retrieves per-user memories from the vector DB. | Extra retrieval + injection per query. |
| `live_retrieval_enabled` | Fetches from MCP connectors in real time when the KB is empty. | Needs active connectors; network latency. |

### Guardrails (off by default — security/compliance, not quality)

| Flag | Default | What it does |
|---|---|---|
| `input_guardrails_enabled` | `false` | Runs deterministic detectors (Thai ID, phone, email, cards, secrets, prompt-injection) before query analysis. |
| `output_guardrails_enabled` | `false` | Runs the same detectors after generation; can block/redact/regenerate. |

These are deterministic (no LLM cost) and don't affect answer quality — enable them for
PDPA/PII compliance, not for accuracy.

---

## Document processing (`[document]`)

These keys govern the **ingest** pipeline (chunking, PDF/image OCR), not the chat
answer path. Unlike the chat-pipeline flags above, most are **deploy-time** settings
read at startup — set them in `config/default.toml`, a tier config, or via env
(`THAIRAG__DOCUMENT__<KEY>`), and restart. Defaults below are the `DocumentConfig`
defaults in `crates/thairag-config/src/schema.rs`. A few PDF knobs
(`pdf_image_dpi`, `ollama_num_ctx_max`) are also editable from the Admin UI and
hot-reload.

### Chunking

| Key | Default | What it does |
|---|---|---|
| `max_chunk_size` | `512` | Target chunk size (chars). |
| `chunk_overlap` | `64` | Overlap between adjacent chunks (chars). |
| `max_upload_size_mb` | `50` | Reject uploads larger than this. |
| `language_aware_chunking` | `true` | Use Thai-aware sentence/word boundaries when chunking. |
| `table_extraction_enabled` | `true` | Heuristic table extraction from PDF/text content. |
| `chunking_strategy` | `standard` | `standard`, `sentence_window`, or `parent_document` (small-to-big). Switching requires reprocessing existing docs. |
| `sentence_window_size` | `3` | Sentence-window: neighbour sentences each side (≤ 10). |
| `parent_chunk_size` | `2048` | Parent-document: parent chunk size (chars). |
| `child_chunk_size` | `384` | Parent-document: indexed child chunk size (chars). Must be `< parent_chunk_size`. |

### PDF / image OCR (vision LLM + deterministic OCR sidecar)

| Key | Default | What it does |
|---|---|---|
| `image_description_enabled` | `false` | LLM-based description for uploaded images. Requires a vision-capable LLM. |
| `pdf_vision_fallback_enabled` | `true` | Rasterize and OCR (vision LLM) PDF pages with too little extractable text. Requires `image_description_enabled` + a vision-capable LLM. |
| `pdf_min_chars_per_page` | `50` | Per-page char threshold below which a page is treated as "no text" and routed to the OCR fallback. |
| `pdf_max_vision_pages` | `100` | Hard cap on pages a single PDF may rasterize through the vision fallback (abuse guard). |
| `pdf_image_dpi` | `150` | Render DPI for full-page images sent to the vision model. Higher = sharper OCR, more tokens/RAM. *(Admin-UI editable, hot-reload.)* |
| `max_image_edge` | `2048` | Longest-edge pixel cap for **any** image sent to the vision model (PDF renders, embedded DOCX/XLSX/HTML images, direct uploads). Larger images are downscaled. `0` disables. |
| `pdf_page_as_image_threshold` | `0.5` | Image-coverage ratio (0.0–1.0) at/above which a PDF page is rendered whole and OCR'd rather than treated as text+embedded-images. |
| `pdf_min_image_size` | `100` | Skip embedded PDF images smaller than this many pixels on either axis. |
| `pdf_max_images_per_page` | `5` | Cap on embedded images described per mixed PDF page (cost guard). |
| `pdf_high_quality` | `false` | Vision-first OCR for **every** PDF page (highest fidelity, highest cost). |
| `pdf_image_enhance` | `false` | Sharpen/contrast enhancement before OCR (helps Thai diacritics). |
| `pdf_vision_concurrency` | `2` | Max per-page vision/OCR calls in flight at once. `1` = sequential. Keep modest — a heavy model on a shared/flaky gateway 5xxs under too much parallelism. |
| `ocr_sidecar_url` | `""` (empty) | Base URL of the deterministic OCR sidecar (PaddleOCR Thai), e.g. `http://paddleocr:8086`. When set, OCR-needing PDF pages prefer it over the vision LLM (faster, local, no hallucination). Empty = OCR sidecar tier off. |
| `always_preview` | `false` | Admin policy: force the pre-ingest "Preview analysis" gate — the upload UI requires a dry-run preview before a document can be ingested. |

> **`ollama_num_ctx_max`** (default `16384`) is set on the **LLM provider**
> (`[providers.llm]`), not `[document]`. It caps the Ollama KV-cache context window
> so a 128K-capable vision model doesn't pre-allocate a 128K cache for one page —
> a key RAM lever for vision ingest (see `MODEL_SETUP.md`). *(Admin-UI editable.)*

### Deterministic OCR sidecar (PaddleOCR)

The PaddleOCR Thai sidecar (`services/paddleocr-sidecar`) provides a deterministic,
local OCR tier that runs **alongside** the vision LLM: when `ocr_sidecar_url` is set,
OCR-needing PDF pages are transcribed by it in preference to the vision model. Bring it
up via the docker-compose `ocr` profile, then point ThaiRAG at it:

```bash
docker compose --profile ocr up -d paddleocr
# then set on the thairag service (env/.env):
#   THAIRAG__DOCUMENT__OCR_SIDECAR_URL=http://paddleocr:8086
```

The sidecar publishes port `8086`. Use the **internal** compose hostname
(`http://paddleocr:8086`) in `ocr_sidecar_url`, since ThaiRAG reaches it container-to-container.

---

## Grounding / hallucination

Across all 9 configurations, ThaiRAG produced **zero hallucinations**, and the
anti-hallucination probe (a question whose answer deliberately doesn't exist in the
source) scored a perfect 1.00 everywhere. Grounding is robust regardless of which
configuration above you choose — pick based on the correctness/speed trade-off, not on
hallucination risk.

---

## Caveat

These recommendations come from a single-fixture, single-run benchmark (12 questions);
treat score gaps under ~0.10 as noise, and validate against your own corpus and
questions before committing a production default. Full caveats:
[BENCHMARK_RESULTS.md → Caveats](BENCHMARK_RESULTS.md#caveats).
