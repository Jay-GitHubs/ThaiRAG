#!/bin/bash
# Safe Docker rebuild — always backs up the database first.
#
# Usage:
#   ./scripts/docker-rebuild.sh              # rebuild all services
#   ./scripts/docker-rebuild.sh thairag      # rebuild specific service
#   ./scripts/docker-rebuild.sh --no-backup  # skip backup (not recommended)

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SKIP_BACKUP=false
SERVICES=()

for arg in "$@"; do
  case "$arg" in
    --no-backup) SKIP_BACKUP=true ;;
    *) SERVICES+=("$arg") ;;
  esac
done

# ── Step 1: Backup ──────────────────────────────────────────────────
if [ "$SKIP_BACKUP" = false ]; then
  # Check if postgres is running
  if docker compose ps --status running 2>/dev/null | grep -q postgres; then
    echo -e "${BLUE}[1/3]${NC} Backing up database before rebuild..."
    "$SCRIPT_DIR/backup-db.sh"
    echo ""
  else
    echo -e "${YELLOW}[1/3]${NC} PostgreSQL not running — skipping backup"
  fi
else
  echo -e "${YELLOW}[1/3]${NC} Backup skipped (--no-backup)"
fi

# ── Step 2: Rebuild ─────────────────────────────────────────────────
echo -e "${BLUE}[2/3]${NC} Rebuilding Docker containers..."

# Detect compose files
COMPOSE_FILES="-f docker-compose.yml"
if [ -f docker-compose.test-idp.yml ]; then
  COMPOSE_FILES="$COMPOSE_FILES -f docker-compose.test-idp.yml"
fi

if [ ${#SERVICES[@]} -gt 0 ]; then
  echo -e "  Services: ${GREEN}${SERVICES[*]}${NC}"
  docker compose $COMPOSE_FILES up -d --build "${SERVICES[@]}"
else
  docker compose $COMPOSE_FILES up -d --build
fi

# ── Step 3: Health check ────────────────────────────────────────────
echo ""
echo -e "${BLUE}[3/3]${NC} Waiting for services to be healthy..."

# Wait for ThaiRAG backend
for i in $(seq 1 30); do
  if curl -sf http://localhost:8080/health > /dev/null 2>&1; then
    echo -e "  ${GREEN}✓${NC} ThaiRAG backend healthy"
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo -e "  ${RED}✗${NC} ThaiRAG backend not responding after 60s"
  fi
  sleep 2
done

# Verify database integrity
if docker compose ps --status running 2>/dev/null | grep -q postgres; then
  IDP_COUNT=$(docker exec thairag-postgres-1 psql -U thairag -d thairag -t -c "SELECT count(*) FROM identity_providers;" 2>/dev/null | tr -d ' ')
  USER_COUNT=$(docker exec thairag-postgres-1 psql -U thairag -d thairag -t -c "SELECT count(*) FROM users;" 2>/dev/null | tr -d ' ')
  SETTINGS_COUNT=$(docker exec thairag-postgres-1 psql -U thairag -d thairag -t -c "SELECT count(*) FROM settings;" 2>/dev/null | tr -d ' ')
  echo -e "  ${GREEN}✓${NC} Database: ${IDP_COUNT} IDPs, ${USER_COUNT} users, ${SETTINGS_COUNT} settings"

  if [ "${IDP_COUNT:-0}" -eq 0 ] && [ "${USER_COUNT:-0}" -le 1 ]; then
    echo -e "  ${RED}⚠  WARNING: Database appears empty — data may have been lost!${NC}"
    echo -e "  ${YELLOW}   Restore with: ./scripts/restore-db.sh backups/<latest>.sql.gz${NC}"
  fi
fi

echo ""
echo -e "${GREEN}Rebuild complete.${NC}"
