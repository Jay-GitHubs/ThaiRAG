#!/usr/bin/env python3
"""A/B accuracy: Vector (hybrid) vs Vectorless (reasoning / PageIndex).

Runs the SAME frozen eval set under each retrieval mode (set at workspace scope)
and prints a side-by-side comparison, so the reasoning path can be measured
against the hybrid-vector baseline and the old lexical floor on identical
questions. Reuses clean_eval's auth / query / deterministic token scoring so the
numbers are directly comparable to the rest of the bench suite.

Requires the live Docker stack (thairag API + Ollama) and an ingested corpus.
Vectorless mode reasons over per-document trees, so build them first — either
flip `chat_pipeline.reasoning_build_on_ingest` before ingest, or pass
`--build-trees` here to backfill via POST /workspaces/{ws}/documents/build-trees
(reads stored converted text only; never re-OCRs/re-chunks). Any document
without a tree transparently falls back to lexical BM25.

Usage:
  python3 reasoning_compare.py --set clean_set.json --ws <WS_ID> [--build-trees] [--runs 2]

The eval set is the same human-reviewed JSON clean_eval produces (`build`).
Customer-derived sets are gitignored; pass your own with --set.
"""

import argparse
import json
import sys
import time

# Reuse the bench harness's auth, querying, warm-up, and scoring verbatim so
# results line up with clean_eval. clean_eval guards main(), so importing it is
# side-effect-free beyond reading env vars.
import clean_eval as ce


def set_ws_retrieval_mode(s, ws: str, mode: str) -> None:
    """Set retrieval_mode at the workspace scope (so it applies to queries that
    resolve to this workspace, mirroring the admin UI selector)."""
    url = f"{ce.API}/api/km/settings/chat-pipeline?scope_type=workspace&scope_id={ws}"
    r = s.put(url, json={"retrieval_mode": mode}, timeout=60)
    r.raise_for_status()
    time.sleep(2)


def build_trees(s, ws: str, poll_timeout: int = 3600) -> dict:
    """Backfill PageIndex trees for every Ready doc in the workspace.

    The endpoint runs in the background (each tree is an LLM call), returning a
    job_id immediately; we poll GET .../jobs/{id} until it reaches a terminal
    status so the sweep doesn't start before the trees exist.
    """
    r = s.post(
        f"{ce.API}/api/km/workspaces/{ws}/documents/build-trees", json={}, timeout=60
    )
    r.raise_for_status()
    started = r.json()
    job_id = started.get("job_id")
    if not job_id:
        return started  # nothing to build (no Ready docs)

    deadline = time.time() + poll_timeout
    while time.time() < deadline:
        time.sleep(5)
        jr = s.get(f"{ce.API}/api/km/workspaces/{ws}/jobs/{job_id}", timeout=30)
        jr.raise_for_status()
        job = jr.json()
        st = job.get("status")
        print(
            f"[compare]   trees: {st} {job.get('items_processed', 0)}/{job.get('items_total')}",
            file=sys.stderr,
        )
        if st in ("completed", "failed", "cancelled"):
            return {
                "status": st,
                "items_processed": job.get("items_processed"),
                "items_total": job.get("items_total"),
                "error": job.get("error"),
            }
    return {"status": "timeout", "job_id": job_id}


def sweep(s, ws: str, questions: list, runs: int) -> dict:
    """Mean token-score over `runs` passes; transients excluded, not scored 0."""
    per_q = [0.0] * len(questions)
    transient = set()
    run_means = []
    for run in range(runs):
        scores = []
        for i, q in enumerate(questions):
            ans, ok = ce.ask(s, ws, q["question"])
            if not ok:
                transient.add(i)
                continue
            sc = ce.token_score(q["expected_tokens"], ans)
            scores.append(sc)
            per_q[i] += sc
        run_means.append(sum(scores) / len(scores) if scores else 0.0)
        print(f"[compare]   run {run + 1}/{runs}: mean={run_means[-1]:.3f}", file=sys.stderr)
    scored = len(questions) - len(transient)
    return {
        "mean": sum(run_means) / len(run_means) if run_means else 0.0,
        "run_means": run_means,
        "scored": scored,
        "transient": len(transient),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--set", required=True, help="frozen eval JSON (clean_eval build output)")
    ap.add_argument("--ws", required=True, help="workspace id")
    ap.add_argument("--runs", type=int, default=2, help="passes per mode (default 2)")
    ap.add_argument(
        "--modes",
        default="vector,vectorless",
        help="comma list of retrieval modes to compare (default vector,vectorless)",
    )
    ap.add_argument(
        "--build-trees",
        action="store_true",
        help="backfill PageIndex trees before measuring (needed for vectorless reasoning)",
    )
    args = ap.parse_args()

    questions = json.load(open(args.set))["questions"]
    modes = [m.strip() for m in args.modes.split(",") if m.strip()]
    s = ce.login()

    # Pin the answer model to temperature 0 for determinism (restored after).
    cp = ce.get_cp(s)
    prev_llm = cp.get("llm")
    if prev_llm:
        det = dict(prev_llm)
        det["temperature"] = 0.0
        ce.put_cp(s, {"llm": det})

    if args.build_trees:
        print("[compare] building trees (backfill, no re-OCR)…", file=sys.stderr)
        rep = build_trees(s, args.ws)
        print(f"[compare] build-trees: {rep}", file=sys.stderr)

    results = {}
    try:
        for mode in modes:
            print(f"[compare] === mode: {mode} ===", file=sys.stderr)
            set_ws_retrieval_mode(s, args.ws, mode)
            ce.warm_up(s, args.ws, questions[0]["question"])
            results[mode] = sweep(s, args.ws, questions, args.runs)
    finally:
        # Restore defaults: workspace back to vector, answer LLM as it was.
        set_ws_retrieval_mode(s, args.ws, "vector")
        if prev_llm:
            restore = dict(prev_llm)
            restore.pop("has_api_key", None)
            ce.put_cp(s, {"llm": restore})

    # Report.
    print("\n=== Retrieval-mode comparison ===")
    print(f"{'mode':<14}{'accuracy':>10}{'scored':>9}{'transient':>11}")
    for mode in modes:
        r = results.get(mode, {})
        print(
            f"{mode:<14}{r.get('mean', 0.0) * 100:>9.1f}%"
            f"{r.get('scored', 0):>9}{r.get('transient', 0):>11}"
        )
    out = args.set.replace(".json", "_modecompare.json")
    json.dump(results, open(out, "w"), indent=1, ensure_ascii=False)
    print(f"\nwrote {out}")


if __name__ == "__main__":
    main()
