/**
 * ThaiRAG Load Test
 *
 * Purpose : Simulate a realistic concurrent workload and confirm the API
 *           stays within acceptable latency/error budgets.
 * Profile : Ramp to 50 VUs over 1 min, hold for 3 min, ramp down over 1 min
 *           (total ≈ 5 minutes).
 * Pass criteria:
 *   - p(95) response time < 2 s
 *   - p(99) response time < 5 s
 *   - error rate < 5 %
 */

import http from "k6/http";
import { check, group, sleep } from "k6";
import { Rate, Trend } from "k6/metrics";

// ── Configuration ─────────────────────────────────────────────────────────────

const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";
const EMAIL = __ENV.EMAIL || "loadtest@test.com";
const PASSWORD = __ENV.PASSWORD || "LoadTest1";

// ── Custom metrics ────────────────────────────────────────────────────────────

const errorRate = new Rate("errors");
const loginDuration = new Trend("login_duration_ms");

// ── Test options ──────────────────────────────────────────────────────────────

export const options = {
  stages: [
    { duration: "1m", target: 50 },  // ramp up
    { duration: "3m", target: 50 },  // steady state
    { duration: "1m", target: 0 },   // ramp down
  ],
  thresholds: {
    http_req_duration: ["p(95)<2000", "p(99)<5000"],
    errors: ["rate<0.05"],
  },
};

// ── Shared token cache (one login per VU init) ────────────────────────────────

// k6 runs setup() once before any VU starts. Each VU then logs in during its
// own first iteration so tokens are spread across the ramp-up naturally.

let cachedToken = null;

function getToken() {
  if (cachedToken) return cachedToken;

  const start = Date.now();
  const res = http.post(
    `${BASE_URL}/api/auth/login`,
    JSON.stringify({ email: EMAIL, password: PASSWORD }),
    { headers: { "Content-Type": "application/json" } }
  );
  loginDuration.add(Date.now() - start);

  const ok = check(res, {
    "login: status 200": (r) => r.status === 200,
  });
  errorRate.add(!ok);

  if (!ok) return null;

  try {
    const body = res.json();
    cachedToken = body.token || body.access_token || null;
    return cachedToken;
  } catch (_) {
    return null;
  }
}

// ── Read-heavy workload mix ───────────────────────────────────────────────────
//
//  Weight distribution (per iteration):
//   40 % — health check (cheapest, always unauthenticated)
//   30 % — list orgs
//   30 % — list users

export default function () {
  const roll = Math.random();

  // ── 40 % health check ────────────────────────────────────────────
  if (roll < 0.4) {
    group("health", () => {
      const res = http.get(`${BASE_URL}/health`);
      const ok = check(res, {
        "health: status 200": (r) => r.status === 200,
      });
      errorRate.add(!ok);
    });
    sleep(0.2);
    return;
  }

  // ── Authenticated requests (60 % of iterations) ──────────────────
  const token = getToken();
  if (!token) {
    sleep(1);
    return;
  }

  const authHeaders = {
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
  };

  if (roll < 0.7) {
    // 30 % — list organisations
    group("list orgs", () => {
      const res = http.get(`${BASE_URL}/api/km/orgs`, authHeaders);
      const ok = check(res, {
        "list orgs: status 200": (r) => r.status === 200,
        "list orgs: response is JSON array": (r) => {
          try {
            return Array.isArray(r.json());
          } catch (_) {
            return false;
          }
        },
      });
      errorRate.add(!ok);
    });
  } else {
    // 30 % — list users
    group("list users", () => {
      const res = http.get(`${BASE_URL}/api/km/users`, authHeaders);
      const ok = check(res, {
        "list users: status 200": (r) => r.status === 200,
        "list users: response is JSON array": (r) => {
          try {
            return Array.isArray(r.json());
          } catch (_) {
            return false;
          }
        },
      });
      errorRate.add(!ok);
    });
  }

  // Think time: 0.5–1.5 s per iteration to avoid hammering the server
  sleep(0.5 + Math.random());
}
