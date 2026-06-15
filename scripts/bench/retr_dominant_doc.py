#!/usr/bin/env python3
"""Offline test: bias context toward a SINGLE document chosen by retrieval-score
aggregation, then feed gemma4 only that doc (full text). Tests whether removing
cross-near-clone contamination from the context recovers accuracy — WITHOUT the
facet selector (which scoped to the right doc only 23%).

Pick-doc rule: run normal retrieval, sum each doc's retrieved-chunk scores, take
the top doc. Then answer from that doc's full text (the oracle prompt that hit 99%).
Reports: accuracy, and how often the retrieval-dominant doc is the ground-truth doc.
"""
import argparse
import json
import os

from clean_eval import GEN_MODEL, doc_text, login, ollama, token_score

API = os.environ.get("THAIRAG_API", "http://localhost:8080")
ANSWER_PROMPT = (
    "ตอบคำถามจากเอกสารด้านล่างเท่านั้น ตอบสั้น กระชับ ตรงประเด็น "
    "ถ้าเป็นตัวเลขให้ระบุพร้อมหน่วยตามที่ปรากฏในเอกสาร\n\n"
    "เอกสาร:\n{doc}\n\nคำถาม: {q}\nคำตอบ:"
)


def dominant_doc(s, ws, q, agg="sum"):
    r = s.post(f"{API}/api/km/workspaces/{ws}/test-query", json={"query": q}, timeout=120).json()
    scores = {}
    for c in r.get("chunks", []):
        did = c.get("doc_id")
        sc = float(c.get("score") or 0.0)
        if agg == "max":
            scores[did] = max(scores.get(did, 0.0), sc)
        else:
            scores[did] = scores.get(did, 0.0) + sc
    if not scores:
        return None
    return max(scores, key=scores.get)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--set", required=True)
    ap.add_argument("--ws", required=True)
    ap.add_argument("--agg", default="sum", choices=["sum", "max"])
    args = ap.parse_args()

    s = login()
    items = json.load(open(args.set))["questions"]
    N = len(items)
    cache = {}
    hits = 0.0
    correct_doc = 0
    for i, it in enumerate(items):
        best = dominant_doc(s, args.ws, it["question"], args.agg)
        if best == it["doc_id"]:
            correct_doc += 1
        if best is None:
            sc = 0.0
        else:
            if best not in cache:
                cache[best] = doc_text(s, args.ws, best)
            ans = ollama(GEN_MODEL, ANSWER_PROMPT.format(doc=cache[best][:24000], q=it["question"]))
            sc = token_score(it["expected_tokens"], ans)
        hits += sc
        pct = int(100 * (i + 1) / N)
        bar = "#" * (pct // 5) + "-" * (20 - pct // 5)
        print(f"[{bar}] {pct:3d}% ({i+1}/{N}) acc={100*hits/(i+1):.0f}% rightdoc={correct_doc}", flush=True)

    print(f"\nSUMMARY ({args.agg}-aggregation, retrieval-dominant doc + full-doc context)", flush=True)
    print(f"  accuracy:                 {100*hits/N:.1f}%  ({hits:.1f}/{N})", flush=True)
    print(f"  retrieval picked RIGHT doc: {correct_doc}/{N} = {100*correct_doc/N:.0f}%", flush=True)
    print(f"  (baseline OFF pipeline = 75%; facet-selector right-doc = 23%)", flush=True)


if __name__ == "__main__":
    main()
