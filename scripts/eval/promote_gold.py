#!/usr/bin/env python3
"""Turn reviewed decisions (decisions.json from review.html) into a gold set.

  python3 -I scripts/eval/promote_gold.py --decisions ~/Downloads/decisions.json \
      --out scripts/eval/gold/sme_v1.json [--test-share 0.5] [--seed 7]

Keeps approved items only, applies corrected answers, derives the
program-named question variant for shared-scope runs, and assigns a frozen
dev/test split (by document, so sibling questions do not leak across splits).
"""
import argparse
import json
import os
import random


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--decisions", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--test-share", type=float, default=0.5)
    ap.add_argument("--seed", type=int, default=7)
    args = ap.parse_args()

    with open(args.decisions, encoding="utf-8") as fh:
        items = json.load(fh)["items"]
    approved = [i for i in items if i.get("decision") == "approve"]

    docs = sorted({i["doc"] for i in approved})
    rng = random.Random(args.seed)
    rng.shuffle(docs)
    n_test = max(1, round(len(docs) * args.test_share))
    test_docs = set(docs[:n_test])

    gold = []
    for i in approved:
        answer = (i.get("corrected") or i.get("answer") or "").strip()
        q = i["question"].strip()
        gold.append(
            {
                "id": i["id"],
                "doc": i["doc"],
                "program": i["program"],
                "page": i.get("page"),
                "type": i["type"],
                "answerable": bool(i.get("answerable", True)),
                "question": q,
                # Shared-scope variant: a user who knows which program they mean.
                "question_named": f"{i['program']} {q}" if i["program"] else q,
                "answer": answer,
                "evidence": i.get("evidence", ""),
                "split": "test" if i["doc"] in test_docs else "dev",
                "corrected": bool(i.get("corrected")),
            }
        )

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump({"version": os.path.splitext(os.path.basename(args.out))[0], "items": gold}, fh, ensure_ascii=False, indent=1)

    by_type = {}
    for g in gold:
        by_type[g["type"]] = by_type.get(g["type"], 0) + 1
    print(f"approved {len(gold)} of {len(items)} reviewed; corrected {sum(g['corrected'] for g in gold)}")
    print(f"split: dev {sum(g['split'] == 'dev' for g in gold)} / test {sum(g['split'] == 'test' for g in gold)} (by document, {n_test} test docs)")
    print("types:", ", ".join(f"{k} {v}" for k, v in sorted(by_type.items())))
    print("wrote", args.out)


if __name__ == "__main__":
    main()
