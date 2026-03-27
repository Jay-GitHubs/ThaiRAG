#!/usr/bin/env bash
# smoke-test.sh — End-to-end smoke test for the ThaiRAG API.
#
# Tests the full user journey across all features: health → auth → API keys →
# KM hierarchy → permissions → ACLs → documents → versioning → chat → V2 API →
# feedback → config snapshots → plugins → knowledge graph → webhooks → jobs →
# evaluation → A/B testing → backup → vector migration → rate limit stats →
# inference logs → usage → settings → error handling → rate limiting → cleanup.
#
# Usage:
#   ./scripts/smoke-test.sh [API_URL]
#
# Default API_URL: http://localhost:8080
#
# Prerequisites: curl, jq
#
# The script creates test data with a unique suffix and cleans up after itself.
# Exit code 0 = all checks passed, 1 = some failed.
set -uo pipefail

API_URL="${1:-http://localhost:8080}"
RUN_ID=$(date +%s)
TEST_EMAIL="smoke-${RUN_ID}@test.com"
TEST_PASSWORD="SmokeTest1${RUN_ID}"
TEST_EMAIL2="smoke2-${RUN_ID}@test.com"

# ── Helpers ──────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

pass=0
fail=0
total=0

log()  { echo -e "${GREEN}  ✓${NC} $*"; }
err()  { echo -e "${RED}  ✗${NC} $*"; }
info() { echo -e "${CYAN}  ➜${NC} $*"; }
section() { echo -e "\n${BOLD}━━━ $* ━━━${NC}"; }

check() {
    local desc="$1"; shift
    ((total++))
    if "$@"; then
        log "$desc"
        ((pass++))
    else
        err "FAILED: $desc"
        ((fail++))
    fi
}

assert_status() {
    local expected="$1" actual="$2"
    [ "$actual" = "$expected" ]
}

assert_json_field() {
    local json="$1" field="$2"
    echo "$json" | jq -e "$field" > /dev/null 2>&1
}

assert_json_value() {
    local json="$1" field="$2" expected="$3"
    local actual
    actual=$(echo "$json" | jq -r "$field" 2>/dev/null)
    [ "$actual" = "$expected" ]
}

assert_contains() {
    local haystack="$1" needle="$2"
    [[ "$haystack" == *"$needle"* ]]
}

# ── Cleanup on exit ──────────────────────────────────────────────────
TOKEN=""
TOKEN2=""
API_KEY=""
API_KEY_ID=""
ORG_ID=""
DEPT_ID=""
WS_ID=""
DOC_ID=""
WEBHOOK_ID=""
SNAPSHOT_ID=""
EVAL_SET_ID=""
AB_TEST_ID=""

cleanup() {
    if [ -n "$TOKEN" ]; then
        info "Cleaning up test data..."
        # Delete webhook
        [ -n "$WEBHOOK_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/km/webhooks/${WEBHOOK_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
        # Delete API key
        [ -n "$API_KEY_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/auth/api-keys/${API_KEY_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
        # Delete snapshot
        [ -n "$SNAPSHOT_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/km/settings/snapshots/${SNAPSHOT_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
        # Delete eval set
        [ -n "$EVAL_SET_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/km/eval/query-sets/${EVAL_SET_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
        # Delete A/B test
        [ -n "$AB_TEST_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/km/ab-tests/${AB_TEST_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
        # Delete document
        [ -n "$DOC_ID" ] && [ -n "$WS_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
        # Delete org (cascade deletes dept, workspace, docs)
        [ -n "$ORG_ID" ] && \
            curl -sf -X DELETE "${API_URL}/api/km/orgs/${ORG_ID}" \
                -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

# ══════════════════════════════════════════════════════════════════════
echo ""
echo -e "${BOLD}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BOLD}  ThaiRAG End-to-End Smoke Test${NC}"
echo -e "${BOLD}  API: ${API_URL}${NC}"
echo -e "${BOLD}═══════════════════════════════════════════════════════════════${NC}"

# ── 1. Health Check ──────────────────────────────────────────────────
section "1. Health Check"

RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/health" 2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')

check "GET /health returns 200" assert_status "200" "$HTTP_CODE"
check "Health response has status field" assert_json_field "$BODY" '.status'

# Deep health check
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/health?deep=true" 2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET /health?deep=true returns 200" assert_status "200" "$HTTP_CODE"

# Metrics
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/metrics" 2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET /metrics returns 200" assert_status "200" "$HTTP_CODE"
check "Metrics contains http_requests_total" assert_contains "$BODY" "http_requests_total"

# Security headers
HEADERS=$(curl -sI "${API_URL}/health" 2>/dev/null)
check "X-Content-Type-Options: nosniff" assert_contains "$HEADERS" "nosniff"
check "X-Frame-Options: DENY" assert_contains "$HEADERS" "DENY"
check "Content-Security-Policy present" assert_contains "$HEADERS" "content-security-policy"
check "Referrer-Policy present" assert_contains "$HEADERS" "referrer-policy"

# ── 2. Models ────────────────────────────────────────────────────────
section "2. Models Endpoint"

RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/v1/models" 2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET /v1/models returns 200" assert_status "200" "$HTTP_CODE"
check "Models list contains ThaiRAG-1.0" assert_contains "$BODY" "ThaiRAG-1.0"

# ── 3. Authentication ────────────────────────────────────────────────
section "3. Authentication"

# Register first user (super admin)
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL}\",\"name\":\"Smoke Test\",\"password\":\"${TEST_PASSWORD}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "POST /api/auth/register returns 201" assert_status "201" "$HTTP_CODE"
check "Register returns user with email" assert_json_value "$BODY" '.email' "$TEST_EMAIL"
USER_ID=$(echo "$BODY" | jq -r '.id')

# Login
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL}\",\"password\":\"${TEST_PASSWORD}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "POST /api/auth/login returns 200" assert_status "200" "$HTTP_CODE"
check "Login returns JWT token" assert_json_field "$BODY" '.token'
check "Login returns CSRF token" assert_json_field "$BODY" '.csrf_token'
TOKEN=$(echo "$BODY" | jq -r '.token')

# Invalid password
RESP=$(curl -s -w "\n%{http_code}" -X POST "${API_URL}/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL}\",\"password\":\"wrongpassword\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Login with wrong password returns 401" assert_status "401" "$HTTP_CODE"

# Password policy (too weak)
RESP=$(curl -s -w "\n%{http_code}" -X POST "${API_URL}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d '{"email":"weak@test.com","name":"Weak","password":"abc"}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Weak password rejected (400)" assert_status "400" "$HTTP_CODE"

# Register second user
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL2}\",\"name\":\"Smoke2\",\"password\":\"${TEST_PASSWORD}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Register second user returns 201" assert_status "201" "$HTTP_CODE"
USER_ID2=$(echo "$BODY" | jq -r '.id')

# Login as second user
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL2}\",\"password\":\"${TEST_PASSWORD}\"}" \
    2>/dev/null || echo -e "\n000")
BODY=$(echo "$RESP" | sed '$d')
TOKEN2=$(echo "$BODY" | jq -r '.token')

# ── 4. KM Hierarchy ─────────────────────────────────────────────────
section "4. Knowledge Management Hierarchy"

# Create organization
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/orgs" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"SmokeOrg-${RUN_ID}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create organization returns 201" assert_status "201" "$HTTP_CODE"
ORG_ID=$(echo "$BODY" | jq -r '.id')

# List organizations
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/orgs" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List organizations returns 200" assert_status "200" "$HTTP_CODE"
check "Org list contains created org" assert_contains "$BODY" "SmokeOrg-${RUN_ID}"

# Create department
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/orgs/${ORG_ID}/depts" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"SmokeDept-${RUN_ID}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create department returns 201" assert_status "201" "$HTTP_CODE"
DEPT_ID=$(echo "$BODY" | jq -r '.id')

# Create workspace
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/orgs/${ORG_ID}/depts/${DEPT_ID}/workspaces" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"SmokeWs-${RUN_ID}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create workspace returns 201" assert_status "201" "$HTTP_CODE"
WS_ID=$(echo "$BODY" | jq -r '.id')

# ── 5. Permissions ───────────────────────────────────────────────────
section "5. Permissions"

# Grant permission to second user
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/orgs/${ORG_ID}/permissions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL2}\",\"role\":\"Viewer\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Grant Viewer permission returns 204" assert_status "204" "$HTTP_CODE"

# List permissions
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/orgs/${ORG_ID}/permissions" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List permissions returns 200" assert_status "200" "$HTTP_CODE"
check "Permission list contains second user" assert_contains "$BODY" "$TEST_EMAIL2"

# Revoke permission
RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/orgs/${ORG_ID}/permissions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${TEST_EMAIL2}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Revoke permission returns 204" assert_status "204" "$HTTP_CODE"

# ── 6. Document Ingestion ────────────────────────────────────────────
section "6. Document Ingestion"

# Ingest text document
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/workspaces/${WS_ID}/documents" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{
        \"title\": \"Smoke Test Document ${RUN_ID}\",
        \"content\": \"ThaiRAG is an AI-powered Retrieval Augmented Generation system specialized for Thai language documents. It supports hybrid search combining vector similarity and BM25 keyword matching, with configurable reranking. The system processes documents through chunking, embedding, and indexing pipelines.\",
        \"mime_type\": \"text/plain\"
    }" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Ingest text document returns 201" assert_status "201" "$HTTP_CODE"
check "Ingest returns doc_id" assert_json_field "$BODY" '.doc_id'
check "Ingest returns chunks count" assert_json_field "$BODY" '.chunks'
DOC_ID=$(echo "$BODY" | jq -r '.doc_id')

# List documents
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/documents" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List documents returns 200" assert_status "200" "$HTTP_CODE"
check "Document list contains ingested doc" assert_contains "$BODY" "Smoke Test Document"

# Get document
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Get document returns 200" assert_status "200" "$HTTP_CODE"
check "Document has correct title" assert_contains "$BODY" "Smoke Test Document"

# Ingest markdown document
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/workspaces/${WS_ID}/documents" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{
        \"title\": \"Markdown Test ${RUN_ID}\",
        \"content\": \"# Test Heading\\n\\nThis is a **markdown** document with a [link](http://example.com).\\n\\n- Item 1\\n- Item 2\",
        \"mime_type\": \"text/markdown\"
    }" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Ingest markdown document returns 201" assert_status "201" "$HTTP_CODE"

# ── 7. Chat Completions ─────────────────────────────────────────────
section "7. Chat Completions"

# Non-streaming chat
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/v1/chat/completions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": "Hello, what is ThaiRAG?"}],
        "stream": false
    }' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Non-streaming chat returns 200" assert_status "200" "$HTTP_CODE"
check "Chat response has id" assert_json_field "$BODY" '.id'
check "Chat response has choices" assert_json_field "$BODY" '.choices[0].message.content'
check "Chat response has usage" assert_json_field "$BODY" '.usage.total_tokens'
check "Chat model is ThaiRAG-1.0" assert_json_value "$BODY" '.model' "ThaiRAG-1.0"

# Streaming chat
STREAM_RESP=$(curl -sf -N -X POST "${API_URL}/v1/chat/completions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    }' \
    2>/dev/null || echo "")
check "Streaming chat returns data" [ -n "$STREAM_RESP" ]
check "Stream contains role chunk" assert_contains "$STREAM_RESP" '"role":"assistant"'
check "Stream contains finish_reason stop" assert_contains "$STREAM_RESP" '"finish_reason":"stop"'
check "Stream ends with [DONE]" assert_contains "$STREAM_RESP" "[DONE]"

# Chat with session (multi-turn)
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/v1/chat/completions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": "Remember: my name is SmokeTest"}],
        "stream": false
    }' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Session chat returns 200" assert_status "200" "$HTTP_CODE"

# Validation: empty messages
RESP=$(curl -s -w "\n%{http_code}" -X POST "${API_URL}/v1/chat/completions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"model": "ThaiRAG-1.0", "messages": []}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Empty messages returns 400" assert_status "400" "$HTTP_CODE"

# Validation: wrong model
RESP=$(curl -s -w "\n%{http_code}" -X POST "${API_URL}/v1/chat/completions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"model": "gpt-4", "messages": [{"role":"user","content":"hi"}]}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Wrong model returns 400" assert_status "400" "$HTTP_CODE"

# ── 8. Users ─────────────────────────────────────────────────────────
section "8. User Management"

RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/users" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List users returns 200" assert_status "200" "$HTTP_CODE"
check "User list contains test user" assert_contains "$BODY" "$TEST_EMAIL"

# Update user role
RESP=$(curl -sf -w "\n%{http_code}" -X PUT "${API_URL}/api/km/users/${USER_ID2}/role" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"role":"editor"}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Update user role returns 200" assert_status "200" "$HTTP_CODE"

# Delete second user
RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/users/${USER_ID2}" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Delete user returns 204" assert_status "204" "$HTTP_CODE"

# ── 9. API Key Management ────────────────────────────────────────────
section "9. API Key Authentication"

# Create API key
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/auth/api-keys" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"smoke-key-${RUN_ID}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create API key returns 201" assert_status "201" "$HTTP_CODE"
check "API key has trag_ prefix" assert_contains "$BODY" "trag_"
API_KEY=$(echo "$BODY" | jq -r '.key // .api_key // empty')
API_KEY_ID=$(echo "$BODY" | jq -r '.id // .key_id // empty')

# List API keys
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/auth/api-keys" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List API keys returns 200" assert_status "200" "$HTTP_CODE"
check "API key list contains created key" assert_contains "$BODY" "smoke-key-${RUN_ID}"

# Authenticate with API key (if key was returned)
if [ -n "$API_KEY" ]; then
    RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/v1/models" \
        -H "X-API-Key: $API_KEY" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "X-API-Key auth returns 200" assert_status "200" "$HTTP_CODE"
fi

# Revoke API key
if [ -n "$API_KEY_ID" ]; then
    RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/auth/api-keys/${API_KEY_ID}" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Revoke API key returns 204" assert_status "204" "$HTTP_CODE"
    API_KEY_ID=""  # prevent double-delete
fi

# ── 10. Settings (super admin) ───────────────────────────────────────
section "10. Settings & Audit Log"

# Audit log
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/audit-log?limit=50" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET audit log returns 200" assert_status "200" "$HTTP_CODE"
check "Audit log contains login events" assert_contains "$BODY" "login"
check "Audit log contains register events" assert_contains "$BODY" "register"
check "Audit log contains permission events" assert_contains "$BODY" "permission_granted"

# Provider config
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/providers" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET provider config returns 200" assert_status "200" "$HTTP_CODE"

# Chat pipeline config
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/chat-pipeline" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET chat pipeline config returns 200" assert_status "200" "$HTTP_CODE"

# Document config
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/document" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET document config returns 200" assert_status "200" "$HTTP_CODE"

# Prompts
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/prompts" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET prompts returns 200" assert_status "200" "$HTTP_CODE"

# Identity providers
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/identity-providers" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET identity providers returns 200" assert_status "200" "$HTTP_CODE"

# Presets
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/presets" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET presets returns 200" assert_status "200" "$HTTP_CODE"

# Scope info
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/scope-info" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET scope info returns 200" assert_status "200" "$HTTP_CODE"

# Vector DB info
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/vectordb/info" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET vectordb info returns 200" assert_status "200" "$HTTP_CODE"

# ── 11. Config Snapshots ─────────────────────────────────────────────
section "11. Config Snapshots"

# Create snapshot
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/settings/snapshots" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"smoke-snapshot-${RUN_ID}\",\"description\":\"Smoke test snapshot\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create snapshot returns 201" assert_status "201" "$HTTP_CODE"
SNAPSHOT_ID=$(echo "$BODY" | jq -r '.id // empty')

# List snapshots
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/snapshots" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List snapshots returns 200" assert_status "200" "$HTTP_CODE"
check "Snapshot list contains created snapshot" assert_contains "$BODY" "smoke-snapshot-${RUN_ID}"

# Delete snapshot
if [ -n "$SNAPSHOT_ID" ]; then
    RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/settings/snapshots/${SNAPSHOT_ID}" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Delete snapshot returns 200 or 204" [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "204" ]
    SNAPSHOT_ID=""  # prevent double-delete
fi

# ── 12. Feedback ─────────────────────────────────────────────────────
section "12. Feedback System"

# Submit feedback
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/v1/chat/feedback" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{
        \"response_id\": \"smoke-${RUN_ID}\",
        \"thumbs_up\": true,
        \"query\": \"test query\",
        \"answer\": \"test answer\",
        \"workspace_id\": \"${WS_ID}\"
    }" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Submit feedback returns 200" assert_status "200" "$HTTP_CODE"

# Feedback stats
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/feedback/stats" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET feedback stats returns 200" assert_status "200" "$HTTP_CODE"

# Feedback entries
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/feedback/entries?page=1&per_page=10" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET feedback entries returns 200" assert_status "200" "$HTTP_CODE"

# Document boosts
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/feedback/document-boosts" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET document boosts returns 200" assert_status "200" "$HTTP_CODE"

# Golden examples
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/feedback/golden-examples" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET golden examples returns 200" assert_status "200" "$HTTP_CODE"

# Retrieval params
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/feedback/retrieval-params" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET retrieval params returns 200" assert_status "200" "$HTTP_CODE"

# ── 13. V2 API ───────────────────────────────────────────────────────
section "13. API v2 Endpoints"

# V2 Models
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/v2/models" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET /v2/models returns 200" assert_status "200" "$HTTP_CODE"
check "V2 models contains ThaiRAG-1.0" assert_contains "$BODY" "ThaiRAG-1.0"

# API version info
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/version" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET /api/version returns 200" assert_status "200" "$HTTP_CODE"

# V2 Chat (non-streaming)
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/v2/chat/completions" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "ThaiRAG-1.0",
        "messages": [{"role": "user", "content": "Hello from V2"}],
        "stream": false
    }' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "V2 non-streaming chat returns 200" assert_status "200" "$HTTP_CODE"
check "V2 chat has choices" assert_json_field "$BODY" '.choices[0].message.content'

# ── 14. Plugins ──────────────────────────────────────────────────────
section "14. Plugin System"

# List plugins
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/plugins" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET plugins returns 200" assert_status "200" "$HTTP_CODE"

# Enable plugin
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/plugins/metadata-strip/enable" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Enable plugin returns 200" assert_status "200" "$HTTP_CODE"

# Disable plugin
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/plugins/metadata-strip/disable" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Disable plugin returns 200" assert_status "200" "$HTTP_CODE"

# ── 15. Webhooks ─────────────────────────────────────────────────────
section "15. Webhooks"

# Create webhook
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/webhooks" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{
        \"name\": \"smoke-webhook-${RUN_ID}\",
        \"url\": \"https://httpbin.org/post\",
        \"events\": [\"document.created\"],
        \"secret\": \"smoke-secret\"
    }" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create webhook returns 201" assert_status "201" "$HTTP_CODE"
WEBHOOK_ID=$(echo "$BODY" | jq -r '.id // empty')

# List webhooks
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/webhooks" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "List webhooks returns 200" assert_status "200" "$HTTP_CODE"
check "Webhook list contains created webhook" assert_contains "$BODY" "smoke-webhook-${RUN_ID}"

# Delete webhook
if [ -n "$WEBHOOK_ID" ]; then
    RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/webhooks/${WEBHOOK_ID}" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Delete webhook returns 204" assert_status "204" "$HTTP_CODE"
    WEBHOOK_ID=""  # prevent double-delete
fi

# ── 16. Document Versioning & Reprocessing ────────────────────────────
section "16. Document Versioning & Chunks"

# Get document chunks
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}/chunks" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET document chunks returns 200" assert_status "200" "$HTTP_CODE"

# Get document content
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}/content" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET document content returns 200" assert_status "200" "$HTTP_CODE"

# List document versions
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}/versions" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "List document versions returns 200" assert_status "200" "$HTTP_CODE"

# Reprocess document
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}/reprocess" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Reprocess document returns 200" assert_status "200" "$HTTP_CODE"

# ── 17. ACLs ─────────────────────────────────────────────────────────
section "17. Access Control Lists"

# Grant workspace ACL (re-register second user since we deleted them)
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"acl-${RUN_ID}@test.com\",\"name\":\"ACL User\",\"password\":\"${TEST_PASSWORD}\"}" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
ACL_USER_ID=$(echo "$BODY" | jq -r '.id // empty')

if [ -n "$ACL_USER_ID" ]; then
    # Grant workspace ACL
    RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/workspaces/${WS_ID}/acl" \
        -H "Authorization: Bearer $TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"user_id\":\"${ACL_USER_ID}\",\"permission\":\"read\"}" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Grant workspace ACL returns 200 or 201" [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "201" ]

    # List workspace ACLs
    RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/acl" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "List workspace ACLs returns 200" assert_status "200" "$HTTP_CODE"

    # Revoke workspace ACL
    RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/workspaces/${WS_ID}/acl/${ACL_USER_ID}" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Revoke workspace ACL returns 200 or 204" [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "204" ]

    # Cleanup ACL user
    curl -sf -X DELETE "${API_URL}/api/km/users/${ACL_USER_ID}" \
        -H "Authorization: Bearer $TOKEN" > /dev/null 2>&1 || true
fi

# ── 18. Knowledge Graph ──────────────────────────────────────────────
section "18. Knowledge Graph"

# Get knowledge graph
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/knowledge-graph" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET knowledge graph returns 200" assert_status "200" "$HTTP_CODE"

# List entities
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/entities" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "List entities returns 200" assert_status "200" "$HTTP_CODE"

# ── 19. Background Jobs ──────────────────────────────────────────────
section "19. Background Jobs"

# List jobs
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/workspaces/${WS_ID}/jobs" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "List jobs returns 200" assert_status "200" "$HTTP_CODE"

# ── 20. Evaluation & A/B Testing ─────────────────────────────────────
section "20. Evaluation & A/B Testing"

# Create evaluation query set
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/eval/query-sets" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{
        \"name\": \"smoke-eval-${RUN_ID}\",
        \"queries\": [{\"query\": \"test question\", \"expected_answer\": \"test answer\"}]
    }" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create eval query set returns 201" assert_status "201" "$HTTP_CODE"
EVAL_SET_ID=$(echo "$BODY" | jq -r '.id // empty')

# List evaluation query sets
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/eval/query-sets" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "List eval query sets returns 200" assert_status "200" "$HTTP_CODE"

# Delete eval set
if [ -n "$EVAL_SET_ID" ]; then
    RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/eval/query-sets/${EVAL_SET_ID}" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Delete eval query set returns 200 or 204" [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "204" ]
    EVAL_SET_ID=""  # prevent double-delete
fi

# Create A/B test
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/ab-tests" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{
        \"name\": \"smoke-ab-${RUN_ID}\",
        \"config_a\": {\"top_k\": 5},
        \"config_b\": {\"top_k\": 10}
    }" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Create A/B test returns 201" assert_status "201" "$HTTP_CODE"
AB_TEST_ID=$(echo "$BODY" | jq -r '.id // empty')

# List A/B tests
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/ab-tests" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "List A/B tests returns 200" assert_status "200" "$HTTP_CODE"

# Delete A/B test
if [ -n "$AB_TEST_ID" ]; then
    RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/ab-tests/${AB_TEST_ID}" \
        -H "Authorization: Bearer $TOKEN" \
        2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    check "Delete A/B test returns 200 or 204" [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "204" ]
    AB_TEST_ID=""  # prevent double-delete
fi

# ── 21. Backup & Restore ─────────────────────────────────────────────
section "21. Backup & Restore"

# Preview backup
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/admin/backup/preview" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Backup preview returns 200" assert_status "200" "$HTTP_CODE"

# Create backup
RESP=$(curl -sf -w "\n%{http_code}" -X POST "${API_URL}/api/km/admin/backup" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Create backup returns 200" assert_status "200" "$HTTP_CODE"

# ── 22. Vector Migration Status ──────────────────────────────────────
section "22. Vector Migration"

# Get migration status
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/admin/vector-migration/status" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "GET migration status returns 200" assert_status "200" "$HTTP_CODE"
check "Migration status has state field" assert_json_field "$BODY" '.state'

# ── 23. Rate Limit Stats ─────────────────────────────────────────────
section "23. Rate Limit Dashboard"

# Get rate limit stats
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/admin/rate-limits/stats" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET rate limit stats returns 200" assert_status "200" "$HTTP_CODE"

# Get blocked events
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/admin/rate-limits/blocked" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET blocked events returns 200" assert_status "200" "$HTTP_CODE"

# ── 24. Inference Logs & Usage ────────────────────────────────────────
section "24. Inference Logs & Usage"

# Inference logs
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/inference-logs" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET inference logs returns 200" assert_status "200" "$HTTP_CODE"

# Inference analytics
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/inference-analytics" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET inference analytics returns 200" assert_status "200" "$HTTP_CODE"

# Usage stats
RESP=$(curl -sf -w "\n%{http_code}" "${API_URL}/api/km/settings/usage" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "GET usage stats returns 200" assert_status "200" "$HTTP_CODE"

# ── 25. Error Handling ───────────────────────────────────────────────
section "25. Error Handling & Edge Cases"

# 404 — nonexistent org
RESP=$(curl -s -w "\n%{http_code}" "${API_URL}/api/km/orgs/00000000-0000-0000-0000-000000000000" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Nonexistent org returns 404" assert_status "404" "$HTTP_CODE"

# Unsupported MIME type
RESP=$(curl -s -w "\n%{http_code}" -X POST "${API_URL}/api/km/workspaces/${WS_ID}/documents" \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"title":"bad","content":"test","mime_type":"application/zip"}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Unsupported MIME type returns 400" assert_status "400" "$HTTP_CODE"

# Missing required fields
RESP=$(curl -s -w "\n%{http_code}" -X POST "${API_URL}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d '{"email":"","name":"","password":""}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Empty fields returns 400" assert_status "400" "$HTTP_CODE"

# Unauthorized access
RESP=$(curl -s -w "\n%{http_code}" "${API_URL}/api/km/settings/providers" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Settings without auth returns 401" assert_status "401" "$HTTP_CODE"

# ── 26. Rate Limiting ────────────────────────────────────────────────
section "26. Rate Limiting"

RATE_LIMIT_HIT=false
for i in $(seq 1 50); do
    RESP=$(curl -s -w "\n%{http_code}" "${API_URL}/health" 2>/dev/null || echo -e "\n000")
    HTTP_CODE=$(echo "$RESP" | tail -1)
    if [ "$HTTP_CODE" = "429" ]; then
        RATE_LIMIT_HIT=true
        break
    fi
done
# Rate limiting may or may not trigger depending on config; just check the endpoint works
check "Health endpoint survives rapid requests" [ "$HTTP_CODE" = "200" ] || [ "$RATE_LIMIT_HIT" = "true" ]

# ── 27. Cleanup ──────────────────────────────────────────────────────
section "27. Cleanup Verification"

# Delete document
RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/workspaces/${WS_ID}/documents/${DOC_ID}" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Delete document returns 204" assert_status "204" "$HTTP_CODE"
DOC_ID=""  # prevent double-delete in trap

# Delete org (cascade)
RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/orgs/${ORG_ID}" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Delete org (cascade) returns 204" assert_status "204" "$HTTP_CODE"
ORG_ID=""  # prevent double-delete in trap

# Verify cascade worked
RESP=$(curl -s -w "\n%{http_code}" "${API_URL}/api/km/orgs" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
check "Org no longer in list after cascade delete" ! assert_contains "$BODY" "SmokeOrg-${RUN_ID}"

# ══════════════════════════════════════════════════════════════════════
echo ""
echo -e "${BOLD}═══════════════════════════════════════════════════════════════${NC}"
if [ "$fail" -eq 0 ]; then
    echo -e "  ${GREEN}ALL ${pass}/${total} CHECKS PASSED${NC}"
else
    echo -e "  ${GREEN}${pass} passed${NC}, ${RED}${fail} failed${NC} out of ${total} checks"
fi
echo -e "${BOLD}═══════════════════════════════════════════════════════════════${NC}"
echo ""

[ "$fail" -eq 0 ] && exit 0 || exit 1
