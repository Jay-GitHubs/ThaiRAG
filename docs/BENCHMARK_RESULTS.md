# Benchmark Results — Chat Pipeline Configurations

> **Scope & date:** configuration micro-benchmark (~2026-06-01: one fixture,
> one chunk, 12 questions) measuring chat-pipeline *config sensitivity* on
> local Ollama models. It is **not** a production-corpus benchmark. Current
> production-corpus numbers (2026-07, all-gateway deployment, clean_eval
> harness): born-digital Thai tables **100%** (isolated scope, 10-run),
> prose/non-table **98–100%**, scanned-twin corpora **92.9%** (PaddleOCR
> tier), shared near-clone scope caps ~75% vs **87.5%** single-product-scope
> (see CLAUDE.md near-clone guidance). Retrieval modes (2026-07-12): vector
> 95.7–98% vs vectorless 97.1% tables / 88.0% prose. Doc-selection flag
> (`doc_selection_enabled`) A/B: **zero lift, zero harm** at these ceilings
> (tables 97.1/97.1, prose 100/100) — stays default-off.

This document reports a controlled benchmark of ThaiRAG chat-pipeline configurations,
measuring **answer correctness** and **latency** across models, pipeline modes, and
optional agent features. The goal is an evidence base for the recommendations in
[CONFIGURATION_GUIDE.md](CONFIGURATION_GUIDE.md).

> **TL;DR**
> - **`gemma4:e4b-it-bf16` (lean shared)** gave the best correctness (token 0.83, judge 0.55)
>   while staying mid-pack on speed (p50 27s) — and was the strongest on Thai.
> - The current default **`chinda-qwen3-4b` (lean shared)** is the **fastest** (p50 16.5s)
>   but the **weakest** on correctness (judge 0.35), especially on Thai (0.12).
> - Turning **the full pipeline on** lifted the small chinda model a lot
>   (judge 0.35 → 0.52) for ~4s more latency.
> - **`+query_rewriter`** raised token coverage (0.73 → 0.83) — best when retrieval recall matters.
> - **`quality_guard`, `self_rag`, and per-agent tiering** added latency with **no correctness gain** in this test.
> - **Zero hallucinations** in all 108 answers; the anti-hallucination probe scored 1.00 in every config.
> - **Thai answers lag English badly** across every model — the single most important finding.

---

## Methodology

- **Fixture:** `tests/fixtures/micro_sme_prohibited_business.pdf` — a 12-row bilingual
  (Thai/English) "prohibited business" table. Ingested AI-preprocessing **off**,
  `max_chunk_size=8000` so the table stays a single atomic chunk (1 chunk total).
- **Eval set:** `tests/eval/eval_set.json` — 12 labeled questions (6 Thai, 6 English)
  spanning direct lookup, regulator precision, reasoning, aggregation, and one
  anti-hallucination probe (asks for a maximum loan figure that does not exist).
- **Scoring (two independent measures):**
  - **Token score** — deterministic. Fraction of `expected_tokens` groups present in
    the answer (case-insensitive substring; a group matches if *any* alternate appears).
  - **Judge score** — `qwen3.6:35b` @ temp 0 grades each answer 0.0–1.0 against the
    reference answer. Penalizes missing facts and hallucinated specifics.
  - **Hallucination flag** — set if the answer contains any `must_not_contain` token
    (e.g. an invented baht figure on the gambling question).
- **Procedure:** each config is applied via `PUT /api/km/settings/chat-pipeline`
  (hot-reload, no restart), all 12 questions are asked via the `test-query` endpoint,
  then all 108 answers are judged in one pass. The original config is snapshotted and
  restored afterward. Harness: `scripts/bench/run_matrix.py`.
- **Latency:** `total_ms` reported per query (wall time end-to-end through the pipeline).
- **Grid:** curated, **one axis at a time** around the baseline (lean shared + chinda),
  not a full cross-product. 9 configs × 12 questions = 108 answers.

---

## Results

Token and judge scores are means over 12 questions (0.0–1.0, higher is better).
p50 / p95 are median / 95th-percentile end-to-end latency in milliseconds.

| Config | Axis | Token | Judge | p50 (ms) | p95 (ms) | Halluc. |
|---|---|---:|---:|---:|---:|---:|
| `model/chinda-4b` (baseline) | model | 0.62 | 0.35 | 16,528 | 33,213 | 0 |
| `model/qwen3-vl-8b` | model | 0.58 | 0.48 | 24,601 | 84,092 | 0 |
| `model/gemma4-e4b` | model | **0.83** | **0.55** | 27,134 | 44,914 | 0 |
| `model/qwen3.6-35b` | model | 0.79 | 0.54 | 35,876 | 52,804 | 0 |
| `mode/full-shared` | mode | 0.73 | 0.52 | 20,491 | 27,253 | 0 |
| `mode/per-agent-tiered` | mode | 0.71 | 0.48 | 42,958 | 59,237 | 0 |
| `feature/+quality_guard` | feature | 0.71 | 0.42 | 28,505 | 31,352 | 0 |
| `feature/+query_rewriter` | feature | **0.83** | 0.51 | 29,712 | 32,228 | 0 |
| `feature/+self_rag` | feature | 0.67 | 0.45 | 24,717 | 28,694 | 0 |

**Baselines for the axes:**
- *model* and *feature* cells use **`iapp/chinda-qwen3-4b`** unless the model name says otherwise.
- *model* cells run in **lean shared** mode (orchestrator off).
- *mode* `full-shared` and all *feature* cells run the **full pipeline** (orchestrator on).
  `feature/*` cells are `mode/full-shared` plus one extra agent toggle.
- `mode/per-agent-tiered` uses chinda for light stages and **`qwen3.6:35b`** for the
  response generator + quality guard.

### Thai vs. English (judge score)

The headline finding. Every model answers English questions well but Thai questions poorly.

| Config | Thai | English |
|---|---:|---:|
| `model/chinda-4b` (baseline) | 0.12 | 0.51 |
| `model/qwen3-vl-8b` | 0.04 | 0.79 |
| `model/gemma4-e4b` | **0.22** | 0.79 |
| `model/qwen3.6-35b` | 0.20 | 0.79 |
| `mode/full-shared` | 0.18 | 0.76 |
| `mode/per-agent-tiered` | **0.24** | 0.64 |
| `feature/+quality_guard` | 0.14 | 0.61 |
| `feature/+query_rewriter` | 0.18 | 0.74 |
| `feature/+self_rag` | 0.08 | 0.71 |

English correctness saturates around 0.76–0.79 for the better configs; Thai stays
in the 0.1–0.24 band. Inspection of the Thai answers shows the models tend to produce
long, *generic* advice essays instead of **extracting the specific table facts**
(e.g. on "protected-wildlife trade" they discuss conservation in general rather than
citing CITES, certificate-of-origin, and export permit). This is a model-behavior /
prompting issue, not a retrieval failure — the correct chunk was always supplied.

### Anti-hallucination probe (q11)

The gambling question asks for a maximum loan amount that the table deliberately does
not provide. **All 9 configs answered correctly** (declined / "not specified"),
token 1.0, judge 1.00, zero invented figures. ThaiRAG's grounding holds across every
configuration tested.

---

## Reading the axes

**Model (lean shared).** `gemma4:e4b-it-bf16` is the correctness winner on both metrics
*and* the strongest on Thai, while being faster than the 35b. `chinda-4b` is fastest but
weakest. `qwen3-vl-8b` has a long latency tail (p95 84s) — a vision model is a poor fit
for text-only RAG here.

**Mode.** Switching the same chinda model from lean to the **full pipeline** raised judge
0.35 → 0.52 and token 0.62 → 0.73 for only ~4s more median latency — the orchestrated
query-analysis + context-curation path clearly helps a small model. **Per-agent tiering**
(35b for response/guard) did *not* beat full-shared and roughly doubled latency.

**Features (on full-shared chinda).** `+query_rewriter` pushed token coverage to 0.83
(best retrieval coverage in the grid) for ~9s. `quality_guard` and `self_rag` added
latency without improving correctness in this test.

---

## Caveats

These results are directional, not definitive. Treat score differences **smaller than
~0.10 as noise**.

- **Single fixture, single chunk, 12 questions.** One bilingual table; results may not
  generalize to large multi-chunk corpora or other domains.
- **Single run, no repeats.** Latency and judge scores are not averaged over multiple
  trials, so per-cell variance (especially cold model loads) is uncharacterized.
- **Judge family overlap.** The judge is `qwen3.6:35b`, the same family as one candidate
  model — a potential mild bias in that model's favor. (It did not top the rankings, so
  the effect appears small here.)
- **Latency is hardware- and warmth-dependent.** Measured on the dev host with native
  Ollama; absolute milliseconds will differ on other GPUs and with different
  `ollama_keep_alive` / model-resident state. The cold-load tail is real (one query hit
  the 300s timeout during harness development — the bench now uses a 600s per-call timeout).
- **Token score rewards surface tokens.** It is a recall proxy, not a correctness measure;
  always read it alongside the judge score.

---

## Reproducing

Prerequisites: the Docker stack up (`thairag` on :8080), native Ollama on :11435 with the
candidate models pulled, and the judge model `qwen3.6:35b` available.

```bash
# Full sweep (9 configs × 12 questions, ~1–2h dominated by the 35b cells)
python3 -u scripts/bench/run_matrix.py

# Fast smoke test (5 configs × 6 questions)
python3 -u scripts/bench/run_matrix.py --quick
```

Output is written to `scripts/bench/results.json` (every answer with token score,
judge score, judge reason, and timing). The harness snapshots and restores the live
chat-pipeline config and deletes its throwaway workspace in a `finally` block, so it is
safe to run against a working instance. Override `THAIRAG_API`, `OLLAMA_URL`,
`JUDGE_MODEL`, `THAIRAG_EMAIL`, `THAIRAG_PASSWORD` via environment variables if needed.
