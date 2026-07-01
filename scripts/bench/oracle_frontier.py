#!/usr/bin/env python3
"""Frontier-model oracle test for the Thai-TABLE accuracy bottleneck.

The 2026-06-19 investigation concluded the residual table-cell gap is an LLM
table-reasoning limit that no free local model breaks (ceiling ~45-55%), and
named "a frontier model for table queries" as the only remaining unlock — then
ruled it out under the free-model constraint.

The stack now runs on an OpenAI-compatible gateway that also serves qwen-235b.
This script re-opens that question deterministically: ingest the 2 Thai tax-table
PDFs (deterministic extraction, AI-preprocessing OFF), then feed the WHOLE doc +
each canonical table-cell question to several gateway models and score with the
same numeric-aware matcher. Whole-doc context isolates table READING from
retrieval, so any model-to-model delta is pure reasoning.

Run (gateway key comes from the running container):
  GW_KEY=$(docker exec thairag-thairag-1 printenv THAIRAG__PROVIDERS__LLM__API_KEY) \
  python3 scripts/bench/oracle_frontier.py
"""
import os
import re
import sys
import time
import uuid

import requests

from clean_eval import API, login, token_score

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))

GW_BASE = os.environ.get("GW_BASE", "https://llm.jay-tech-ai.com/v1")
GW_KEY = os.environ.get("GW_KEY", "")
MODELS = os.environ.get("MODELS", "qwen3.6-27b-fast,qwen3.6-27b,qwen-235b").split(",")
SET = os.environ.get("SET", os.path.join(HERE, "table_set.json"))
PDFS = [
    "tests/fixtures/thai-real/rd_tp4_table.pdf",
    "tests/fixtures/thai-real/rd_withholding_table.pdf",
]

ANSWER_PROMPT = (
    "ตอบคำถามจากเอกสารด้านล่างเท่านั้น ตอบสั้น กระชับ ตรงประเด็น "
    "ถ้าเป็นตัวเลขให้ระบุพร้อมหน่วยตามที่ปรากฏในเอกสาร\n\n"
    "เอกสาร:\n{doc}\n\nคำถาม: {q}\nคำตอบ:"
)


def gw_chat(model, prompt, timeout=900, retries=8):
    """Return (answer, ok). ok=False = unrecoverable transient (503/524/timeout/
    empty after retries) — excluded from accuracy, not scored 0. JSON is decoded
    from raw bytes as UTF-8 (the gateway omits charset, so requests' latin-1 text
    fallback would mojibake Thai)."""
    import json as _json

    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0,
        "stream": False,
    }
    for attempt in range(retries):
        try:
            r = requests.post(
                f"{GW_BASE}/chat/completions",
                headers={"Authorization": f"Bearer {GW_KEY}"},
                json=body,
                timeout=timeout,
            )
            if r.status_code != 200:
                raise RuntimeError(f"HTTP {r.status_code}")
            c = _json.loads(r.content)["choices"][0]["message"]["content"]
            c = re.sub(r"<think>.*?</think>", "", c, flags=re.DOTALL).strip()
            if c:
                return c, True
            raise RuntimeError("empty content")
        except Exception as e:  # noqa: BLE001
            print(f"    gw_chat retry {attempt+1}/{retries} ({model}): {e}", file=sys.stderr)
            time.sleep(min(3 + attempt * 3, 20))
    return "", False


def chunks_text(s, ws, doc_id):
    r = s.get(f"{API}/api/km/workspaces/{ws}/documents/{doc_id}/chunks", timeout=60)
    r.raise_for_status()
    d = r.json()
    items = d if isinstance(d, list) else d.get("chunks") or d.get("data") or []
    return " ".join((c.get("text") or c.get("content") or "") for c in items)


def wait_ready(s, ws, doc_id, timeout=600):
    deadline = time.time() + timeout
    while time.time() < deadline:
        docs = s.get(f"{API}/api/km/workspaces/{ws}/documents", timeout=60).json()["data"]
        d = next((x for x in docs if x["id"] == doc_id), None)
        if d and d.get("status") != "processing":
            if d.get("status") == "failed":
                raise RuntimeError(f"ingest failed: {d.get('error_message')}")
            return d.get("chunk_count")
        time.sleep(2)
    raise TimeoutError("ingest timed out")


def main():
    if not GW_KEY:
        print("GW_KEY is required (gateway api key).", file=sys.stderr)
        return 2
    import json

    qset = json.load(open(SET))
    questions = qset.get("questions") or qset.get("items") or qset

    s = login()
    suffix = uuid.uuid4().hex[:8]
    org_id = None
    try:
        org_id = s.post(f"{API}/api/km/orgs", json={"name": f"OracleOrg-{suffix}"}).json()["id"]
        dept_id = s.post(
            f"{API}/api/km/orgs/{org_id}/depts", json={"name": "D"}
        ).json()["id"]
        ws_id = s.post(
            f"{API}/api/km/orgs/{org_id}/depts/{dept_id}/workspaces", json={"name": "W"}
        ).json()["id"]

        # Deterministic extraction: AI-preprocessing off, one big chunk per doc.
        s.put(f"{API}/api/km/settings/document", json={"ai_preprocessing": {"enabled": False}})
        s.put(
            f"{API}/api/km/settings/document?scope_type=org&scope_id={org_id}",
            json={"max_chunk_size": 12000},
        )

        ctx_by_file = {}
        for rel in PDFS:
            path = os.path.join(ROOT, rel)
            base = os.path.basename(path)
            with open(path, "rb") as fh:
                up = s.post(
                    f"{API}/api/km/workspaces/{ws_id}/documents/upload",
                    files={"file": (base, fh, "application/pdf")},
                    timeout=300,
                )
            up.raise_for_status()
            doc_id = up.json()["doc_id"]
            n = wait_ready(s, ws_id, doc_id)
            ctx = chunks_text(s, ws_id, doc_id)
            ctx_by_file[base] = ctx
            print(f"[ingest] {base}: {n} chunk(s), {len(ctx)} chars", flush=True)

        print()
        summary = {}
        for model in MODELS:
            # Warm up (load the model) so the first scored call isn't a cold 503.
            print(f"[warm] {model} ...", flush=True)
            gw_chat(model, "ตอบว่า: พร้อม", timeout=300, retries=10)
            hits = 0.0
            scored = 0
            transient = 0
            misses = []
            for it in questions:
                ctx = ctx_by_file.get(it["doc"], "")
                ans, ok = gw_chat(model, ANSWER_PROMPT.format(doc=ctx[:28000], q=it["question"]))
                if not ok:
                    transient += 1
                    print(f"  TRANSIENT (excluded): {it['question'][:50]}")
                    continue
                scored += 1
                sc = token_score(it["expected_tokens"], ans)
                hits += sc
                if sc < 1.0:
                    misses.append((it["question"], it["expected_tokens"], ans[:140]))
            summary[model] = (hits, scored, transient)
            pct = 100 * hits / scored if scored else 0.0
            print(f"=== {model}: {pct:.1f}%  ({hits:.1f}/{scored} scored, {transient} transient) ===")
            for q, exp, ans in misses:
                print(f"  MISS Q: {q[:64]}")
                print(f"       want={exp}  got={ans!r}")
            print(flush=True)

        print("==== SUMMARY (whole-doc oracle, temp 0) ====")
        for m, (h, sc, tr) in summary.items():
            pct = 100 * h / sc if sc else 0.0
            print(f"  {m:22s} {pct:5.1f}%  ({h:.1f}/{sc} scored, {tr} transient)")
    finally:
        if org_id:
            try:
                s.delete(f"{API}/api/km/orgs/{org_id}")
            except Exception as e:  # noqa: BLE001
                print(f"WARN cleanup org: {e}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
