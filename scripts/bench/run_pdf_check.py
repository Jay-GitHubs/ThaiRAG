#!/usr/bin/env python3
"""End-to-end Thai check on a real PDF (default: the image-only PowerPoint export).

Unlike run_matrix.py / run_thinking_budget_ab.py (which use the deterministic TEXT
fixture), this ingests an arbitrary PDF — including image-only PDFs that go through
vision-OCR at ingest — and asks the same Thai questions against multiple LLMs so we
can read the actual answers. There is no judge/reference scoring here: the point is
to SEE what each model returns on the real document.

Config changes hot-reload via PUT (no rebuild, no DB risk). Original chat-pipeline +
document settings are restored in a finally block; the throwaway org is deleted.

Usage:
  python3 scripts/bench/run_pdf_check.py
  FIXTURE=tests/fixtures/test-from-powerpoint.pdf python3 scripts/bench/run_pdf_check.py
Env (same defaults as run_matrix.py): THAIRAG_API / THAIRAG_EMAIL / THAIRAG_PASSWORD
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
FIXTURE = os.environ.get("FIXTURE", "tests/fixtures/test-from-powerpoint.pdf")

MODELS = ["gemma4:e4b-it-bf16", "qwen3.6:35b"]
QUESTIONS = [
    ("headline-prohibited", "ธุรกิจต้องห้ามมีอะไรบ้าง"),
    ("q06-fireworks",
     "ธุรกิจดอกไม้เพลิงและวัตถุระเบิดควรจัดการอย่างไรเพื่อลดความเสี่ยง?"),
    ("q08-loanshark", "ธุรกิจกู้นอกระบบผิดกฎหมายอย่างไร?"),
]

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


def cfg(model):
    return dict(LEAN, llm_mode="shared", llm={"kind": "Ollama", "model": model},
                **REMOVE_ALL_AGENT_LLMS)


class Api:
    def __init__(self):
        self.s = requests.Session()
        r = self.s.post(f"{API_BASE}/api/auth/login",
                        json={"email": EMAIL, "password": PASSWORD})
        r.raise_for_status()
        self.s.headers["Authorization"] = f"Bearer {r.json()['token']}"

    def get(self, p):
        r = self.s.get(f"{API_BASE}{p}"); r.raise_for_status(); return r.json()

    def put(self, p, d):
        r = self.s.put(f"{API_BASE}{p}", json=d); r.raise_for_status()
        return r.json() if r.text else {}

    def post(self, p, d=None, timeout=600):
        r = self.s.post(f"{API_BASE}{p}", json=d, timeout=timeout)
        r.raise_for_status(); return r.json()

    def delete(self, p):
        self.s.delete(f"{API_BASE}{p}")


def wait_ready(api, ws_id, doc_id, timeout=600):
    deadline = time.time() + timeout
    while time.time() < deadline:
        docs = api.get(f"/api/km/workspaces/{ws_id}/documents")["data"]
        d = next((x for x in docs if x["id"] == doc_id), None)
        if d and d.get("status") != "processing":
            if d.get("status") == "failed":
                raise RuntimeError(f"ingest failed: {d.get('error_message')}")
            return d.get("chunk_count")
        time.sleep(2)
    raise TimeoutError("document ingest timed out")


def main():
    fixture = os.path.join(ROOT, FIXTURE)
    print(f"[pdf] fixture: {fixture}")
    api = Api()
    cp_snap = api.get("/api/km/settings/chat-pipeline")
    doc_cfg = api.get("/api/km/settings/document")
    orig_ai = doc_cfg["ai_preprocessing"]["enabled"]

    suffix = uuid.uuid4().hex[:8]
    org_id = dept_id = ws_id = None
    try:
        org_id = api.post("/api/km/orgs", {"name": f"PdfOrg-{suffix}"})["id"]
        dept_id = api.post(f"/api/km/orgs/{org_id}/depts", {"name": "PdfDept"})["id"]
        ws_id = api.post(f"/api/km/orgs/{org_id}/depts/{dept_id}/workspaces",
                         {"name": "PdfWS"})["id"]

        # AI enrichment off; big chunk. Image-only PDFs still get vision-OCR text
        # (THAIRAG__DOCUMENT__IMAGE_DESCRIPTION_ENABLED is independent of this).
        api.put("/api/km/settings/document", {"ai_preprocessing": {"enabled": False}})
        api.put(f"/api/km/settings/document?scope_type=org&scope_id={org_id}",
                {"max_chunk_size": 8000})

        with open(fixture, "rb") as fh:
            up = api.s.post(
                f"{API_BASE}/api/km/workspaces/{ws_id}/documents/upload",
                files={"file": (os.path.basename(fixture), fh, "application/pdf")},
                timeout=300,
            )
        up.raise_for_status()
        doc_id = up.json()["doc_id"]
        print("[pdf] uploaded; waiting for vision-OCR ingest...")
        chunks = wait_ready(api, ws_id, doc_id)
        print(f"[pdf] ingested: {chunks} chunk(s)\n")

        results = []
        for model in MODELS:
            api.put("/api/km/settings/chat-pipeline", cfg(model))
            time.sleep(3)
            print(f"================ MODEL: {model} ================")
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
                results.append({"model": model, "qid": qid, "len": len(ans),
                                "gen_ms": gen_ms, "wall_ms": wall, "answer": ans})
                print(f"\n  [{qid}] len={len(ans)} gen_ms={gen_ms} wall_ms={wall}")
                print(f"  Q: {qtext}")
                print(f"  A: {ans if ans else '(EMPTY)'}")
            print()

        out = os.path.join(HERE, "results-pdf-check.json")
        json.dump(results, open(out, "w"), ensure_ascii=False, indent=2)
        print(f"[pdf] wrote {out}")
    finally:
        try:
            api.put("/api/km/settings/chat-pipeline", cp_snap)
        except Exception as e:
            print(f"[pdf] WARN restore chat-pipeline: {e}", file=sys.stderr)
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
            print(f"[pdf] WARN restore document: {e}", file=sys.stderr)
        print("[pdf] restored config + cleaned up throwaway org")


if __name__ == "__main__":
    main()
