/**
 * ThaiRAG Smoke Test
 *
 * Purpose : Verify the API is healthy and core endpoints respond correctly.
 * Profile : 1 VU, 30 seconds — minimal load, zero ramp.
 * Pass criteria:
 *   - p(95) response time < 500 ms
 *   - error rate < 1 %
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate } from "k6/metrics";

// ── Configuration ────────────────────────────────────────────────────────────

const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";
const EMAIL = __ENV.EMAIL || "loadtest@test.com";
const PASSWORD = __ENV.PASSWORD || "LoadTest1";

// ── Custom metrics ────────────────────────────────────────────────────────────

const errorRate = new Rate("errors");

// ── Test options ──────────────────────────────────────────────────────────────

export const options = {
  vus: 1,
  duration: "30s",
  thresholds: {
    http_req_duration: ["p(95)<500"],
    errors: ["rate<0.01"],
  },
};

// ── Helper: login and return JWT token ───────────────────────────────────────

function login() {
  const res = http.post(
    `${BASE_URL}/api/auth/login`,
    JSON.stringify({ email: EMAIL, password: PASSWORD }),
    { headers: { "Content-Type": "application/json" } }
  );

  const ok = check(res, {
    "login: status 200": (r) => r.status === 200,
    "login: token present": (r) => {
      try {
        return r.json("token") !== undefined || r.json("access_token") !== undefined;
      } catch (_) {
        return false;
      }
    },
  });

  errorRate.add(!ok);

  if (!ok) {
    console.error(`Login failed — status ${res.status}: ${res.body}`);
    return null;
  }

  try {
    const body = res.json();
    return body.token || body.access_token;
  } catch (_) {
    return null;
  }
}

// ── Main VU scenario ──────────────────────────────────────────────────────────

export default function () {
  // 1. Health check (unauthenticated)
  {
    const res = http.get(`${BASE_URL}/health`);
    const ok = check(res, {
      "health: status 200": (r) => r.status === 200,
    });
    errorRate.add(!ok);
  }

  sleep(0.5);

  // 2. Login
  const token = login();
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

  sleep(0.5);

  // 3. List organisations
  {
    const res = http.get(`${BASE_URL}/api/km/orgs`, authHeaders);
    const ok = check(res, {
      "list orgs: status 200": (r) => r.status === 200,
      "list orgs: body is array": (r) => {
        try {
          return Array.isArray(r.json());
        } catch (_) {
          return false;
        }
      },
    });
    errorRate.add(!ok);
  }

  sleep(0.5);

  // 4. List users
  {
    const res = http.get(`${BASE_URL}/api/km/users`, authHeaders);
    const ok = check(res, {
      "list users: status 200": (r) => r.status === 200,
      "list users: body is array": (r) => {
        try {
          return Array.isArray(r.json());
        } catch (_) {
          return false;
        }
      },
    });
    errorRate.add(!ok);
  }

  sleep(1);
}
