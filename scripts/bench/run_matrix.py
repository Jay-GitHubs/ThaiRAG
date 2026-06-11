#!/usr/bin/env python3
"""ThaiRAG configuration benchmark harness.

Ingests the prohibited-business fixture into a throwaway workspace, then sweeps a
curated grid of chat-pipeline configurations (one axis at a time: model / mode /
feature). For each config it runs the labeled eval set via the test-query
endpoint, scoring every answer two ways:

  * token score  - fraction of expected_tokens groups present (deterministic)
  * judge score  - an LLM-as-judge (qwen3.6:35b @ temp 0) grades the answer
                   0..1 against the reference answer

Config changes hot-reload via PUT (no rebuild, no DB risk); the original
chat-pipeline config is restored in a finally block. Results -> results.json.

Usage:
  python3 scripts/bench/run_matrix.py            # full run
  python3 scripts/bench/run_matrix.py --quick    # subset of questions + cells
"""
import argparse
import json
import os
import re
import sys
import time
import requests

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))

API_BASE = os.environ.get("THAIRAG_API", "http://localhost:8080")
OLLAMA = os.environ.get("OLLAMA_URL", "http://localhost:11435")
EMAIL = os.environ.get("THAIRAG_EMAIL", "playwright@test.com")
PASSWORD = os.environ.get("THAIRAG_PASSWORD", "Test1234!")
JUDGE_MODEL = os.environ.get("JUDGE_MODEL", "qwen3.6:35b")
EVAL_PATH = os.path.join(ROOT, "tests", "eval", "eval_set.json")
RESULTS_PATH = os.path.join(HERE, "results.json")

# ── chat-pipeline toggle presets ────────────────────────────────────────────
REMOVE_ALL_AGENT_LLMS = {
    "remove_query_analyzer_llm": True,
    "remove_query_rewriter_llm": True,
    "remove_context_curator_llm": True,
    "remove_response_generator_llm": True,
    "remove_quality_guard_llm": True,
    "remove_language_adapter_llm": True,
    "remove_orchestrator_llm": True,
}
LEAN = {
    "query_analyzer_enabled": True,
    "context_curator_enabled": True,
    "language_adapter_enabled": True,
    "orchestrator_enabled": False,
    "quality_guard_enabled": False,
    "query_rewriter_enabled": False,
    "self_rag_enabled": False,
    # Generous per-call timeout so a cold model load (worst seen: ~311s) doesn't 500.
    "request_timeout_secs": 600,
}
# Full = orchestrator ON so the FullPipeline route runs (lean route skips guard).
FULL = dict(LEAN, orchestrator_enabled=True)


def ollama(model):
    return {"kind": "Ollama", "model": model}


def grid(quick):
    """Curated, one-axis-at-a-time around baseline = lean-shared + chinda."""
    CHINDA = "iapp/chinda-qwen3-4b"
    cells = []

    # ── Axis: model (lean shared) ──
    model_axis = [
        ("model/chinda-4b (baseline)", CHINDA),
        ("model/qwen3-vl-8b", "qwen3-vl:8b"),
    ]
    if not quick:
        model_axis += [
            ("model/gemma4-e4b", "gemma4:e4b-it-bf16"),
            ("model/qwen3.6-35b", "qwen3.6:35b"),
        ]
    for name, m in model_axis:
        cells.append({
            "name": name, "axis": "model",
            "config": dict(LEAN, llm_mode="shared", llm=ollama(m), **REMOVE_ALL_AGENT_LLMS),
        })

    # ── Axis: mode (chinda) ──
    cells.append({
        "name": "mode/full-shared", "axis": "mode",
        "config": dict(FULL, llm_mode="shared", llm=ollama(CHINDA), **REMOVE_ALL_AGENT_LLMS),
    })
    if not quick:
        cells.append({
            "name": "mode/per-agent-tiered", "axis": "mode",
            "config": dict(
                FULL, llm_mode="per-agent", llm=ollama(CHINDA),
                **REMOVE_ALL_AGENT_LLMS,
                response_generator_llm=ollama("qwen3.6:35b"),
                quality_guard_llm=ollama("qwen3.6:35b"),
                query_analyzer_llm=ollama(CHINDA),
                context_curator_llm=ollama(CHINDA),
            ),
        })

    # ── Axis: feature (on full-shared chinda, orchestrator ON) ──
    feat_axis = [
        ("feature/+quality_guard", dict(quality_guard_enabled=True)),
        ("feature/+query_rewriter", dict(query_rewriter_enabled=True)),
    ]
    if not quick:
        feat_axis.append(("feature/+self_rag", dict(self_rag_enabled=True)))
    for name, extra in feat_axis:
        cells.append({
            "name": name, "axis": "feature",
            "config": dict(FULL, llm_mode="shared", llm=ollama(CHINDA),
                           **REMOVE_ALL_AGENT_LLMS, **extra),
        })
    return cells


# ── HTTP helpers ────────────────────────────────────────────────────────────
class Api:
    def __init__(self):
        self.s = requests.Session()
        r = self.s.post(f"{API_BASE}/api/auth/login",
                        json={"email": EMAIL, "password": PASSWORD}, timeout=30)
        r.raise_for_status()
        self.s.headers["Authorization"] = f"Bearer {r.json()['token']}"

    def get(self, path):
        r = self.s.get(f"{API_BASE}{path}", timeout=60)
        r.raise_for_status()
        return r.json()

    def put(self, path, data):
        r = self.s.put(f"{API_BASE}{path}", json=data, timeout=120)
        r.raise_for_status()
        return r.json() if r.text else {}

    def post(self, path, data=None, timeout=600):
        r = self.s.post(f"{API_BASE}{path}", json=data, timeout=timeout)
        r.raise_for_status()
        return r.json() if r.text else {}

    def delete(self, path):
        self.s.delete(f"{API_BASE}{path}", timeout=60)


def judge(question, reference, answer):
    """LLM-as-judge: returns (score 0..1 or None, reason)."""
    # Strip the model's own <think> dump and cap length: a 2000-token reasoning
    # answer fed verbatim into a 35b judge is what blew the 600s timeout before.
    answer = re.sub(r"<think>.*?</think>", "", answer, flags=re.DOTALL).strip()
    if len(answer) > 4000:
        answer = answer[:4000] + " …[truncated]"
    prompt = (
        "You are grading a Thai/English RAG system's ANSWER against the REFERENCE "
        "ground truth. Score factual correctness from 0.0 to 1.0. Reward answers "
        "that convey the reference facts (in either Thai or English). Penalize "
        "missing key facts and penalize HALLUCINATED specifics not supported by the "
        "reference. If the reference says a value does not exist and the answer "
        "invents one, score 0.0.\n\n"
        f"QUESTION:\n{question}\n\nREFERENCE:\n{reference}\n\nANSWER:\n{answer}\n\n"
        'Respond with ONLY a JSON object: {"score": <float 0..1>, "reason": "<one short sentence>"}'
    )
    last_err = None
    for attempt in range(3):
        try:
            r = requests.post(f"{OLLAMA}/api/chat", json={
                "model": JUDGE_MODEL,
                "messages": [{"role": "user", "content": prompt}],
                "stream": False,
                "think": False,  # qwen3.6 is a reasoning model; skip its <think> for speed
                "options": {"temperature": 0},
            }, timeout=900)
            r.raise_for_status()
            content = r.json()["message"]["content"]
            content = re.sub(r"<think>.*?</think>", "", content, flags=re.DOTALL)
            m = re.search(r"\{.*\}", content, flags=re.DOTALL)
            if not m:
                return 0.0, f"unparseable judge output: {content[:80]}"
            obj = json.loads(m.group(0))
            return float(obj.get("score", 0.0)), str(obj.get("reason", ""))[:200]
        except Exception as e:
            last_err = e
            print(f"[bench]   judge attempt {attempt+1} failed: {e}", file=sys.stderr)
            time.sleep(5)
    # Degrade gracefully: keep the answer + token score, mark judge as unavailable.
    return None, f"judge failed after 3 attempts: {last_err}"


def token_score(q, answer):
    """Fraction of expected_tokens groups present; flag must_not_contain hits."""
    low = answer.lower()
    groups = q.get("expected_tokens", [])
    hits = 0
    for g in groups:
        if any(alt.strip().lower() in low for alt in g.split("|")):
            hits += 1
    score = hits / len(groups) if groups else 0.0
    halluc = any(bad.lower() in low for bad in q.get("must_not_contain", []))
    return score, halluc


def wait_ready(api, ws_id, doc_id, timeout=180):
    deadline = time.time() + timeout
    while time.time() < deadline:
        docs = api.get(f"/api/km/workspaces/{ws_id}/documents")["data"]
        d = next((x for x in docs if x["id"] == doc_id), None)
        if d and d.get("status") != "processing":
            if d.get("status") == "failed":
                raise RuntimeError(f"ingest failed: {d.get('error_message')}")
            return d.get("chunk_count")
        time.sleep(1)
    raise TimeoutError("document ingest timed out")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="subset of questions + cells")
    args = ap.parse_args()

    eval_set = json.load(open(EVAL_PATH))
    questions = eval_set["questions"]
    if args.quick:
        # representative subset: 2 EN direct, 2 TH direct, aggregation, anti-hallucination
        keep = {"q01-ml-caution", "q03-crypto-regulator", "q02-wildlife-caution-th",
                "q08-loanshark-reason-th", "q10-aggregate-licence", "q11-negative-gambling"}
        questions = [q for q in questions if q["id"] in keep]

    fixtures = [eval_set["fixture"]] + eval_set.get("extra_fixtures", [])
    fixtures = [os.path.join(ROOT, f) for f in fixtures]
    cells = grid(args.quick)
    print(f"[bench] {len(cells)} cells x {len(questions)} questions "
          f"({'quick' if args.quick else 'full'} mode)")

    api = Api()
    suffix = int(time.time())
    org_id = dept_id = ws_id = None
    snap = None
    orig_ai = None
    try:
        # Snapshot chat-pipeline config to restore later.
        cp = api.get("/api/km/settings/chat-pipeline")
        snap = dict(
            llm_mode=cp["llm_mode"],
            query_analyzer_enabled=cp["query_analyzer_enabled"],
            query_rewriter_enabled=cp["query_rewriter_enabled"],
            context_curator_enabled=cp["context_curator_enabled"],
            quality_guard_enabled=cp["quality_guard_enabled"],
            language_adapter_enabled=cp["language_adapter_enabled"],
            orchestrator_enabled=cp["orchestrator_enabled"],
            self_rag_enabled=cp["self_rag_enabled"],
            request_timeout_secs=cp.get("request_timeout_secs", 300),
            **REMOVE_ALL_AGENT_LLMS,
        )
        # llm may be null (llm_mode "chat" = inherit the main chat LLM).
        if cp.get("llm"):
            snap["llm"] = ollama(cp["llm"]["model"])
            snap_temp = cp["llm"].get("temperature")
            if snap_temp is not None:
                snap["llm"]["temperature"] = snap_temp

        # Throwaway org/dept/workspace.
        org_id = api.post("/api/km/orgs", {"name": f"BenchOrg-{suffix}"})["id"]
        dept_id = api.post(f"/api/km/orgs/{org_id}/depts", {"name": "BenchDept"})["id"]
        ws_id = api.post(f"/api/km/orgs/{org_id}/depts/{dept_id}/workspaces",
                         {"name": "BenchWS"})["id"]

        # Deterministic ingest: AI preprocessing off, big chunk so the table stays atomic.
        doc_cfg = api.get("/api/km/settings/document")
        orig_ai = doc_cfg["ai_preprocessing"]["enabled"]
        api.put("/api/km/settings/document", {"ai_preprocessing": {"enabled": False}})
        api.put(f"/api/km/settings/document?scope_type=org&scope_id={org_id}",
                {"max_chunk_size": 8000})

        # Upload fixtures (multipart). All land in ONE workspace so every
        # question is also an implicit multi-document ranking test.
        for fixture in fixtures:
            with open(fixture, "rb") as fh:
                up = api.s.post(
                    f"{API_BASE}/api/km/workspaces/{ws_id}/documents/upload",
                    files={"file": (os.path.basename(fixture), fh, "application/pdf")},
                    timeout=120,
                )
            up.raise_for_status()
            doc_id = up.json()["doc_id"]
            chunks = wait_ready(api, ws_id, doc_id, timeout=600)
            print(f"[bench] ingested {os.path.basename(fixture)}: {chunks} chunk(s)")

        def query(q_text):
            """Resilient test-query: retry transient 500s (model swap/OOM) a few
            times, then degrade to an empty answer rather than aborting the sweep."""
            last_err = None
            for attempt in range(3):
                try:
                    return api.post(f"/api/km/workspaces/{ws_id}/test-query",
                                    {"query": q_text})
                except Exception as e:
                    last_err = e
                    print(f"[bench]   query attempt {attempt+1} failed: {e}", file=sys.stderr)
                    time.sleep(10)  # give Ollama time to finish loading the model
            print(f"[bench]   query gave up: {last_err}", file=sys.stderr)
            return {"answer": "", "timing": {}}

        # ── Phase 1: collect answers per cell ──
        rows = []
        for ci, cell in enumerate(cells, 1):
            api.put("/api/km/settings/chat-pipeline", cell["config"])
            time.sleep(3)  # let provider bundle rebuild + model warm/swap
            for q in questions:
                t0 = time.time()
                resp = query(q["question"])
                wall = int((time.time() - t0) * 1000)
                ans = resp.get("answer", "")
                tok, halluc = token_score(q, ans)
                rows.append({
                    "cell": cell["name"], "axis": cell["axis"], "qid": q["id"],
                    "lang": q["lang"], "type": q["type"], "answer": ans,
                    "question": q["question"], "reference": q["reference_answer"],
                    "token_score": tok, "hallucinated": halluc,
                    "total_ms": resp.get("timing", {}).get("total_ms", wall),
                    "gen_ms": resp.get("timing", {}).get("generation_ms"),
                    "search_ms": resp.get("timing", {}).get("search_ms"),
                })
            print(f"[bench] ({ci}/{len(cells)}) {cell['name']}: answered {len(questions)}")

        meta = {"quick": args.quick, "judge_model": JUDGE_MODEL,
                "n_cells": len(cells), "n_questions": len(questions)}

        def save():
            json.dump({"meta": meta, "rows": rows}, open(RESULTS_PATH, "w"),
                      ensure_ascii=False, indent=2)

        # Persist answers + token scores BEFORE judging so a judge hiccup can't
        # discard an hour of answer collection.
        save()
        print(f"[bench] wrote answers to {RESULTS_PATH} (pre-judge)")

        # ── Phase 2: judge every answer (35b stays warm) ──
        print(f"[bench] judging {len(rows)} answers with {JUDGE_MODEL} ...")
        # Warm up the judge model once so the first real call doesn't eat a load.
        try:
            judge("warmup", "warmup", "warmup")
        except Exception:
            pass
        for i, row in enumerate(rows, 1):
            score, reason = judge(row["question"], row["reference"], row["answer"])
            row["judge_score"] = score
            row["judge_reason"] = reason
            if i % 5 == 0:
                save()  # incremental: crash keeps everything judged so far
                print(f"[bench]   judged {i}/{len(rows)}")
        save()
        print(f"[bench] wrote {RESULTS_PATH}")
    finally:
        # Restore config + clean up throwaway data.
        if snap is not None:
            try:
                api.put("/api/km/settings/chat-pipeline", snap)
            except Exception as e:
                print(f"[bench] WARN restore chat-pipeline: {e}", file=sys.stderr)
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
                print(f"[bench] WARN restore document cfg: {e}", file=sys.stderr)
        print("[bench] restored config + cleaned up")


if __name__ == "__main__":
    main()
