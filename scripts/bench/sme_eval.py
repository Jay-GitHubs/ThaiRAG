#!/usr/bin/env python3
"""Per-document Thai QA evaluation over a live workspace + HTML dashboard.

For every READY document in the target workspace:
  1. pull its chunk text (the OCR'd/converted content that retrieval sees),
  2. have a local LLM generate N grounded Thai Q/A pairs (JSON-schema forced),
  3. ask each question through the real RAG endpoint (test-query, multi-doc),
  4. judge each answer 0..1 against the reference with a stronger LLM,
  5. write results JSON + a self-contained dark dashboard (no CDN, offline).

Usage:
  python3 scripts/bench/sme_eval.py --ws <workspace_id> [--per-doc 20]
                                    [--docs 14] [--out scripts/bench/sme_eval]
"""
import argparse
import json
import os
import re
import sys
import time

import requests

API = os.environ.get("THAIRAG_API", "http://localhost:8080")
OLLAMA = os.environ.get("OLLAMA_URL", "http://localhost:11435")
EMAIL = os.environ.get("THAIRAG_EMAIL", "playwright@test.com")
PASSWORD = os.environ.get("THAIRAG_PASSWORD", "Test1234!")
QGEN_MODEL = os.environ.get("QGEN_MODEL", "qwen3.6:35b")
JUDGE_MODEL = os.environ.get("JUDGE_MODEL", "qwen3.6:35b")

QA_SCHEMA = {
    "type": "object",
    "properties": {
        "questions": {
            "type": "array",
            "minItems": 1,
            "items": {
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "reference_answer": {"type": "string"},
                },
                "required": ["question", "reference_answer"],
            },
        }
    },
    "required": ["questions"],
}


def login():
    s = requests.Session()
    r = s.post(f"{API}/api/auth/login", json={"email": EMAIL, "password": PASSWORD}, timeout=30)
    r.raise_for_status()
    s.headers["Authorization"] = f"Bearer {r.json()['token']}"
    return s


def ollama_chat(model, prompt, schema=None, timeout=900):
    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "think": False,
        "options": {"temperature": 0.2},
    }
    if schema is not None:
        body["format"] = schema
    r = requests.post(f"{OLLAMA}/api/chat", json=body, timeout=timeout)
    r.raise_for_status()
    content = r.json()["message"]["content"]
    return re.sub(r"<think>.*?</think>", "", content, flags=re.DOTALL).strip()


def doc_text(s, ws, doc_id, max_chars=12000):
    r = s.get(f"{API}/api/km/workspaces/{ws}/documents/{doc_id}/chunks", timeout=60)
    r.raise_for_status()
    data = r.json()
    items = data if isinstance(data, list) else data.get("chunks") or data.get("data") or []
    text = "\n\n".join(c.get("text") or c.get("content") or "" for c in items)
    return text[:max_chars], len(items)


def gen_questions(title, text, n):
    prompt = (
        f"คุณเป็นผู้สร้างชุดข้อสอบสำหรับประเมินระบบ RAG ภาษาไทย\n"
        f"เอกสาร: {title}\n\nเนื้อหา:\n{text}\n\n"
        f"จงสร้างคำถามภาษาไทย {n} ข้อ พร้อมคำตอบอ้างอิง โดยมีเงื่อนไข:\n"
        f"- ทุกคำถามต้องตอบได้จากเนื้อหาข้างต้นเท่านั้น และคำตอบอ้างอิงต้องถูกต้องตรงตามเนื้อหา\n"
        f"- เน้นข้อเท็จจริงเชิงตาราง: วงเงิน อัตราดอกเบี้ย ระยะเวลา หลักประกัน คุณสมบัติผู้กู้ เงื่อนไข ค่าธรรมเนียม\n"
        f"- คำถามต้องระบุชื่อโครงการให้ชัดเจน (มีหลายเอกสารในคลังเดียวกัน)\n"
        f"- คำตอบอ้างอิงสั้น กระชับ มีตัวเลข/เงื่อนไขครบ\n"
        f'ตอบเป็น JSON เท่านั้น: {{"questions":[{{"question":"...","reference_answer":"..."}}]}}'
    )
    last = None
    for attempt in range(3):
        try:
            out = ollama_chat(QGEN_MODEL, prompt, schema=QA_SCHEMA)
            qs = json.loads(out)["questions"][:n]
            return [q for q in qs if q.get("question") and q.get("reference_answer")]
        except (json.JSONDecodeError, KeyError) as e:
            last = e
            print(f"[eval]   qgen retry {attempt + 1}: {e}", file=sys.stderr)
    raise RuntimeError(f"qgen failed after retries: {last}")


def judge(question, reference, answer):
    answer = re.sub(r"<think>.*?</think>", "", answer, flags=re.DOTALL).strip()[:4000]
    prompt = (
        "You are grading a Thai RAG system's ANSWER against the REFERENCE ground truth. "
        "Score factual correctness 0.0-1.0. Penalize missing key facts and hallucinated "
        "specifics. If the answer refuses while the reference exists, score 0.0.\n\n"
        f"QUESTION:\n{question}\n\nREFERENCE:\n{reference}\n\nANSWER:\n{answer}\n\n"
        'Respond ONLY JSON: {"score": <float>, "reason": "<short>"}'
    )
    for attempt in range(3):
        try:
            out = ollama_chat(JUDGE_MODEL, prompt, schema={
                "type": "object",
                "properties": {"score": {"type": "number"}, "reason": {"type": "string"}},
                "required": ["score", "reason"],
            })
            obj = json.loads(out)
            return max(0.0, min(1.0, float(obj["score"]))), str(obj.get("reason", ""))[:200]
        except Exception as e:  # noqa: BLE001
            print(f"[eval]   judge retry {attempt + 1}: {e}", file=sys.stderr)
            time.sleep(5)
    return None, "judge unavailable"


def ask(s, ws, q):
    t0 = time.time()
    for attempt in range(3):
        try:
            r = s.post(f"{API}/api/km/workspaces/{ws}/test-query", json={"query": q}, timeout=600)
            r.raise_for_status()
            d = r.json()
            return d.get("answer", ""), int((time.time() - t0) * 1000)
        except Exception as e:  # noqa: BLE001
            print(f"[eval]   query retry {attempt + 1}: {e}", file=sys.stderr)
            time.sleep(10)
    return "", int((time.time() - t0) * 1000)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ws", required=True)
    ap.add_argument("--per-doc", type=int, default=20)
    ap.add_argument("--docs", type=int, default=0, help="limit docs (0 = all)")
    ap.add_argument("--out", default="scripts/bench/sme_eval")
    args = ap.parse_args()

    s = login()
    r = s.get(f"{API}/api/km/workspaces/{args.ws}/documents", timeout=60)
    r.raise_for_status()
    docs = [d for d in r.json()["data"] if d.get("status") == "ready"]
    docs.sort(key=lambda d: d["title"])
    if args.docs:
        docs = docs[: args.docs]
    print(f"[eval] {len(docs)} ready docs × {args.per_doc} questions")

    results = []
    for di, d in enumerate(docs, 1):
        title = d["title"]
        text, n_chunks = doc_text(s, args.ws, d["id"])
        thai = sum(1 for ch in text if "ก" <= ch <= "๛")
        if thai < 200:
            print(f"[eval] ({di}/{len(docs)}) SKIP no-text doc: {title[:50]}")
            results.append({"doc": title, "doc_id": d["id"], "skipped": "no_text", "rows": []})
            continue
        print(f"[eval] ({di}/{len(docs)}) {title[:50]} — generating {args.per_doc} questions")
        try:
            qs = gen_questions(title, text, args.per_doc)
        except Exception as e:  # noqa: BLE001
            print(f"[eval]   QA generation failed: {e}", file=sys.stderr)
            results.append({"doc": title, "doc_id": d["id"], "skipped": f"qgen: {e}", "rows": []})
            continue
        rows = []
        for qi, q in enumerate(qs, 1):
            ans, ms = ask(s, args.ws, q["question"])
            score, reason = judge(q["question"], q["reference_answer"], ans)
            rows.append({
                "question": q["question"],
                "reference": q["reference_answer"],
                "answer": ans,
                "judge_score": score,
                "judge_reason": reason,
                "ms": ms,
            })
            print(f"[eval]   q{qi}/{len(qs)} score={score} {ms}ms")
            # checkpoint
            json.dump({"results": results + [{"doc": title, "doc_id": d["id"], "rows": rows}]},
                      open(args.out + ".partial.json", "w"), ensure_ascii=False)
        results.append({"doc": title, "doc_id": d["id"], "rows": rows})

    payload = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "workspace": args.ws,
        "per_doc": args.per_doc,
        "qgen_model": QGEN_MODEL,
        "judge_model": JUDGE_MODEL,
        "results": results,
    }
    json.dump(payload, open(args.out + ".json", "w"), ensure_ascii=False, indent=1)
    print(f"[eval] wrote {args.out}.json")
    build_dashboard(payload, args.out + ".html")
    print(f"[eval] wrote {args.out}.html")


def build_dashboard(payload, path):
    here = os.path.dirname(os.path.abspath(__file__))
    tpl = open(os.path.join(here, "sme_dashboard_template.html"), encoding="utf-8").read()
    html = tpl.replace("/*__DATA__*/", "const DATA = " + json.dumps(payload, ensure_ascii=False) + ";")
    open(path, "w", encoding="utf-8").write(html)


if __name__ == "__main__":
    main()
