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
