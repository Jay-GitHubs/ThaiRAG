#!/usr/bin/env bash
# smoke-test.sh — End-to-end smoke test for the ThaiRAG API.
#
# Tests the full user journey: health → register → login → KM hierarchy →
# document ingestion → chat → permissions → audit log → cleanup.
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
ORG_ID=""
DEPT_ID=""
WS_ID=""
DOC_ID=""

cleanup() {
    if [ -n "$TOKEN" ]; then
        info "Cleaning up test data..."
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

# Delete second user
RESP=$(curl -sf -w "\n%{http_code}" -X DELETE "${API_URL}/api/km/users/${USER_ID2}" \
    -H "Authorization: Bearer $TOKEN" \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
check "Delete user returns 204" assert_status "204" "$HTTP_CODE"

# ── 9. Settings (super admin) ────────────────────────────────────────
section "9. Settings & Audit Log"

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

# ── 10. Error Handling ───────────────────────────────────────────────
section "10. Error Handling & Edge Cases"

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

# ── 11. Rate Limiting ────────────────────────────────────────────────
section "11. Rate Limiting"

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

# ── 12. Document Cleanup ────────────────────────────────────────────
section "12. Cleanup Verification"

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
