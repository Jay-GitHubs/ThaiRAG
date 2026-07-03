#!/usr/bin/env bash
# Seed the dedicated e2e workspace for the "scanned-doc inline source images"
# spec (chat-ui/e2e/parity.spec.ts). Creates the E2E-Scanned workspace under
# BTUDE/BA101, grants the playwright test user viewer access, and ingests the
# scanned-gazette fixture (vision-OCR'd → chunks carry [IMAGE:...] page
# linkage, which is what the spec asserts). Idempotent-ish: re-running on a
# stack that already has the workspace just adds a duplicate doc — check first.
#
# Usage: ./scripts/e2e-seed-scanned-workspace.sh [api-base]
set -euo pipefail
BASE="${1:-http://localhost:8080}"
ADMIN_EMAIL="${THAIRAG_ADMIN_EMAIL:-admin@thairag.local}"
ADMIN_PASSWORD="${THAIRAG_ADMIN_PASSWORD:-admin123}"
FIXTURE="tests/fixtures/thai-real/scanned_gazette_2486.pdf"
WS_NAME="E2E-Scanned"

TOKEN=$(curl -sf -X POST "$BASE/api/auth/login" -H 'Content-Type: application/json' \
  -d "{\"email\":\"$ADMIN_EMAIL\",\"password\":\"$ADMIN_PASSWORD\"}" | python3 -c "import json,sys; print(json.load(sys.stdin)['token'])")
auth=(-H "Authorization: Bearer $TOKEN")

ORG=$(curl -sf "$BASE/api/km/orgs" "${auth[@]}" | python3 -c "
import json,sys
orgs=json.load(sys.stdin)['data']
print(next(o['id'] for o in orgs if o['name']=='BTUDE'))")
DEPT=$(curl -sf "$BASE/api/km/orgs/$ORG/depts" "${auth[@]}" | python3 -c "
import json,sys; print(json.load(sys.stdin)['data'][0]['id'])")

EXISTING=$(curl -sf "$BASE/api/km/orgs/$ORG/depts/$DEPT/workspaces" "${auth[@]}" | python3 -c "
import json,sys
ws=json.load(sys.stdin)['data']
print(next((w['id'] for w in ws if w['name']=='$WS_NAME'), ''))")
if [ -n "$EXISTING" ]; then
  echo "Workspace $WS_NAME already exists ($EXISTING) — nothing to do."
  exit 0
fi

WS=$(curl -sf -X POST "$BASE/api/km/orgs/$ORG/depts/$DEPT/workspaces" "${auth[@]}" \
  -H 'Content-Type: application/json' -d "{\"name\":\"$WS_NAME\"}" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
echo "Created workspace $WS_NAME: $WS"

curl -sf -X POST "$BASE/api/km/orgs/$ORG/depts/$DEPT/workspaces/$WS/permissions" "${auth[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"email":"playwright@test.com","role":"viewer"}' -o /dev/null
echo "Granted playwright@test.com viewer."

curl -sf -X POST "$BASE/api/km/workspaces/$WS/documents/upload" "${auth[@]}" \
  -F "file=@$FIXTURE" | python3 -m json.tool
echo "Ingestion started — poll the admin UI / documents API until status=ready."
