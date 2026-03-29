# ThaiRAG Load Tests (k6)

Load testing scripts for the ThaiRAG API using [k6](https://k6.io/).

## Install k6

**macOS**

```sh
brew install k6
```

**Linux (Debian / Ubuntu)**

```sh
sudo gpg -k
sudo gpg --no-default-keyring \
  --keyring /usr/share/keyrings/k6-archive-keyring.gpg \
  --keyserver hkp://keyserver.ubuntu.com:80 \
  --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] \
  https://dl.k6.io/deb stable main" \
  | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update && sudo apt-get install k6
```

**Docker**

```sh
docker pull grafana/k6
```

## Prerequisites

The ThaiRAG server must be running and reachable at `http://localhost:8080`
(or the URL you pass via `BASE_URL`).

The load-test user must exist before running any script:

```sh
curl -s -X POST http://localhost:8080/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"loadtest@test.com","password":"LoadTest1"}'
```

If the user already exists the command will return a 409 — that is fine.

## Running the tests

All scripts accept the following environment variables:

| Variable   | Default                   | Description           |
|------------|---------------------------|-----------------------|
| `BASE_URL` | `http://localhost:8080`   | API base URL          |
| `EMAIL`    | `loadtest@test.com`       | Login email           |
| `PASSWORD` | `LoadTest1`               | Login password        |

### Smoke test — 1 VU, 30 s

Verifies that the API starts correctly and core endpoints are reachable.
Run this before every deployment.

```sh
k6 run tests/load/k6-smoke.js
```

### Load test — 50 VUs, ~5 min

Simulates a realistic concurrent workload (read-heavy mix).
Use this to establish performance baselines and catch regressions.

```sh
k6 run tests/load/k6-load.js
```

### Stress test — ramp 10 → 200 VUs, ~10 min

Finds the breaking point by progressively increasing concurrency.
Watch for the VU count where p(95) latency spikes or errors begin rising.

```sh
k6 run tests/load/k6-stress.js
```

### Override defaults

```sh
BASE_URL=http://staging.example.com:8080 k6 run tests/load/k6-load.js
```

## Interpreting results

```
scenarios: (100.00%) 1 scenario, 50 max VUs
default: ...

✓ health: status 200
✓ list orgs: status 200
✓ list users: status 200

checks.........................: 99.80%  ✓ 14970  ✗ 30
data_received..................: 12 MB   40 kB/s
data_sent......................: 3.1 MB  10 kB/s
http_req_duration..............: avg=180ms  min=12ms  med=150ms  max=4.2s
  { expected_response:true }...: avg=180ms  min=12ms  med=150ms  max=4.2s
http_req_failed................: 0.20%   ✓ 30     ✗ 14970
errors.........................: 0.20%   ✓ 30     ✗ 14970

✓ http_req_duration........: p(95)=420ms < 2000ms
✓ errors...................: rate=0.20% < 5%
```

Key columns to watch:

- **`http_req_duration p(95)/p(99)`** — tail latency. The threshold values
  differ per script (500 ms smoke / 2 s load / 10 s stress).
- **`http_req_failed`** — requests that received a non-2xx response or timed
  out. Should stay well below the per-script error threshold.
- **`checks`** — application-level assertions (correct status code, valid JSON
  shape). A failing check does not automatically mark a request as failed.
- **`errors` (custom metric)** — aggregates both HTTP failures and failed
  checks. This is what the threshold gates.

For the stress test, correlate the stage timestamps with latency/error spikes
to identify the VU count at which the system saturates.

## Running with Docker

```sh
docker run --rm -i \
  --network host \
  -v "$(pwd)/tests/load:/scripts" \
  grafana/k6 run /scripts/k6-load.js
```
