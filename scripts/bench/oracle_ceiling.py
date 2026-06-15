#!/usr/bin/env python3
"""Oracle ceiling: feed the answer-LLM ONLY the ground-truth document for each
verified question, score with the same numeric-aware matcher as clean_eval.

This is the upper bound of perfect doc-selection + full-doc-context: it measures
what the answer model can do when retrieval is solved and the whole right doc is
in context. If this ceiling is not meaningfully above the live pipeline's score,
no retrieval/scoping/full-doc work can close the gap — the answer model is the
limit.
"""
import argparse
import json
import sys

from clean_eval import GEN_MODEL, doc_text, login, ollama, token_score

ANSWER_PROMPT = (
    "ตอบคำถามจากเอกสารด้านล่างเท่านั้น ตอบสั้น กระชับ ตรงประเด็น "
    "ถ้าเป็นตัวเลขให้ระบุพร้อมหน่วยตามที่ปรากฏในเอกสาร\n\n"
    "เอกสาร:\n{doc}\n\nคำถาม: {q}\nคำตอบ:"
)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--set", required=True)
    ap.add_argument("--ws", required=True)
    args = ap.parse_args()

    s = login()
    data = json.load(open(args.set))
    items = (
        data
        if isinstance(data, list)
        else data.get("questions") or data.get("items") or []
    )

    cache = {}
    hits = 0.0
    misses = []
    for i, it in enumerate(items):
        did = it["doc_id"]
        if did not in cache:
            cache[did] = doc_text(s, args.ws, did)
        ctx = cache[did]
        ans = ollama(GEN_MODEL, ANSWER_PROMPT.format(doc=ctx[:24000], q=it["question"]))
        sc = token_score(it["expected_tokens"], ans)
        hits += sc
        if sc < 1.0:
            misses.append((it["question"], it["expected_tokens"], ans[:120]))
        print(f"[{i+1}/{len(items)}] {sc:.2f}", flush=True)

    n = len(items)
    print(f"\n=== ORACLE CEILING (gemma4, whole right doc in context) ===")
    print(f"  accuracy {100*hits/n:.1f}%  ({hits:.1f}/{n})")
    print(f"\n=== {len(misses)} non-perfect (ceiling misses = answer-model floor) ===")
    for q, exp, ans in misses[:25]:
        print(f"  Q: {q[:60]}")
        print(f"     want={exp}  got={ans!r}")


if __name__ == "__main__":
    sys.exit(main())
