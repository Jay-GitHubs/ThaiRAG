# Feature Testing Guide — Model Capability & Discovery

A focused, self-contained runbook for verifying the **advisory model-capability +
model-discovery** features (the searchable model pickers, ⭐/vision badges, and the
layered recommendation resolver). For the full SIT/UAT suite see
[`TESTING_GUIDE.md`](./TESTING_GUIDE.md) (§12–13 cover these same features in the
numbered-section style); for the operator-facing config see
[`OPERATOR_GUIDE.md`](./OPERATOR_GUIDE.md) §2.6.5 and §2.6.8.

> **Guiding principle being verified:** capability detection is **advisory, never
> enforcing**. The system *recommends* models but never blocks one — any model id
> stays selectable and is attempted.

---

## 0. Prerequisites

- The server is running the build that includes these endpoints (`git log` should
  show `feat(api,admin-ui): layered model-recommendation resolver (PR-D1)` and
  `… HTTP + MCP model-discovery sources (PR-D2)`). If you deploy via Docker,
  rebuild with `./scripts/docker-rebuild.sh thairag` (backs up the DB first).
- `curl` + `jq`.
- A **super-admin** account (these endpoints are super-admin gated).

```bash
API_URL="http://localhost:8080"
TOKEN=$(curl -fsS -X POST "$API_URL/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"<admin-email>","password":"<password>"}' | jq -r .token)
[ -n "$TOKEN" ] && echo "token OK" || echo "LOGIN FAILED"
```

---

## 1. Capability resolve — the core fix

The original bug: a valid vision model (`qwen3-vl:8b-instruct-bf16`) was silently
skipped because it wasn't on a hardcoded list. Verify it now resolves as
vision-capable:

```bash
curl -fsS -X POST "$API_URL/api/km/settings/recommendations/resolve" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"kind":"Ollama","models":["qwen3-vl:8b-instruct-bf16","llama3.2","llava:13b"]}' | jq .
```

**Expect:**
- `resolved["qwen3-vl:8b-instruct-bf16"].vision == true`
- `resolved["llava:13b"].vision == true`
- `resolved["llama3.2"].vision == false`
- `source` is `"builtin"` (offline floor) or `"catalog"` (once a discovery source warms).
- A `status` object is included alongside `resolved`.

Repeat with `"kind":"Claude"` / `["claude-sonnet-4-20250514"]` → `vision: true`,
`recommended: true`.

---

## 2. Catalog status & refresh

```bash
# Status (drives the admin-UI "Model Recommendations" banner)
curl -fsS "$API_URL/api/km/settings/recommendations/status" \
  -H "Authorization: Bearer $TOKEN" | jq .
# Expect: { has_data, model_count, age_secs?, stale, enabled, configured }

# Warm the catalog (fire-and-forget background fetch), then re-check status
curl -fsS -X POST "$API_URL/api/km/settings/recommendations/refresh" \
  -H "Authorization: Bearer $TOKEN" | jq .
sleep 3
curl -fsS "$API_URL/api/km/settings/recommendations/status" \
  -H "Authorization: Bearer $TOKEN" | jq '{has_data, model_count, stale, last_error}'
```

**Expect (host with internet):** after the refresh, `has_data: true` and
`model_count` in the hundreds/thousands (LiteLLM catalog). `last_error` absent.

**Expect (air-gapped / no internet):** `has_data: false`, `last_error` populated
with the fetch failure — and §1 still returns correct `source: "builtin"` verdicts.
This is the graceful-degradation guarantee.

---

## 3. Discovery config round-trip & air-gapped toggle

```bash
# Read current settings
curl -fsS "$API_URL/api/km/settings/model-discovery" \
  -H "Authorization: Bearer $TOKEN" | jq .
# Expect: { enabled, catalog_url, mode:"catalog", endpoint, tool, auth }

# Disable external discovery (air-gapped) — clears the cache
curl -fsS -X PUT "$API_URL/api/km/settings/model-discovery" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"enabled":false,"catalog_url":"","mode":"catalog","endpoint":"","tool":"","auth":""}' | jq .
# Resolve again → source must be "builtin"
curl -fsS -X POST "$API_URL/api/km/settings/recommendations/resolve" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"kind":"OpenAi","models":["gpt-4o"]}' | jq '.resolved["gpt-4o"]'
# Re-enable when done
curl -fsS -X PUT "$API_URL/api/km/settings/model-discovery" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"enabled":true,"catalog_url":"","mode":"catalog","endpoint":"","tool":"","auth":""}' >/dev/null
```

### 3a. Custom HTTP catalog (`mode: http_catalog`)

Point `endpoint` at any URL returning either LiteLLM-style JSON or a simple
`{ "<id>": { "vision": bool, "recommended": bool, "max_input_tokens": n } }` map:

```bash
curl -fsS -X PUT "$API_URL/api/km/settings/model-discovery" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"enabled":true,"catalog_url":"","mode":"http_catalog","endpoint":"https://your-host/caps.json","tool":"","auth":""}' >/dev/null
curl -fsS -X POST "$API_URL/api/km/settings/recommendations/refresh" -H "Authorization: Bearer $TOKEN" >/dev/null
sleep 3
curl -fsS "$API_URL/api/km/settings/recommendations/status" -H "Authorization: Bearer $TOKEN" | jq '{has_data, model_count, last_error}'
```

**Expect:** `has_data: true` from your endpoint; an explicit `recommended`/`vision`
in your JSON overrides the built-in floor for those ids.

### 3b. MCP discovery tool (`mode: mcp`) — best-effort

Requires `[mcp].enabled = true` on the server. Without a real MCP server exposing
a capability tool, verify the **graceful-degradation** path:

```bash
curl -fsS -X PUT "$API_URL/api/km/settings/model-discovery" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"enabled":true,"catalog_url":"","mode":"mcp","endpoint":"https://bad.invalid/sse","tool":"list_models","auth":""}' >/dev/null
curl -fsS -X POST "$API_URL/api/km/settings/recommendations/refresh" -H "Authorization: Bearer $TOKEN" >/dev/null
sleep 3
curl -fsS "$API_URL/api/km/settings/recommendations/status" -H "Authorization: Bearer $TOKEN" | jq '{has_data, last_error}'
```

**Expect:** `last_error` populated (connect/tool failure), and §1 `resolve` still
returns correct built-in verdicts. A working MCP server should call `tool` with no
args and return a capability map / array of `{id, vision, recommended, ...}`.
Restore `mode: catalog` when done.

---

## 4. Admin UI checks (Settings → Providers)

| Check | Expected |
|---|---|
| **Free-text model entry** | Type `qwen3-vl:8b-instruct-bf16` into the Vision LLM model box — it stays selectable and saves, even though it's not in any built-in list. |
| **Per-section Sync** | LLM, Vision LLM, and Reranker fields each have a **Sync** button that lists the provider's live models. |
| **Badges** | Recognized models show **⭐ recommended** and/or **vision** tags in the dropdown; tooltips say "a recommendation, not a requirement." |
| **Model Recommendations panel** | Shows source = *catalog* (with model count + freshness) or *built-in*; has the enable toggle, discovery-source selector (Built-in / Custom HTTP / MCP) with mode-specific fields, and a **Refresh now** button. |
| **Graceful UI fallback** | With discovery disabled/unreachable, badges still appear (local heuristic) and the panel shows the *built-in* source + any error. |

---

## 5. End-to-end (vision OCR still works)

Confirm the advisory change didn't break real OCR: with a vision LLM configured
(even an "unrecognized" one like `qwen3-vl`), upload an image-only PDF and verify
it produces semantic-markdown chunks rather than failing. Use:

```bash
./scripts/docker-verify.sh --smart-pdf <file.pdf> --workspace <id> --no-teardown
```

See `TESTING_GUIDE.md` for the full smart-PDF assertions (page strategies,
persisted image blobs, pdfium engine load).

---

## Pass criteria

- §1 resolves `qwen3-vl` as vision-capable (the original bug is fixed).
- §2 catalog warms on a connected host **and** degrades cleanly when offline.
- §3 config round-trips; disabling forces `source: "builtin"`; http_catalog and the
  mcp error-path behave as described.
- §4 UI lets you select any model id and shows advisory badges.
- §5 a real image-only PDF still OCRs end to end.
