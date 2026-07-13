# Production Launch Runbook — Self-Hosted Serving for 1,500 Users

Status: **pre-launch**. This runbook operationalizes the July 2026 measurement
campaign (PRs #324–#339). Every number in it is measured or verified against
primary sources — nothing is estimated by feel. Companion doc for general
deployment mechanics: `DEPLOYMENT_GUIDE.md`.

## 1. The sizing verdict (verified)

**One 80 GB GPU is sufficient. 141 GB is unnecessary.**

| fact | value | source |
|---|---|---|
| qwen3.6-27B architecture | hybrid: 64 layers, only **16 full-attention** (interval 4; rest Gated DeltaNet, constant per-seq state) | official `config.json` |
| KV cache | **64 KB/token** bf16 (16 layers × 4 KV heads × 256 head-dim) | computed from config |
| per-request KV ceiling | ≈ 0.95 GB (12.5k-token ceiling); typical ≈ 0.45 GB | measured prompts + code-verified compaction (trigger 20 msgs → summary + last 6) |
| request duration | median 33 s, p90 103 s (gateway-era; self-hosting should improve) | 941-request inference-log sample |
| concurrency @ 1,500 users | ~6 avg / 30–60 peak (Little's law, 10 q/user/day) | measured duration × assumed arrival rate |

Deployment options on one 80 GB card:

- **Option A (preferred): 27B alone, bf16** — 54 GB weights + 26 GB KV ≈ 27
  ceiling / 55 typical requests in flight. VL-7B + embed + rerank go on a
  second small GPU (24 GB is ample: ~15 + 1.2 + 1 GB bf16).
- **Option B: everything on the 80 GB card** — quantize the 27B to FP8
  (~27 GB); total ≈ 44 GB weights, ~36 GB KV. Standard vLLM FP8 serving,
  typically <1 % quality delta.

## 2. Serving stack (vLLM)

```bash
# 27B chat model (Option A: alone on the 80 GB card)
vllm serve Qwen/Qwen3.6-27B \
  --served-model-name qwen3.6-27b-fast \
  --max-model-len 32768 \
  --gpu-memory-utilization 0.95
# Option B adds: --quantization fp8

# Vision model (second GPU or colocated under Option B)
vllm serve Qwen/Qwen2.5-VL-7B-Instruct --served-model-name qwen2.5-vl-7b

# Embeddings + reranker (same host, CPU-adjacent GPU share is fine)
vllm serve Qwen/Qwen3-Embedding-0.6B --served-model-name embed-qwen3 --task embed
# rerank-bge per its model card / TEI if preferred
```

Notes:
- `--max-model-len 32768` comfortably covers the measured 12.5k ceiling with
  margin; the model supports 262k but longer limits only grow the KV
  reservation for no measured benefit.
- Keep the served model names IDENTICAL to the gateway's
  (`qwen3.6-27b-fast`, `qwen2.5-vl-7b`, `embed-qwen3`, `rerank-bge`) so the
  cutover is a pure base-URL change.

## 3. Cutover procedure (gateway → self-hosted)

The entire cutover is a `.env` edit + restart. **Do not change models — only
hosts.** Same `embed-qwen3` weights ⇒ existing Qdrant vectors remain valid
(no re-ingest; the "embedder switch wipes Qdrant" rule applies to changing
MODELS, not hosts).

1. Preflight checklist (§4) fully green.
2. `./scripts/backup-db.sh` (never skip; also snapshot Qdrant storage dir).
3. Verify **no documents are processing** (rebuild/restart kills in-flight
   ingestion; BM25 self-heals empty post-#328 but the doc is lost).
4. Edit `.env`:
   ```
   THAIRAG__PROVIDERS__LLM__BASE_URL=http://<inference-host>:8000/v1
   THAIRAG__PROVIDERS__EMBEDDING__BASE_URL=http://<inference-host>:8001/v1
   THAIRAG__PROVIDERS__RERANKER__BASE_URL=http://<inference-host>:8002/v1
   THAIRAG__PROVIDERS__LLM__API_KEY=<new key or empty per vLLM config>
   ```
   `providers.doc_vision_llm` is a **persisted setting** (Settings →
   Providers), not env: update its base_url there after restart.
5. `docker compose restart thairag`.
6. Smoke gauntlet (all must pass before opening traffic; dry-run rehearsed
   against the gateway 2026-07-12 — expected shapes below are as-observed):
   - `GET /health?deep=true` — every probe `ok`; `redis: not_configured` is
     normal when redis is not enabled, don't fail on it
   - one `test-query` content question (expect an answer with citations)
   - one doc-ops question: `สรุปเอกสารนี้ให้หน่อย` in a single-doc scope.
     First-party SSE events are `{"type":"token","text":…}` (+ `progress`
     and a final `done` carrying `message_id`/`usage`) if scripting this.
   - one document upload → `ready`. Multipart endpoint is
     `POST /api/km/workspaces/{ws}/documents/upload` (the sibling
     `…/documents` POST is JSON-body ingest and 400s on multipart).
     Use a throwaway org/dept/ws and delete the org afterwards (cascades).
   - `python3 scripts/bench/clean_eval.py run --set scripts/bench/table_set.json --ws <TableOnly-ws> --runs 1`
     → expect ≥6/7. A single 6/7 is within measured gateway temp-0
     nondeterminism (3 of 10 baseline runs score 6/7); re-run once and
     investigate only if it stays below 7/7 twice, or below 6/7 ever.
   - verify `GET /api/km/settings/providers` still shows `doc_vision_llm`
     set — the 2026-07-12 dry-run found it silently reverted to unset
     (the measured silent-vision-degradation failure mode); it is a
     persisted setting and can be restored hot via the same endpoint.
7. Re-tune the bulk lane: on OWNED slots, raise
   `THAIRAG_INGEST_MAX_CONCURRENT` from 2 → 4 (vLLM continuous batching
   tolerates more; re-measure chat latency before going higher).

## 4. Preflight checklist

- [ ] `THAIRAG__AUTH__JWT_SECRET` unique per environment; admin bootstrap done
- [ ] `THAIRAG__SERVER__CORS_ORIGINS` locked to real origins
- [ ] `chat_pipeline.citation_base_url` = public HTTPS hostname (deploy-time
      env; must be browser-reachable, not an internal container name)
- [ ] `providers.doc_vision_llm` configured (silently degrades ALL vision
      ingestion if missing — measured failure mode)
- [ ] `THAIRAG_INGEST_MAX_CONCURRENT` set explicitly (2 on shared slots,
      4 to start on owned)
- [ ] `max_message_length` — consider capping to ~6,000 chars to hold the
      12.5k-token request ceiling hard (32,000 permits a 21k-token message)
- [ ] Nightly backup sidecar running (compose `backup` service — 03:30, TCC-proof; verify `grep BACKUP-VERIFY backups/cron.log`); restore drill done once
- [ ] Prometheus/Grafana dashboards reachable; alert on 5xx rate + request
      duration p90
- [ ] E2E: both suites exit 0 against the staged environment
      (`npm run test:e2e` in admin-ui/ and chat-ui/ — certified green as of
      #339; the factory-reset spec only runs with `E2E_FACTORY_RESET=1`)

## 5. Rollback

Revert the `.env` base URLs to the gateway, restart `thairag`, re-run the
smoke gauntlet. No data migration in either direction (vectors/DB untouched
by cutover), so rollback is minutes.

## 6. Post-launch telemetry (week 1)

Run daily:

```bash
python3 scripts/ops/usage_report.py            # q/user/day, durations, concurrency
```

Decisions it feeds:
- **q/user/day** replaces the assumed 10 in the capacity model — recompute
  peak concurrency (Little's law is in the report output).
- **duration p50/p90** on owned hardware — if p50 drops well under 33 s,
  concurrency (and KV pressure) shrinks proportionally.
- **est-vs-actual prompt tokens** — the estimator over-counts single-turn RAG
  (~0.7×) and under-counts multi-turn (context-only metric); capacity math
  must always use ACTUAL `prompt_tokens`.

## 7. Owner actions (not automatable)

1. Procure the 80 GB GPU host (+ optional 24 GB second card for Option A).
2. Pull model weights; stand up vLLM services (§2).
3. Choose Option A/B; execute §3.
