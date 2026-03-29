#!/usr/bin/env bash
# Search quality regression script.
# Loads golden queries, calls the ThaiRAG API, computes recall@k and MRR,
# then compares against baseline_scores.json.
#
# Environment variables:
#   THAIRAG_API_URL   Base URL for the running API  (default: http://localhost:8080)
#   THRESHOLD         Minimum ratio vs baseline     (default: 0.90)
#   GOLDEN_QUERIES    Path to golden_queries.json   (default: tests/search_quality/golden_queries.json)
#   BASELINE_SCORES   Path to baseline_scores.json  (default: tests/search_quality/baseline_scores.json)

set -euo pipefail

API_URL="${THAIRAG_API_URL:-http://localhost:8080}"
THRESHOLD="${THRESHOLD:-0.90}"
GOLDEN_QUERIES="${GOLDEN_QUERIES:-tests/search_quality/golden_queries.json}"
BASELINE_SCORES="${BASELINE_SCORES:-tests/search_quality/baseline_scores.json}"
RESULTS_FILE="${RESULTS_FILE:-/tmp/search-quality-results.json}"

# Require jq and python3 (both available on ubuntu-latest GitHub runners)
command -v jq  >/dev/null 2>&1 || { echo "jq is required but not installed."; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "python3 is required but not installed."; exit 1; }

echo "==> Search Quality Evaluation"
echo "    API:       $API_URL"
echo "    Threshold: $THRESHOLD"
echo "    Queries:   $GOLDEN_QUERIES"
echo "    Baseline:  $BASELINE_SCORES"
echo ""

QUERY_COUNT=$(jq 'length' "$GOLDEN_QUERIES")
echo "    Loaded $QUERY_COUNT golden queries"
echo ""

# Run evaluation via Python (avoids bash JSON arithmetic)
python3 - <<PYEOF
import json, sys, urllib.request, urllib.error

api_url  = "${API_URL}"
threshold = float("${THRESHOLD}")
golden_path   = "${GOLDEN_QUERIES}"
baseline_path = "${BASELINE_SCORES}"
results_path  = "${RESULTS_FILE}"

with open(golden_path) as f:
    queries = json.load(f)

with open(baseline_path) as f:
    baseline = json.load(f)

results = []
total_recall = 0.0
total_mrr    = 0.0

for q in queries:
    query_id  = q["id"]
    query_text = q["query"]
    expected  = set(q.get("expected_doc_ids", []))
    k         = q.get("k", 5)

    payload = json.dumps({
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": query_text}],
        "stream": False
    }).encode()

    req = urllib.request.Request(
        f"{api_url}/v1/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            response = json.loads(resp.read())
        # Extract cited document IDs from response metadata if present,
        # otherwise treat the response as a text answer (no doc IDs available).
        cited = set(response.get("cited_doc_ids", []))
        answer = response.get("choices", [{}])[0].get("message", {}).get("content", "")
    except urllib.error.URLError as e:
        print(f"  [WARN] Query '{query_id}' failed: {e}")
        cited  = set()
        answer = ""

    # Recall@k: how many expected docs appear in cited (up to k)
    if expected:
        hit_count = len(expected & cited)
        recall_k  = hit_count / len(expected)
    else:
        # No expected doc IDs — fall back to keyword presence in answer
        hits = sum(1 for kw in q.get("expected_keywords", []) if kw.lower() in answer.lower())
        recall_k = hits / max(len(q.get("expected_keywords", [])), 1)

    # MRR: rank of first relevant result (1-based); 0 if none
    ranked = list(response.get("cited_doc_ids", []))
    mrr = 0.0
    for rank, doc_id in enumerate(ranked[:k], start=1):
        if doc_id in expected:
            mrr = 1.0 / rank
            break
    # If no ranked results but keyword recall > 0, use recall as MRR proxy
    if mrr == 0.0 and recall_k > 0:
        mrr = recall_k

    results.append({
        "id": query_id,
        "query": query_text,
        "recall_at_k": round(recall_k, 4),
        "mrr": round(mrr, 4),
    })
    total_recall += recall_k
    total_mrr    += mrr
    print(f"  [{query_id}] recall@{k}={recall_k:.3f}  mrr={mrr:.3f}")

n = len(queries) or 1
avg_recall = total_recall / n
avg_mrr    = total_mrr    / n

summary = {
    "avg_recall_at_k": round(avg_recall, 4),
    "avg_mrr":         round(avg_mrr, 4),
    "query_results":   results,
}

with open(results_path, "w") as f:
    json.dump(summary, f, indent=2)

print()
print(f"==> Results: avg_recall@k={avg_recall:.4f}  avg_mrr={avg_mrr:.4f}")
print(f"    Baseline: avg_recall@k={baseline['avg_recall_at_k']}  avg_mrr={baseline['avg_mrr']}")
print()

failures = []
for metric in ("avg_recall_at_k", "avg_mrr"):
    current  = summary[metric]
    base_val = baseline[metric]
    min_ok   = base_val * threshold
    status   = "OK" if current >= min_ok else "FAIL"
    print(f"  {metric:20s}: {current:.4f} >= {min_ok:.4f} (baseline {base_val} * {threshold}) -> {status}")
    if status == "FAIL":
        failures.append(metric)

if failures:
    print()
    print(f"FAILED: {', '.join(failures)} dropped below {threshold*100:.0f}% of baseline.")
    sys.exit(1)

print()
print("All search quality checks passed.")
PYEOF
