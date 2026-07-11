#!/usr/bin/env python3
"""Post-launch usage report from the inference logs.

Feeds the capacity model with MEASURED inputs (see docs/LAUNCH_RUNBOOK.md §6):
queries/user/day, request-duration percentiles, observed + Little's-law
concurrency, and est-vs-actual prompt tokens.

Usage:
  python3 scripts/ops/usage_report.py [--days 7] [--users 1500]
Env: THAIRAG_API (default http://localhost:8080), THAIRAG_EMAIL, THAIRAG_PASSWORD
"""
import argparse
import datetime as dt
import os
import statistics
import sys

import requests

API = os.environ.get("THAIRAG_API", "http://localhost:8080")
EMAIL = os.environ.get("THAIRAG_EMAIL", "playwright@test.com")
PASSWORD = os.environ.get("THAIRAG_PASSWORD", "Test1234!")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--days", type=int, default=7, help="lookback window")
    ap.add_argument("--users", type=int, default=1500, help="planned user count for projection")
    args = ap.parse_args()

    s = requests.Session()
    r = s.post(f"{API}/api/auth/login", json={"email": EMAIL, "password": PASSWORD}, timeout=30)
    r.raise_for_status()
    s.headers["Authorization"] = f"Bearer {r.json()['token']}"

    entries = s.get(
        f"{API}/api/km/settings/inference-logs", params={"limit": 10000}, timeout=120
    ).json().get("entries", [])
    cutoff = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=args.days)

    rows = []
    for e in entries:
        try:
            end = dt.datetime.fromisoformat(e["timestamp"].replace("Z", "+00:00"))
        except (KeyError, ValueError):
            continue
        if end < cutoff:
            continue
        rows.append(
            {
                "end": end,
                "dur_s": (e.get("total_ms") or 0) / 1000.0,
                "user": e.get("user_id") or "anon",
                "est": e.get("estimated_context_tokens") or 0,
                "act": e.get("prompt_tokens") or 0,
            }
        )
    if not rows:
        print(f"No inference-log entries in the last {args.days} day(s).")
        return 1

    days = {}
    users_by_day = {}
    for row in rows:
        d = row["end"].date().isoformat()
        days[d] = days.get(d, 0) + 1
        users_by_day.setdefault(d, set()).add(row["user"])

    print(f"── Usage report: last {args.days} day(s), {len(rows)} requests ──")
    print(f"{'day':12} {'requests':>9} {'users':>6} {'q/user':>7}")
    qpu_samples = []
    for d in sorted(days):
        n, u = days[d], len(users_by_day[d])
        qpu = n / u if u else 0.0
        qpu_samples.append(qpu)
        print(f"{d:12} {n:>9} {u:>6} {qpu:>7.1f}")

    durs = sorted(row["dur_s"] for row in rows if row["dur_s"] > 0)
    if durs:
        p50 = statistics.median(durs)
        p90 = durs[int(len(durs) * 0.9)]
        print(f"\nrequest duration: p50={p50:.0f}s p90={p90:.0f}s n={len(durs)}")

        # Observed concurrency (interval sweep) and Little's-law projection.
        events = []
        for row in rows:
            start = row["end"] - dt.timedelta(seconds=row["dur_s"])
            events += [(start, 1), (row["end"], -1)]
        events.sort()
        cur = peak = 0
        for _, delta in events:
            cur += delta
            peak = max(peak, cur)
        qpu_med = statistics.median(qpu_samples) if qpu_samples else 0.0
        avg_rps = (args.users * qpu_med) / 86400.0
        avg_conc = avg_rps * p50
        print(f"observed peak concurrency (window): {peak}")
        print(
            f"projection @ {args.users} users × {qpu_med:.1f} q/user/day: "
            f"avg concurrency ≈ {avg_conc:.1f}, peak (×5–10) ≈ "
            f"{avg_conc * 5:.0f}–{avg_conc * 10:.0f}"
        )

    both = [(row["est"], row["act"]) for row in rows if row["est"] > 0 and row["act"] > 0]
    if both:
        ratios = sorted(a / e for e, a in both)
        acts = sorted(a for _, a in both)
        print(
            f"\nprompt tokens (n={len(both)}): actual p50={statistics.median(acts):.0f} "
            f"p90={acts[int(len(acts) * 0.9)]:.0f}; actual/est ratio "
            f"p50={statistics.median(ratios):.2f} p90={ratios[int(len(ratios) * 0.9)]:.2f}"
        )
        print("capacity math must use ACTUAL prompt_tokens (estimator is context-only).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
