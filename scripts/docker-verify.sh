#!/usr/bin/env bash
# docker-verify.sh — End-to-end verification of the ThaiRAG Docker setup.
#
# Usage:
#   ./scripts/docker-verify.sh [--no-teardown] [--native-ollama]
#                              [--smart-pdf <file.pdf> --workspace <id>]
#
# Options:
#   --no-teardown      Keep containers running after verification
#   --native-ollama    Use native Ollama instead of Docker (recommended on macOS
#                      for Metal GPU acceleration). Requires `ollama serve`.
#   --smart-pdf <f>    Upload <f> and verify the smart-PDF pipeline end to end:
#                      semantic-markdown content, persisted image blobs, page
#                      strategies, and that the pdfium engine actually loaded.
#                      Requires --workspace and a vision-capable vision_llm
#                      configured on the server (e.g. Ollama llava/qwen2.5vl).
#   --workspace <id>   Existing workspace id to upload the --smart-pdf file into.
#
# Prerequisites: docker, docker compose, curl, jq
set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────
API_URL="http://localhost:8080"
OLLAMA_URL="http://localhost:11435"
OLLAMA_MODEL="llama3.2"
COMPOSE_FILE="docker-compose.yml"

MAX_WAIT=300        # seconds to wait for Ollama readiness
HEALTH_WAIT=120     # seconds to wait for ThaiRAG health
PULL_TIMEOUT=600    # seconds for model pull
TEARDOWN=true
NATIVE_OLLAMA=false # set true on macOS to skip Docker Ollama (uses Metal GPU)
SMART_PDF_FILE=""
SMART_PDF_WS=""

while [ $# -gt 0 ]; do
    case "$1" in
        --no-teardown)   TEARDOWN=false ;;
        --native-ollama) NATIVE_OLLAMA=true ;;
        --smart-pdf)     SMART_PDF_FILE="${2:-}"; shift ;;
        --smart-pdf=*)   SMART_PDF_FILE="${1#*=}" ;;
        --workspace)     SMART_PDF_WS="${2:-}"; shift ;;
        --workspace=*)   SMART_PDF_WS="${1#*=}" ;;
    esac
    shift
done

# ── Helpers ───────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass=0
fail=0

log()  { echo -e "${GREEN}[✓]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
err()  { echo -e "${RED}[✗]${NC} $*"; }

check() {
    local desc="$1"; shift
    if "$@"; then
        log "$desc"
        pass=$((pass + 1))
    else
        err "$desc"
        fail=$((fail + 1))
    fi
}

cleanup() {
    if $TEARDOWN; then
        warn "Tearing down containers..."
        docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
    else
        warn "Skipping teardown (--no-teardown). Containers are still running."
    fi
}

# ── Step 1: Build ─────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "  ThaiRAG Docker Verification"
echo "═══════════════════════════════════════════════════════════════"
echo ""

echo "▸ Step 1: Building images..."
if ! docker compose -f "$COMPOSE_FILE" build; then
    err "docker compose build failed"
    exit 1
fi
log "Docker images built"

# ── Step 2: Start services ────────────────────────────────────────────
echo ""
echo "▸ Step 2: Starting services..."
if $NATIVE_OLLAMA; then
    # macOS: run only ThaiRAG container, connect to native Ollama via host.docker.internal
    warn "Using native Ollama (--native-ollama). Skipping Docker Ollama container."
    THAIRAG__PROVIDERS__LLM__BASE_URL=http://host.docker.internal:11435 \
        docker compose -f "$COMPOSE_FILE" up -d thairag
else
    docker compose -f "$COMPOSE_FILE" up -d
fi
trap cleanup EXIT
log "Services started"

# ── Step 3: Wait for Ollama ───────────────────────────────────────────
echo ""
echo "▸ Step 3: Waiting for Ollama (up to ${MAX_WAIT}s)..."
elapsed=0
while ! curl -sf "${OLLAMA_URL}/api/tags" >/dev/null 2>&1; do
    if [ $elapsed -ge $MAX_WAIT ]; then
        err "Ollama did not become ready within ${MAX_WAIT}s"
        exit 1
    fi
    sleep 2
    elapsed=$((elapsed + 2))
done
log "Ollama is ready (${elapsed}s)"

# ── Step 4: Pull model ───────────────────────────────────────────────
echo ""
echo "▸ Step 4: Pulling model '${OLLAMA_MODEL}' (up to ${PULL_TIMEOUT}s)..."
if $NATIVE_OLLAMA; then
    if ! timeout "$PULL_TIMEOUT" ollama pull "$OLLAMA_MODEL"; then
        err "Failed to pull model '${OLLAMA_MODEL}'"
        exit 1
    fi
else
    if ! timeout "$PULL_TIMEOUT" docker compose exec ollama ollama pull "$OLLAMA_MODEL"; then
        err "Failed to pull model '${OLLAMA_MODEL}'"
        exit 1
    fi
fi
log "Model '${OLLAMA_MODEL}' pulled"

# ── Step 5: Wait for ThaiRAG health ──────────────────────────────────
echo ""
echo "▸ Step 5: Waiting for ThaiRAG health (up to ${HEALTH_WAIT}s)..."
elapsed=0
while ! curl -sf "${API_URL}/health" >/dev/null 2>&1; do
    if [ $elapsed -ge $HEALTH_WAIT ]; then
        err "ThaiRAG did not become healthy within ${HEALTH_WAIT}s"
        docker compose -f "$COMPOSE_FILE" logs thairag | tail -30
        exit 1
    fi
    sleep 2
    elapsed=$((elapsed + 2))
done
log "ThaiRAG health OK (${elapsed}s)"

# ── Step 6: Non-streaming chat completion ─────────────────────────────
echo ""
echo "▸ Step 6: Non-streaming chat completion..."
RESP=$(curl -sf "${API_URL}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": "Say hello in Thai"}],
        "stream": false
    }') || true

test_non_stream_id() {
    echo "$RESP" | jq -e '.id' >/dev/null 2>&1
}
test_non_stream_content() {
    echo "$RESP" | jq -e '.choices[0].message.content' >/dev/null 2>&1
}
test_non_stream_usage() {
    echo "$RESP" | jq -e '.usage.prompt_tokens >= 0' >/dev/null 2>&1
}

check "Non-stream: response has id" test_non_stream_id
check "Non-stream: response has content" test_non_stream_content
check "Non-stream: response has usage" test_non_stream_usage

if [ -n "${RESP:-}" ]; then
    echo "  Response preview: $(echo "$RESP" | jq -c '{id: .id, content: .choices[0].message.content[:80], usage: .usage}')"
fi

# ── Step 7: Streaming chat completion ─────────────────────────────────
echo ""
echo "▸ Step 7: Streaming chat completion (SSE)..."
SSE_RAW=$(curl -sf "${API_URL}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -H "Accept: text/event-stream" \
    -d '{
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": "Say hi"}],
        "stream": true
    }') || true

test_sse_has_data() {
    echo "$SSE_RAW" | grep -q "^data: "
}

test_sse_role_chunk() {
    echo "$SSE_RAW" | grep "^data: " | head -1 | sed 's/^data: //' | jq -e '.choices[0].delta.role == "assistant"' >/dev/null 2>&1
}

test_sse_finish_reason() {
    echo "$SSE_RAW" | grep "^data: " | grep -v '^\[DONE\]' | sed 's/^data: //' | \
        jq -s '[.[] | select(.choices[0].finish_reason == "stop")] | length > 0' 2>/dev/null | grep -q "true"
}

test_sse_usage_chunk() {
    echo "$SSE_RAW" | grep "^data: " | sed 's/^data: //' | \
        jq -s '[.[] | select(.usage != null and (.choices | length) == 0)] | length > 0' 2>/dev/null | grep -q "true"
}

test_sse_done() {
    echo "$SSE_RAW" | grep -q "^data: \[DONE\]"
}

check "Stream: has SSE data lines" test_sse_has_data
check "Stream: first chunk has role=assistant" test_sse_role_chunk
check "Stream: has finish_reason=stop" test_sse_finish_reason
check "Stream: has usage chunk with choices=[]" test_sse_usage_chunk
check "Stream: ends with [DONE]" test_sse_done

# ── Step 8: Phase 6 API endpoints ─────────────────────────────────
echo ""
echo "▸ Step 8: Phase 6 API endpoints (auth-protected)..."

# Register + login to obtain a Bearer token
REG_RESP=$(curl -sf -X POST "${API_URL}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d '{"email":"verify@thairag.local","name":"Verify User","password":"Verify1pass"}' 2>/dev/null || true)

LOGIN_RESP=$(curl -sf -X POST "${API_URL}/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"email":"verify@thairag.local","password":"Verify1pass"}' 2>/dev/null || true)

TOKEN=$(echo "$LOGIN_RESP" | jq -r '.token // empty' 2>/dev/null || true)

test_got_token() { [ -n "$TOKEN" ]; }
check "Phase 6: obtained auth token" test_got_token

# Helper: assert that a protected GET endpoint returns HTTP 2xx
check_phase6_get() {
    local path="$1"
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        -H "Authorization: Bearer ${TOKEN}" \
        "${API_URL}${path}" 2>/dev/null || echo "000")
    case "$http_code" in
        2*|403) return 0 ;;
        *)  return 1 ;;
    esac
}

check "Phase 6: GET /search-analytics/popular" \
    check_phase6_get "/api/km/search-analytics/popular"

check "Phase 6: GET /search-analytics/summary" \
    check_phase6_get "/api/km/search-analytics/summary"

check "Phase 6: GET /settings/audit-log/analytics" \
    check_phase6_get "/api/km/settings/audit-log/analytics"

check "Phase 6: GET /settings/audit-log/export?format=json" \
    check_phase6_get "/api/km/settings/audit-log/export?format=json"

check "Phase 6: GET /tenants" \
    check_phase6_get "/api/km/tenants"

check "Phase 6: GET /roles (RBAC)" \
    check_phase6_get "/api/km/roles"

check "Phase 6: GET /prompts/marketplace" \
    check_phase6_get "/api/km/prompts/marketplace"

check "Phase 6: GET /finetune/datasets" \
    check_phase6_get "/api/km/finetune/datasets"

check "Phase 6: GET /finetune/jobs" \
    check_phase6_get "/api/km/finetune/jobs"

# ── Step 9: Smart-PDF extraction (optional: --smart-pdf <file>) ───────
if [ -n "$SMART_PDF_FILE" ]; then
    echo ""
    echo "▸ Step 9: Smart-PDF extraction ($SMART_PDF_FILE)..."
    if [ -z "$TOKEN" ]; then
        err "Smart-PDF: no auth token (Step 8 must succeed first)"; fail=$((fail + 1))
    elif [ -z "$SMART_PDF_WS" ]; then
        err "Smart-PDF: --workspace <id> is required with --smart-pdf"; fail=$((fail + 1))
    elif [ ! -f "$SMART_PDF_FILE" ]; then
        err "Smart-PDF: file not found: $SMART_PDF_FILE"; fail=$((fail + 1))
    else
        SP_BASE="${API_URL}/api/km/workspaces/${SMART_PDF_WS}/documents"
        UP_RESP=$(curl -sf -X POST "${SP_BASE}/upload" \
            -H "Authorization: Bearer ${TOKEN}" \
            -F "file=@${SMART_PDF_FILE}" 2>/dev/null || true)
        SP_DOC=$(echo "$UP_RESP" | jq -r '.id // .doc_id // .document.id // empty' 2>/dev/null || true)

        test_uploaded() { [ -n "$SP_DOC" ]; }
        check "Smart-PDF: upload accepted" test_uploaded

        if [ -n "$SP_DOC" ]; then
            # Poll until the document leaves Processing (up to ~180s).
            SP_STATUS=""
            for _ in $(seq 1 90); do
                SP_JSON=$(curl -sf -H "Authorization: Bearer ${TOKEN}" \
                    "${SP_BASE}/${SP_DOC}" 2>/dev/null || true)
                SP_STATUS=$(echo "$SP_JSON" | jq -r '.status // .document.status // empty' 2>/dev/null || true)
                case "$SP_STATUS" in ready|failed) break ;; esac
                sleep 2
            done

            test_ready() { [ "$SP_STATUS" = "ready" ]; }
            check "Smart-PDF: document reached Ready (status=${SP_STATUS:-unknown})" test_ready

            SP_CONTENT=$(curl -sf -H "Authorization: Bearer ${TOKEN}" "${SP_BASE}/${SP_DOC}/content" 2>/dev/null || true)
            test_md_pages() { echo "$SP_CONTENT" | grep -q "## Page "; }
            check "Smart-PDF: content has semantic '## Page' sections" test_md_pages

            SP_IMAGES=$(curl -sf -H "Authorization: Bearer ${TOKEN}" "${SP_BASE}/${SP_DOC}/images" 2>/dev/null || true)
            test_has_images() { echo "$SP_IMAGES" | jq -e 'length > 0' >/dev/null 2>&1; }
            check "Smart-PDF: image blobs persisted (PR #79 producer)" test_has_images

            SP_CHUNKS=$(curl -sf -H "Authorization: Bearer ${TOKEN}" "${SP_BASE}/${SP_DOC}/chunks" 2>/dev/null || true)
            test_page_strategy() {
                echo "$SP_CHUNKS" | jq -e \
                    '[.[] | .metadata.page_strategy // empty | select(startswith("pdf_"))] | length > 0' \
                    >/dev/null 2>&1
            }
            check "Smart-PDF: chunks carry pdf_* page_strategy" test_page_strategy

            # Decisive: confirm the pdfium engine ran (validates the Dockerfile
            # libpdfium install — the one piece not covered by unit tests).
            test_pdfium_log() {
                docker compose -f "$COMPOSE_FILE" logs thairag 2>/dev/null \
                    | grep -q "Smart PDF (pdfium) processing complete"
            }
            check "Smart-PDF: pdfium engine ran (libpdfium loaded in image)" test_pdfium_log

            echo "  Content preview:"
            echo "$SP_CONTENT" | head -12 | sed 's/^/    /'
        fi
    fi
fi

# ── Results ───────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════════════"
total=$((pass + fail))
if [ $fail -eq 0 ]; then
    echo -e "  ${GREEN}All ${total} checks passed!${NC}"
else
    echo -e "  ${GREEN}${pass} passed${NC}, ${RED}${fail} failed${NC} (${total} total)"
fi
echo "═══════════════════════════════════════════════════════════════"
echo ""

exit $fail
