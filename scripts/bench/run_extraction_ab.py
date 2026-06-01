#!/usr/bin/env python3
"""Structured-extraction A/B on the 5 Thai eval questions.

Single-run A/B: ingests the fixture into a throwaway workspace under the lean
baseline (chinda-4b shared), then for each Thai question runs the query twice —
structured_extraction_enabled = False (baseline) then True (treatment) — in the
SAME run so run-to-run model noise cancels. Both answers are judged by the same
LLM-judge (qwen3.6:35b @ temp 0). Config restored + data cleaned in finally.

Rationale: the historical Thai baseline (~0.24) is noisy run-to-run (gemma swung
0.00/0.22/0.28/0.32), so a same-run paired comparison is more trustworthy than
comparing the treatment to a stored number. Treat per-question swings under ~0.10
as noise; look at the aggregate paired delta.

Usage:
  python3 scripts/bench/run_extraction_ab.py
"""
import json
import os
import sys
import time

# Reuse the matrix harness's infra.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run_matrix import (  # noqa: E402
    API_BASE, ROOT, EVAL_PATH, Api, judge, token_score, wait_ready,
    ollama, LEAN, REMOVE_ALL_AGENT_LLMS,
)

BASELINE_MODEL = os.environ.get("AB_MODEL", "iapp/chinda-qwen3-4b")
RESULTS_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                            "extraction_ab_results.json")


def base_config():
    """Lean shared config on the baseline model, agent LLMs stripped."""
    return dict(LEAN, llm_mode="shared", llm=ollama(BASELINE_MODEL),
                **REMOVE_ALL_AGENT_LLMS)


def main():
    eval_set = json.load(open(EVAL_PATH))
    thai_qs = [q for q in eval_set["questions"] if q["lang"] == "th"]
    fixture = os.path.join(ROOT, eval_set["fixture"])
    print(f"[ab] {len(thai_qs)} Thai questions, model={BASELINE_MODEL}")

    api = Api()
    suffix = int(time.time())
    org_id = dept_id = ws_id = None
    snap = None
    orig_ai = None
    try:
        # Snapshot chat-pipeline to restore later (incl. the extraction flag).
        cp = api.get("/api/km/settings/chat-pipeline")
        snap = dict(
            llm_mode=cp["llm_mode"], llm=ollama(cp["llm"]["model"]),
            query_analyzer_enabled=cp["query_analyzer_enabled"],
            query_rewriter_enabled=cp["query_rewriter_enabled"],
            context_curator_enabled=cp["context_curator_enabled"],
            quality_guard_enabled=cp["quality_guard_enabled"],
            language_adapter_enabled=cp["language_adapter_enabled"],
            orchestrator_enabled=cp["orchestrator_enabled"],
            self_rag_enabled=cp["self_rag_enabled"],
            structured_extraction_enabled=cp.get("structured_extraction_enabled", False),
            request_timeout_secs=cp.get("request_timeout_secs", 300),
            **REMOVE_ALL_AGENT_LLMS,
        )

        # Throwaway org/dept/workspace.
        org_id = api.post("/api/km/orgs", {"name": f"ABOrg-{suffix}"})["id"]
        dept_id = api.post(f"/api/km/orgs/{org_id}/depts", {"name": "ABDept"})["id"]
        ws_id = api.post(f"/api/km/orgs/{org_id}/depts/{dept_id}/workspaces",
                         {"name": "ABWS"})["id"]

        # Deterministic ingest: AI preprocessing off, big chunk -> table stays atomic.
        doc_cfg = api.get("/api/km/settings/document")
        orig_ai = doc_cfg["ai_preprocessing"]["enabled"]
        api.put("/api/km/settings/document", {"ai_preprocessing": {"enabled": False}})
        api.put(f"/api/km/settings/document?scope_type=org&scope_id={org_id}",
                {"max_chunk_size": 8000})

        with open(fixture, "rb") as fh:
            up = api.s.post(
                f"{API_BASE}/api/km/workspaces/{ws_id}/documents/upload",
                files={"file": (os.path.basename(fixture), fh, "application/pdf")},
                timeout=120,
            )
        up.raise_for_status()
        doc_id = up.json()["doc_id"]
        chunks = wait_ready(api, ws_id, doc_id)
        print(f"[ab] ingested fixture: {chunks} chunk(s)")

        def query(q_text):
            last_err = None
            for attempt in range(3):
                try:
                    return api.post(f"/api/km/workspaces/{ws_id}/test-query",
                                    {"query": q_text})
                except Exception as e:
                    last_err = e
                    print(f"[ab]   query attempt {attempt+1} failed: {e}", file=sys.stderr)
                    time.sleep(10)
            print(f"[ab]   query gave up: {last_err}", file=sys.stderr)
            return {"answer": "", "timing": {}}

        rows = []
        for arm, flag in [("baseline", False), ("extraction", True)]:
            api.put("/api/km/settings/chat-pipeline",
                    dict(base_config(), structured_extraction_enabled=flag))
            time.sleep(3)  # provider bundle rebuild + model warm
            for q in thai_qs:
                t0 = time.time()
                resp = query(q["question"])
                wall = int((time.time() - t0) * 1000)
                ans = resp.get("answer", "")
                tok, halluc = token_score(q, ans)
                rows.append({
                    "arm": arm, "extraction_enabled": flag, "qid": q["id"],
                    "question": q["question"], "reference": q["reference_answer"],
                    "answer": ans, "token_score": tok, "hallucinated": halluc,
                    "total_ms": resp.get("timing", {}).get("total_ms", wall),
                })
            print(f"[ab] arm={arm} (extraction={flag}): answered {len(thai_qs)}")

        # Judge every answer (35b stays warm).
        print(f"[ab] judging {len(rows)} answers ...")
        try:
            judge("warmup", "warmup", "warmup")
        except Exception:
            pass
        for i, row in enumerate(rows, 1):
            score, reason = judge(row["question"], row["reference"], row["answer"])
            row["judge_score"] = score
            row["judge_reason"] = reason
            print(f"[ab]   judged {i}/{len(rows)} ({row['arm']}/{row['qid']}): {score}")

        json.dump({"meta": {"model": BASELINE_MODEL, "n_questions": len(thai_qs)},
                   "rows": rows}, open(RESULTS_PATH, "w"), ensure_ascii=False, indent=2)
        print(f"[ab] wrote {RESULTS_PATH}")

        # ── Paired summary ──
        by = {(r["arm"], r["qid"]): r for r in rows}
        print("\n=== Structured-extraction A/B (Thai, judge score) ===")
        print(f"{'qid':<28} {'baseline':>9} {'extract':>9} {'delta':>7}")
        b_tot = e_tot = 0.0
        n = 0
        for q in thai_qs:
            b = by[("baseline", q["id"])].get("judge_score")
            e = by[("extraction", q["id"])].get("judge_score")
            bs = 0.0 if b is None else b
            es = 0.0 if e is None else e
            b_tot += bs
            e_tot += es
            n += 1
            print(f"{q['id']:<28} {bs:>9.2f} {es:>9.2f} {es - bs:>+7.2f}")
        print("-" * 56)
        print(f"{'MEAN':<28} {b_tot/n:>9.2f} {e_tot/n:>9.2f} {(e_tot - b_tot)/n:>+7.2f}")
        print("\n(treat per-question deltas under ~0.10 as noise)")
    finally:
        if snap is not None:
            try:
                api.put("/api/km/settings/chat-pipeline", snap)
            except Exception as e:
                print(f"[ab] WARN restore chat-pipeline: {e}", file=sys.stderr)
        if org_id:
            api.delete(f"/api/km/settings/scoped?scope_type=org&scope_id={org_id}")
            if ws_id:
                api.delete(f"/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}")
            if dept_id:
                api.delete(f"/api/km/orgs/{org_id}/depts/{dept_id}")
            api.delete(f"/api/km/orgs/{org_id}")
        if orig_ai is not None:
            try:
                api.put("/api/km/settings/document", {"ai_preprocessing": {"enabled": orig_ai}})
            except Exception as e:
                print(f"[ab] WARN restore document cfg: {e}", file=sys.stderr)
        print("[ab] restored config + cleaned up")


if __name__ == "__main__":
    main()
