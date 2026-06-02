#!/usr/bin/env python3
"""Thinking-budget A/B for the two Thai questions that came back EMPTY.

Context: after the Qdrant Thai-decode fix, the only cells still lagging were the
big "thinking" models (qwen3.6:35b, qwen3-vl:8b). On q06/q08 they returned a
completely empty answer (judge=0, token=0) at ~21s of generation — the signature
of a reasoner that burned its whole token budget inside <think>...</think> and
emitted no final answer. This script tests the one remaining lever: raise
agent_max_tokens so the model can finish thinking AND still produce an answer.

It mirrors scripts/bench/run_matrix.py exactly (same throwaway org/ws, same
deterministic ingest, same lean-shared cell config) so results are comparable.
Only difference: it runs just q06 + q08 against qwen3.6:35b at two token budgets.

Config changes hot-reload via PUT (no rebuild, no DB risk). The original
chat-pipeline + document settings are restored in a finally block.

Usage:
  python3 scripts/bench/run_thinking_budget_ab.py
Env (same defaults as run_matrix.py):
  THAIRAG_API   (default http://localhost:8080)
  THAIRAG_EMAIL / THAIRAG_PASSWORD (default playwright@test.com / Test1234!)
"""
import json
import os
import sys
import time
import uuid

import requests

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))

API_BASE = os.environ.get("THAIRAG_API", "http://localhost:8080")
EMAIL = os.environ.get("THAIRAG_EMAIL", "playwright@test.com")
PASSWORD = os.environ.get("THAIRAG_PASSWORD", "Test1234!")

MODEL = "qwen3.6:35b"
# The two Thai questions that came back empty in results.json.
QUESTIONS = [
    ("q06-fireworks-caution-th",
     "ธุรกิจดอกไม้เพลิงและวัตถุระเบิดควรจัดการอย่างไรเพื่อลดความเสี่ยง?"),
    ("q08-loanshark-reason-th",
     "ธุรกิจกู้นอกระบบผิดกฎหมายอย่างไร?"),
]

# Same lean preset run_matrix.py uses for the model axis.
LEAN = {
    "query_analyzer_enabled": True,
    "context_curator_enabled": True,
    "language_adapter_enabled": True,
    "orchestrator_enabled": False,
    "quality_guard_enabled": False,
    "query_rewriter_enabled": False,
    "self_rag_enabled": False,
    "request_timeout_secs": 600,
}
REMOVE_ALL_AGENT_LLMS = {
    "remove_query_analyzer_llm": True,
    "remove_query_rewriter_llm": True,
    "remove_context_curator_llm": True,
    "remove_response_generator_llm": True,
    "remove_quality_guard_llm": True,
    "remove_language_adapter_llm": True,
    "remove_orchestrator_llm": True,
}

# The two budgets under test. 2048 = current default (reproduces empty).
BUDGETS = [2048, 8192]


def cell_config(agent_max_tokens):
    return dict(
        LEAN,
        llm_mode="shared",
        llm={"kind": "Ollama", "model": MODEL},
        agent_max_tokens=agent_max_tokens,
        **REMOVE_ALL_AGENT_LLMS,
    )


class Api:
    def __init__(self):
        self.s = requests.Session()
        r = self.s.post(f"{API_BASE}/api/auth/login",
                        json={"email": EMAIL, "password": PASSWORD})
        r.raise_for_status()
        self.s.headers["Authorization"] = f"Bearer {r.json()['token']}"

    def get(self, path):
        r = self.s.get(f"{API_BASE}{path}")
        r.raise_for_status()
        return r.json()

    def put(self, path, data):
        r = self.s.put(f"{API_BASE}{path}", json=data)
        r.raise_for_status()
        return r.json() if r.text else {}

    def post(self, path, data=None, timeout=600):
        r = self.s.post(f"{API_BASE}{path}", json=data, timeout=timeout)
        r.raise_for_status()
        return r.json()

    def delete(self, path):
        self.s.delete(f"{API_BASE}{path}")


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
    eval_set = json.load(open(os.path.join(ROOT, "tests", "eval", "eval_set.json")))
    fixture = os.path.join(ROOT, eval_set["fixture"])
    print(f"[ab] fixture: {fixture}")

    api = Api()
    cp_snap = api.get("/api/km/settings/chat-pipeline")
    doc_cfg = api.get("/api/km/settings/document")
    orig_ai = doc_cfg["ai_preprocessing"]["enabled"]

    suffix = uuid.uuid4().hex[:8]
    org_id = dept_id = ws_id = None
    try:
        org_id = api.post("/api/km/orgs", {"name": f"AbOrg-{suffix}"})["id"]
        dept_id = api.post(f"/api/km/orgs/{org_id}/depts", {"name": "AbDept"})["id"]
        ws_id = api.post(f"/api/km/orgs/{org_id}/depts/{dept_id}/workspaces",
                         {"name": "AbWS"})["id"]

        # Deterministic ingest: AI preprocessing off, big chunk so the table stays atomic.
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
        print(f"[ab] ingested fixture: {chunks} chunk(s)\n")

        results = []
        for budget in BUDGETS:
            api.put("/api/km/settings/chat-pipeline", cell_config(budget))
            time.sleep(3)  # let provider bundle rebuild + model warm
            print(f"=== agent_max_tokens={budget} ===")
            for qid, qtext in QUESTIONS:
                t0 = time.time()
                try:
                    resp = api.post(f"/api/km/workspaces/{ws_id}/test-query",
                                    {"query": qtext})
                except Exception as e:
                    resp = {"answer": "", "timing": {}}
                    print(f"  {qid}: query failed: {e}", file=sys.stderr)
                wall = int((time.time() - t0) * 1000)
                ans = resp.get("answer", "") or ""
                gen_ms = resp.get("timing", {}).get("generation_ms")
                results.append({"budget": budget, "qid": qid, "len": len(ans),
                                "gen_ms": gen_ms, "wall_ms": wall, "answer": ans})
                preview = ans.replace("\n", " ")[:120]
                print(f"  {qid}: len={len(ans):4d} gen_ms={gen_ms} wall_ms={wall}")
                print(f"     {preview!r}")
            print()

        print("=== SUMMARY (answer length by budget) ===")
        for qid, _ in QUESTIONS:
            row = {r["budget"]: r["len"] for r in results if r["qid"] == qid}
            print(f"  {qid:26s} " +
                  "  ".join(f"{b}->{row.get(b)}" for b in BUDGETS))
        out = os.path.join(HERE, "results-thinking-budget-ab.json")
        json.dump(results, open(out, "w"), ensure_ascii=False, indent=2)
        print(f"\n[ab] wrote {out}")
    finally:
        try:
            api.put("/api/km/settings/chat-pipeline", cp_snap)
        except Exception as e:
            print(f"[ab] WARN restore chat-pipeline: {e}", file=sys.stderr)
        if org_id:
            try:
                api.delete(f"/api/km/settings/scoped?scope_type=org&scope_id={org_id}")
            except Exception:
                pass
            if ws_id:
                api.delete(f"/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}")
            if dept_id:
                api.delete(f"/api/km/orgs/{org_id}/depts/{dept_id}")
            api.delete(f"/api/km/orgs/{org_id}")
        try:
            api.put("/api/km/settings/document", {"ai_preprocessing": {"enabled": orig_ai}})
        except Exception as e:
            print(f"[ab] WARN restore document: {e}", file=sys.stderr)
        print("[ab] restored config + cleaned up throwaway org")


if __name__ == "__main__":
    main()
