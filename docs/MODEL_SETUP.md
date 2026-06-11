# Model Selection & Setup Guide

Bench-backed model choices for running ThaiRAG on local open-source models
(Ollama), and the exact configuration to reproduce them on a new machine.

Measured 2026-06-11 with `scripts/bench/run_matrix.py` (12 bilingual questions
against the prohibited-business table fixture; LLM-judge = `qwen3.6:35b` @
temp 0; results in `scripts/bench/results-model-selection.json`).

## TL;DR

| Machine RAM | Answer model | Judge score | Notes |
|---|---|---|---|
| ≥ 32 GB | **`gemma4:12b-it-bf16`** (24 GB) | **0.942** (Thai 1.000) | Best overall; vision-capable |
| 16–24 GB | **`gemma4:e4b-it-bf16`** (16 GB) | 0.908 (Thai 0.960) | ~2× faster (3.9 s vs 7.6 s avg); vision-capable |

Pipeline mode: **lean shared** (orchestrator off, analyzer + curator +
language-adapter on) — the per-agent tiered mode scored *lower* than plain
gemma4-12b (0.921 vs 0.942) while adding latency and configuration surface.

## Full bench results

| Model (lean shared) | Judge | Thai-only | Token recall | Blank | Halluc. | Avg ms |
|---|---|---|---|---|---|---|
| gemma4:12b-it-bf16 | **0.942** | **1.000** | **1.000** | 0 | 0 | 7,574 |
| qwen3.5:9b-bf16 | 0.938 | 0.920 | 0.979 | 0 | 0 | 9,849 |
| tiered (gemma12 responder) | 0.921 | 0.960 | 0.979 | 0 | 0 | 6,621 |
| gemma4:e4b-it-bf16 | 0.908 | 0.960 | 0.958 | 0 | 0 | 3,900 |
| qwen3:14b | 0.883 | 0.960 | 0.958 | 0 | 0 | 5,326 |
| qwen3.6:35b-a3b-q8_0 (MoE) | 0.842 | 0.960 | 0.917 | 0 | 0 | 4,593 |
| qwen3.6:35b (dense Q4, earlier run) | 0.858 | 0.800 | — | 0 | 0 | — |
| qwen3-vl:8b (earlier run) | 0.817 | 0.760 | — | 0 | 0 | — |
| iapp/chinda-qwen3-4b (earlier run) | 0.658 | 0.520 | — | 0 | 0 | — |

## Models to pull on a new machine

```bash
# Answer LLM (pick one per the RAM table above)
ollama pull gemma4:12b-it-bf16        # or: gemma4:e4b-it-bf16

# Embeddings — REQUIRED, indexing 404s without it
ollama pull qwen3-embedding:0.6b

# Optional: bench judge / quality-guard LLM
ollama pull qwen3.6:35b
```

Both gemma4 variants report `vision` capability in Ollama, so the same model
serves chat answers, embedded-image description, and the scanned-PDF OCR tier —
no separate vision model needed.

## Configuration

### Ollama (macOS host, Docker stack)

The stack reaches native Ollama via `host.docker.internal:11435`. After macOS
updates Ollama can rebind to `127.0.0.1` — start it with:

```bash
export OLLAMA_HOST=0.0.0.0:11435 && ollama serve
```

### ThaiRAG providers (deploy-time, config/env)

```toml
[providers.llm]
kind = "ollama"
model = "gemma4:12b-it-bf16"
base_url = "http://host.docker.internal:11435"
ollama_num_ctx_max = 16384      # KV-cache ceiling — keep; prevents RAM blowups

[providers.embedding]
kind = "ollama"
model = "qwen3-embedding:0.6b"
```

(or env: `THAIRAG__PROVIDERS__LLM__MODEL=gemma4:12b-it-bf16` etc.)

### Chat pipeline (hot-reload via admin UI or settings API)

```json
{
  "llm_mode": "shared",
  "llm": { "kind": "Ollama", "model": "gemma4:12b-it-bf16" },
  "query_analyzer_enabled": true,
  "context_curator_enabled": true,
  "language_adapter_enabled": true,
  "orchestrator_enabled": false,
  "quality_guard_enabled": false,
  "query_rewriter_enabled": false,
  "self_rag_enabled": false,
  "request_timeout_secs": 600
}
```

`PUT /api/km/settings/chat-pipeline` applies it without a restart.

## Known pitfalls (hard-won — do not relearn)

- **Never deploy SCB10X Typhoon models** without a deterministic brand-denylist
  guard: `typhoon2.5-qwen3-4b` scored 0.00 on the bench, and brand-bias
  hallucination cannot be prompt-guaranteed away (project rule: can't guarantee
  → drop).
- **`iapp/chinda-qwen3-4b` is too weak as the answer model** (0.658; Thai 0.52).
  It remains acceptable as a cheap *agent* LLM (analyzer/curator) in tiered
  mode, but tiered mode itself is not worth it vs plain gemma4-12b.
- **The MoE `qwen3.6:35b-a3b` underperforms its dense sibling** on this
  workload (0.842) — total parameter count is not the lever here.
- **Thinking models can emit blank answers**: an all-`<think>` response
  collapses to empty after tag stripping (seen intermittently on gemma4-e4b in
  older runs; 0 blanks in the current bench, but watch the chat logs after any
  model swap).
- **RAM levers for PDF/vision ingestion**: `ollama_num_ctx_max` (default
  16384) and `document.pdf_image_dpi` (default 150) — both admin-UI editable.
  A 128K-context vision model with an uncapped KV cache once took the host
  from 6 → 52 GB.

## Re-running the bench

```bash
# stack up, models pulled, then:
python3 scripts/bench/run_matrix.py            # full grid, ~30-40 min
python3 scripts/bench/run_matrix.py --quick    # smoke subset
```

The harness ingests its own throwaway workspace, sweeps configs via the
hot-reload settings API, restores the original config afterwards, and writes
`scripts/bench/results.json`.
