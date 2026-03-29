/**
 * ThaiRAG Stress Test
 *
 * Purpose : Push the API to — and beyond — its practical limit to find the
 *           breaking point (error spike, latency cliff, OOM, etc.).
 * Profile:
 *   Phase 1 — warm up   :  0 → 10 VUs over 1 min
 *   Phase 2 — load zone : hold 10 VUs for 1 min
 *   Phase 3 — push      : 10 → 100 VUs over 2 min
 *   Phase 4 — spike     : hold 100 VUs for 2 min
 *   Phase 5 — peak      : 100 → 200 VUs over 2 min
 *   Phase 6 — hold peak : hold 200 VUs for 1 min
 *   Phase 7 — recovery  :  200 → 0 VUs over 1 min
 *   Total ≈ 10 minutes
 * Pass criteria:
 *   - p(95) response time < 10 s
 *   - error rate < 20 %
 *
 * Note: thresholds are intentionally lenient — the goal is observation, not
 *       a hard pass/fail gate. Tighten them once you know your baseline.
 */

import http from "k6/http";
import { check, group, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

// ── Configuration ─────────────────────────────────────────────────────────────

const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";
const EMAIL = __ENV.EMAIL || "loadtest@test.com";
const PASSWORD = __ENV.PASSWORD || "LoadTest1";

// ── Custom metrics ────────────────────────────────────────────────────────────

const errorRate = new Rate("errors");
const loginErrors = new Counter("login_errors");
const authDuration = new Trend("auth_duration_ms");

// ── Test options ──────────────────────────────────────────────────────────────

export const options = {
  stages: [
    { duration: "1m", target: 10 },   // warm up
    { duration: "1m", target: 10 },   // hold — establish baseline
    { duration: "2m", target: 100 },  // push — find first signs of stress
    { duration: "2m", target: 100 },  // hold at 100 VUs
    { duration: "2m", target: 200 },  // peak — find breaking point
    { duration: "1m", target: 200 },  // hold at peak
    { duration: "1m", target: 0 },    // recovery ramp-down
  ],
  thresholds: {
    http_req_duration: ["p(95)<10000"],
    errors: ["rate<0.20"],
  },
};

// ── Per-VU token cache ────────────────────────────────────────────────────────
// Each VU maintains its own token so we don't race on a shared variable.

let _token = null;

function getToken() {
  if (_token) return _token;

  const start = Date.now();
  const res = http.post(
    `${BASE_URL}/api/auth/login`,
    JSON.stringify({ email: EMAIL, password: PASSWORD }),
    {
      headers: { "Content-Type": "application/json" },
      timeout: "10s",
    }
  );
  authDuration.add(Date.now() - start);

  if (res.status !== 200) {
    loginErrors.add(1);
    errorRate.add(1);
    return null;
  }

  try {
    const body = res.json();
    _token = body.token || body.access_token || null;
    return _token;
  } catch (_) {
    loginErrors.add(1);
    errorRate.add(1);
    return null;
  }
}

// ── Workload mix (same as load test, kept consistent for comparability) ───────

export default function () {
  const roll = Math.random();

  // 40 % health check
  if (roll < 0.4) {
    group("health", () => {
      const res = http.get(`${BASE_URL}/health`, { timeout: "10s" });
      const ok = check(res, {
        "health: status 200": (r) => r.status === 200,
      });
      errorRate.add(!ok);
    });
    sleep(0.1);
    return;
  }

  // Remaining 60 % require auth
  const token = getToken();
  if (!token) {
    // Token acquisition failed; back off and let the VU try again next iter
    sleep(2);
    return;
  }

  const authHeaders = {
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    timeout: "10s",
  };

  if (roll < 0.7) {
    // 30 % — list organisations
    group("list orgs", () => {
      const res = http.get(`${BASE_URL}/api/km/orgs`, authHeaders);
      const ok = check(res, {
        "list orgs: status 200": (r) => r.status === 200,
      });
      // If we receive 401 the token may have expired under load; clear it so
      // the next iteration re-authenticates.
      if (res.status === 401) _token = null;
      errorRate.add(!ok);
    });
  } else {
    // 30 % — list users
    group("list users", () => {
      const res = http.get(`${BASE_URL}/api/km/users`, authHeaders);
      const ok = check(res, {
        "list users: status 200": (r) => r.status === 200,
      });
      if (res.status === 401) _token = null;
      errorRate.add(!ok);
    });
  }

  // Shorter think time under stress to maintain pressure
  sleep(0.1 + Math.random() * 0.4);
}

// ── Teardown: print a summary hint ───────────────────────────────────────────

export function teardown() {
  console.log(
    "Stress test complete. Check the p(95)/p(99) trend across stages " +
      "and note at which VU count error rates began rising."
  );
}
