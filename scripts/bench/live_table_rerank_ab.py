#!/usr/bin/env python3
"""Live-pipeline A/B for the gateway reranker on the Thai table set.

The oracle test (oracle_frontier.py) puts the whole doc in context, so it
isolates LLM reading and is blind to retrieval. This runs the REAL pipeline
(test-query → retrieve + RRF + rerank + answer) on the same 5 table-cell
questions, once with the reranker OFF (passthrough) and once with the gateway
cross-encoder (jina/rerank-bge), so we can see whether reranking changes which
chunk reaches the model — and the live accuracy vs the 60% oracle ceiling.

Run:
  GW_KEY=$(docker exec thairag-thairag-1 printenv THAIRAG__PROVIDERS__LLM__API_KEY) \
  python3 scripts/bench/live_table_rerank_ab.py
"""
import json
import os
import sys
import time
import uuid

from clean_eval import API, login, token_score

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
GW_KEY = os.environ.get("GW_KEY", "")
SET = os.environ.get("SET", os.path.join(HERE, "table_set.json"))
PDFS = [
    "tests/fixtures/thai-real/rd_tp4_table.pdf",
    "tests/fixtures/thai-real/rd_withholding_table.pdf",
]

PASSTHROUGH = {"kind": "passthrough", "model": "", "base_url": "", "normalize_scores": False}
JINA = {
    "kind": "jina",
    "model": "rerank-bge",
    "base_url": "https://llm.jay-tech-ai.com/v1",
    "api_key": GW_KEY,
    "normalize_scores": True,
}


def wait_ready(s, ws, doc_id, timeout=600):
    deadline = time.time() + timeout
    while time.time() < deadline:
        docs = s.get(f"{API}/api/km/workspaces/{ws}/documents", timeout=60).json()["data"]
        d = next((x for x in docs if x["id"] == doc_id), None)
        if d and d.get("status") != "processing":
            if d.get("status") == "failed":
                raise RuntimeError(f"ingest failed: {d.get('error_message')}")
            return
        time.sleep(2)
    raise TimeoutError("ingest timed out")


# Lean pipeline: one generation call per query. The full pipeline stacks
# orchestrator + generator + quality_guard = 100s+/query on the gateway, which is
# slow but irrelevant to a reranker A/B — we only care which chunk reaches the
# model. Keep analyzer/curator (they shape retrieval); drop the rest.
LEAN = {
    "query_analyzer_enabled": True,
    "context_curator_enabled": True,
    "language_adapter_enabled": False,
    "orchestrator_enabled": False,
    "quality_guard_enabled": False,
    "query_rewriter_enabled": False,
    "self_rag_enabled": False,
    "request_timeout_secs": 600,
}


def ask(s, ws, q, retries=2):
    # Generation over the large table context runs ~45-70s on the gateway, so
    # allow 240s/attempt (vs the old 120s that truncated every answer to empty).
    for _ in range(retries):
        try:
            r = s.post(f"{API}/api/km/workspaces/{ws}/test-query", json={"query": q}, timeout=240)
            if r.status_code == 200:
                j = r.json()
                ans = j.get("answer", "") or ""
                if ans:
                    return ans
        except Exception:  # noqa: BLE001
            pass
        time.sleep(4)
    return ""


def main():
    if not GW_KEY:
        print("GW_KEY required", file=sys.stderr)
        return 2
    questions = json.load(open(SET))["questions"]
    s = login()

    cp_snap = s.get(f"{API}/api/km/settings/chat-pipeline").json()
    suffix = uuid.uuid4().hex[:8]
    org_id = None
    try:
        org_id = s.post(f"{API}/api/km/orgs", json={"name": f"RerankAB-{suffix}"}).json()["id"]
        dept_id = s.post(f"{API}/api/km/orgs/{org_id}/depts", json={"name": "D"}).json()["id"]
        ws_id = s.post(
            f"{API}/api/km/orgs/{org_id}/depts/{dept_id}/workspaces", json={"name": "W"}
        ).json()["id"]
        s.put(f"{API}/api/km/settings/document", json={"ai_preprocessing": {"enabled": False}})
        s.put(f"{API}/api/km/settings/chat-pipeline", json=LEAN)

        for rel in PDFS:
            path = os.path.join(ROOT, rel)
            with open(path, "rb") as fh:
                up = s.post(
                    f"{API}/api/km/workspaces/{ws_id}/documents/upload",
                    files={"file": (os.path.basename(path), fh, "application/pdf")},
                    timeout=300,
                )
            up.raise_for_status()
            wait_ready(s, ws_id, up.json()["doc_id"])
            print(f"[ingest] {os.path.basename(path)} ready", flush=True)

        results = {}
        for name, rcfg in [("passthrough", PASSTHROUGH), ("jina/rerank-bge", JINA)]:
            s.put(f"{API}/api/km/settings/providers", json={"reranker": rcfg})
            time.sleep(3)
            hits = 0.0
            misses = []
            for it in questions:
                ans = ask(s, ws_id, it["question"])
                sc = token_score(it["expected_tokens"], ans)
                hits += sc
                if sc < 1.0:
                    misses.append((it["question"], it["expected_tokens"], ans[:120]))
            results[name] = (hits, len(questions))
            print(f"\n=== reranker={name}: {100*hits/len(questions):.1f}% ({hits:.1f}/{len(questions)}) ===")
            for q, exp, ans in misses:
                print(f"  MISS {q[:50]} want={exp} got={ans!r}")

        print("\n==== LIVE PIPELINE SUMMARY (qwen3.6-27b-fast) ====")
        for name, (h, n) in results.items():
            print(f"  {name:18s} {100*h/n:5.1f}% ({h:.1f}/{n})")
    finally:
        # Leave the stack on the gateway reranker (the #2 fix), with the key;
        # restore the original chat-pipeline config.
        try:
            s.put(f"{API}/api/km/settings/providers", json={"reranker": JINA})
            # Restore only the toggles LEAN flipped, to their original values.
            restore = {k: cp_snap.get(k) for k in LEAN if k in cp_snap}
            s.put(f"{API}/api/km/settings/chat-pipeline", json=restore)
        except Exception as e:  # noqa: BLE001
            print(f"WARN restore config: {e}", file=sys.stderr)
        if org_id:
            s.delete(f"{API}/api/km/orgs/{org_id}")


if __name__ == "__main__":
    sys.exit(main())
