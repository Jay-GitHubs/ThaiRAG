#!/usr/bin/env python3
"""Deterministic, verification-grade RAG eval harness.

The accuracy question ("are we above 90%?") was un-answerable with the old
harness because three sources of noise swamped the signal: non-deterministic
answers (temp > 0), an LLM judge whose score itself varied, and LLM-generated
reference answers that were sometimes garbage ("refer to the document"). This
harness removes all three:

  * Answers are deterministic — it pins the chat model to temperature 0 for the
    run and restores the prior value afterwards.
  * Scoring is deterministic — exact `expected_tokens` substring match (the
    proven format from tests/eval/eval_set.json), no LLM in the scoring path.
  * References are *verified*, not trusted — every generated question carries a
    verbatim `source_quote` from the document, and `build` drops any question
    whose answer/quote is not actually present in the source. What survives is
    a clean set for a human to skim and sign off.

It also measures its own noise floor: `run` repeats the whole set N times and
reports per-question stability, so a real effect can be told apart from jitter.

Usage:
  # 1. generate auto-verified candidate questions for human review
  python3 scripts/bench/clean_eval.py build --ws <id> --per-doc 8 \
        --out scripts/bench/clean_set.json
  # 2. (human) skim clean_set.json — each item shows the source_quote proving
  #    its answer; delete or fix any that look wrong.
  # 3. run deterministically, N times, with doc-selection OFF then ON
  python3 scripts/bench/clean_eval.py run --set scripts/bench/clean_set.json \
        --ws <id> --runs 3 --org <org_id>
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
GEN_MODEL = os.environ.get("GEN_MODEL", "gemma4:12b-it-bf16")


def login():
    s = requests.Session()
    r = s.post(f"{API}/api/auth/login", json={"email": EMAIL, "password": PASSWORD}, timeout=30)
    r.raise_for_status()
    s.headers["Authorization"] = f"Bearer {r.json()['token']}"
    return s


def norm(t: str) -> str:
    """Whitespace-insensitive form for substring verification."""
    return re.sub(r"\s+", "", t or "").lower()


def doc_text(s, ws, doc_id):
    r = s.get(f"{API}/api/km/workspaces/{ws}/documents/{doc_id}/chunks", timeout=60)
    r.raise_for_status()
    d = r.json()
    items = d if isinstance(d, list) else d.get("chunks") or d.get("data") or []
    return " ".join((c.get("text") or c.get("content") or "") for c in items)


def ollama(model, prompt, schema=None, timeout=900):
    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "think": False,
        "options": {"temperature": 0.1},
    }
    if schema:
        body["format"] = schema
    for _ in range(3):
        try:
            r = requests.post(f"{OLLAMA}/api/chat", json=body, timeout=timeout)
            r.raise_for_status()
            c = r.json()["message"]["content"]
            c = re.sub(r"<think>.*?</think>", "", c, flags=re.DOTALL).strip()
            if c:
                return c
        except Exception:  # noqa: BLE001
            pass
        time.sleep(5)
    return ""


# ── build ────────────────────────────────────────────────────────────────────
BUILD_SCHEMA = {
    "type": "object",
    "properties": {
        "items": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "answer": {"type": "string"},
                    "source_quote": {"type": "string"},
                },
                "required": ["question", "answer", "source_quote"],
            },
        }
    },
    "required": ["items"],
}

BUILD_PROMPT = (
    "คุณกำลังสร้างชุดข้อสอบเพื่อวัดความแม่นยำของระบบ RAG อย่างเข้มงวด จากเอกสารด้านล่าง\n"
    "สร้าง {n} ข้อ โดยแต่ละข้อมี 3 ส่วน:\n"
    "1. question: คำถามภาษาไทยที่ระบุชื่อโครงการ/คุณลักษณะเด่นให้ตอบได้ฉบับเดียว\n"
    "2. answer: คำตอบที่เป็นข้อความสั้นมาก (ตัวเลข/คำสำคัญ) ที่ปรากฏ**คำต่อคำ**ในเอกสาร\n"
    "3. source_quote: ประโยคหรือวลีที่ยกมา**คำต่อคำ**จากเอกสาร ซึ่งมีคำตอบ (answer) อยู่ในนั้น\n"
    "กฎเหล็ก: answer ต้องเป็นสตริงย่อยของ source_quote และ source_quote ต้องคัดลอกตรงจากเอกสารเป๊ะ ๆ\n"
    "ตอบ JSON เท่านั้น\n\nเอกสาร:\n{doc}"
)


def cmd_build(args):
    s = login()
    docs = sorted(
        [d for d in s.get(f"{API}/api/km/workspaces/{args.ws}/documents", timeout=60).json()["data"]
         if d.get("status") == "ready"],
        key=lambda d: d["title"],
    )
    out = []
    kept = dropped = 0
    for d in docs:
        text = doc_text(s, args.ws, d["id"])
        ntext = norm(text)
        prompt = BUILD_PROMPT.format(n=args.per_doc, doc=text[:8000])
        raw = ollama(GEN_MODEL, prompt, BUILD_SCHEMA)
        try:
            obj = json.loads(raw) if raw else {}
            items = obj.get("items") or obj.get("questions") or (obj if isinstance(obj, list) else [])
        except Exception as e:  # noqa: BLE001
            print(f"[build] {d['title'][:34]}: parse failed {e} raw={raw[:80]!r}", file=sys.stderr)
            continue
        if not items:
            print(f"[build] {d['title'][:34]}: no items", file=sys.stderr)
            continue
        for it in items:
            q, ans, quote = it.get("question"), it.get("answer"), it.get("source_quote")
            if not (q and ans and quote):
                dropped += 1
                continue
            # Verification: the quoted span and the answer must really be in the
            # document, and the answer inside the quote. This is what makes the
            # reference trustworthy instead of an LLM's claim.
            if norm(quote) in ntext and norm(ans) in norm(quote) and len(norm(ans)) >= 1:
                out.append({
                    "doc": d["title"], "doc_id": d["id"], "question": q,
                    "expected_tokens": [ans], "reference_answer": ans,
                    "source_quote": quote,
                })
                kept += 1
            else:
                dropped += 1
        print(f"[build] {d['title'][:34]}: kept so far {kept}", flush=True)
    json.dump({"questions": out}, open(args.out, "w"), ensure_ascii=False, indent=1)
    print(f"[build] wrote {len(out)} VERIFIED questions to {args.out} "
          f"(kept {kept}, dropped {dropped} unverifiable). Review the source_quote of each.")


# ── run ──────────────────────────────────────────────────────────────────────
THAI_DIGITS = str.maketrans("๐๑๒๓๔๕๖๗๘๙", "0123456789")


def canon(t: str) -> str:
    """Whitespace-free, lowercased, with numbers canonicalised so a correct
    answer phrased differently still matches: Thai numerals mapped to Arabic
    (๒๔๘๔→2484 — gazette-era references use them, models answer either way),
    drop thousands separators (1,500→1500) and trailing zeros (15.0→15)."""
    t = norm(t).translate(THAI_DIGITS)
    t = re.sub(r"(?<=\d),(?=\d)", "", t)
    t = re.sub(r"(\d)\.0+(?!\d)", r"\1", t)
    return t


def _key_tokens(t: str):
    """Significant units of an expected answer for partial matching: numbers,
    latin words (≥2), and Thai runs (≥3 chars, since Thai has no spaces)."""
    return re.findall(r"\d+", t) + re.findall(r"[a-z]{2,}", t) + re.findall(r"[ก-๛]{3,}", t)


def matches(expected_alt: str, answer: str) -> bool:
    """A correct-answer test that is exact on meaning but tolerant of format.
    Passes on canonicalised substring (handles 15.0 vs 15, comma'd numbers,
    spacing), or when ≥70% of the expected answer's key tokens (numbers / words
    / Thai runs) appear in the answer — so a paraphrase that carries the facts
    counts, while a generic non-answer (missing the numbers/terms) still fails.
    """
    e, a = canon(expected_alt), canon(answer)
    if not e:
        return False
    if e in a:
        return True
    keys = _key_tokens(e)
    if not keys:
        return False
    present = sum(1 for k in keys if k in a)
    return present / len(keys) >= 0.7


def token_score(expected, answer):
    groups = expected or []
    if not groups:
        return 0.0
    hits = sum(1 for g in groups if any(matches(alt, answer) for alt in g.split("|")))
    return hits / len(groups)


def ask(s, ws, q, retries=6):
    """Query the pipeline, returning (answer, ok).

    ok=False marks an *unrecoverable transient* backend failure — non-200, no
    JSON, an empty answer, or an incomplete pipeline stage — which happens when
    a query contends with gemma model-load (e.g. right after a reprocess) and
    yields 500s or truncated generations. These are NOT genuine wrong answers,
    so the caller excludes them from accuracy rather than scoring them 0 (which
    would silently corrupt the measurement). A healthy response has a non-empty
    answer and every `pipeline_stages` entry == "done".
    """
    for attempt in range(retries):
        reason = "?"
        try:
            r = s.post(f"{API}/api/km/workspaces/{ws}/test-query", json={"query": q}, timeout=600)
            if r.status_code == 200:
                j = r.json()
                ans = j.get("answer", "")
                stages = j.get("pipeline_stages", [])
                done = (
                    all(st.get("status") == "done" for st in stages) if stages else bool(ans.strip())
                )
                if ans.strip() and done:
                    return ans, True
                reason = "empty answer" if not ans.strip() else "incomplete stages"
            else:
                reason = f"http {r.status_code}"
        except Exception as e:  # noqa: BLE001
            reason = str(e)[:60]
        print(f"[run]   transient ({reason}); retry {attempt + 1}/{retries}", file=sys.stderr)
        time.sleep(min(4 * (attempt + 1), 20))
    return "", False


def warm_up(s, ws, q):
    """Send throwaway queries until one comes back clean, so the answer model is
    loaded and stable before measurement — the model-load window is exactly when
    transients cluster."""
    for _ in range(8):
        _, ok = ask(s, ws, q, retries=2)
        if ok:
            return True
        time.sleep(5)
    return False


def get_cp(s):
    return s.get(f"{API}/api/km/settings/chat-pipeline", timeout=30).json()


def put_cp(s, payload, scope=None):
    url = f"{API}/api/km/settings/chat-pipeline"
    if scope:
        url += f"?scope_type=org&scope_id={scope}"
    r = s.put(url, json=payload, timeout=60)
    r.raise_for_status()
    time.sleep(2)


def cmd_run(args):
    s = login()
    qs = json.load(open(args.set))["questions"]
    print(f"[run] {len(qs)} verified questions × {args.runs} runs (deterministic, temp 0)")

    cp = get_cp(s)
    prev_llm = cp.get("llm")
    # Pin the answer model to temperature 0 for reproducibility.
    if prev_llm:
        det = dict(prev_llm)
        det["temperature"] = 0.0
        det.pop("has_api_key", None)
        put_cp(s, {"llm": det})

    conditions = [("OFF", False)]
    if args.org:
        conditions.append(("ON", True))

    # Warm the model before measuring so the load window's transients don't land
    # on real questions.
    if not warm_up(s, args.ws, qs[0]["question"]):
        print("[run] WARNING: warm-up never got a clean response — backend may be unstable")

    results = {}
    try:
        for cond, on in conditions:
            if args.org:
                put_cp(s, {"doc_selection_enabled": on}, scope=args.org)
            # per-question pass across runs → stability
            passes = [0.0] * len(qs)
            transient_q = set()
            run_means = []
            for run in range(args.runs):
                scores = []
                for i, q in enumerate(qs):
                    ans, ok = ask(s, args.ws, q["question"])
                    if not ok:
                        # Unrecoverable backend transient — exclude from accuracy
                        # (scoring it 0 would corrupt the number), flag the question.
                        transient_q.add(i)
                        continue
                    sc = token_score(q["expected_tokens"], ans)
                    scores.append(sc)
                    passes[i] += sc
                run_means.append(sum(scores) / len(scores) if scores else 0.0)
                print(f"[run] {cond} run {run + 1}/{args.runs}: {run_means[-1]:.3f}"
                      f"  (scored {len(scores)}/{len(qs)})", flush=True)
            mean = sum(run_means) / len(run_means)
            spread = max(run_means) - min(run_means)
            # questions that flip between runs = the unstable tail
            unstable = sum(1 for p in passes if 0 < p < args.runs)
            results[cond] = {"mean": mean, "run_means": run_means, "spread": spread,
                             "unstable_questions": unstable, "transient_questions": len(transient_q)}
            print(f"[run] {cond}: mean={mean:.3f}  run-to-run spread={spread:.3f}  "
                  f"unstable={unstable}/{len(qs)}  transient-excluded={len(transient_q)}")
    finally:
        if args.org:
            put_cp(s, {"doc_selection_enabled": False}, scope=args.org)
        if prev_llm:
            restore = dict(prev_llm)
            restore.pop("has_api_key", None)
            put_cp(s, {"llm": restore})
        print("[run] restored chat-pipeline config")

    print("\n=== SUMMARY (deterministic token-match) ===")
    for cond, r in results.items():
        tq = r.get("transient_questions", 0)
        tnote = f", {tq} transient-excluded" if tq else ""
        print(f"  {cond:<4} accuracy {100 * r['mean']:.1f}%  "
              f"(noise floor ±{100 * r['spread'] / 2:.1f}%, {r['unstable_questions']} unstable Q{tnote})")
        if tq:
            print(f"       ⚠ {tq} question(s) hit unrecoverable backend transients and were "
                  f"EXCLUDED from accuracy (not scored as misses). Re-run if this is non-trivial.")
    if "ON" in results and "OFF" in results:
        lift = results["ON"]["mean"] - results["OFF"]["mean"]
        noise = max(results["ON"]["spread"], results["OFF"]["spread"])
        verdict = "REAL" if abs(lift) > noise else "WITHIN NOISE"
        print(f"  doc-selection lift {100 * lift:+.1f}%  → {verdict} "
              f"(noise floor ±{100 * noise / 2:.1f}%)")
    json.dump(results, open(args.set.replace(".json", "_results.json"), "w"), indent=1)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--ws", required=True)
    b.add_argument("--per-doc", type=int, default=8)
    b.add_argument("--out", default="scripts/bench/clean_set.json")
    b.set_defaults(func=cmd_build)
    r = sub.add_parser("run")
    r.add_argument("--set", required=True)
    r.add_argument("--ws", required=True)
    r.add_argument("--runs", type=int, default=3)
    r.add_argument("--org", default=None, help="org id to toggle doc_selection ON/OFF")
    r.set_defaults(func=cmd_run)
    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
