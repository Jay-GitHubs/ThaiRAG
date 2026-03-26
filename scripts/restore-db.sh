#!/usr/bin/env bash
set -euo pipefail

# ── Colors ────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# ── Config ────────────────────────────────────────────────────────────────
CONTAINER="thairag-postgres-1"
DB_USER="thairag"
DB_NAME="thairag"

# ── Helpers ───────────────────────────────────────────────────────────────
info()    { echo -e "${BLUE}[INFO]${NC}  $*"; }
success() { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error()   { echo -e "${RED}[ERR]${NC}   $*" >&2; }

die() {
    error "$@"
    exit 1
}

usage() {
    echo -e "${BOLD}Usage:${NC} $0 <backup-file.sql.gz>"
    echo ""
    echo "  Restores the ThaiRAG PostgreSQL database from a backup file."
    echo ""
    echo -e "${BOLD}Arguments:${NC}"
    echo "  backup-file.sql.gz   Path to a gzipped pg_dump file"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo "  $0 backups/thairag-2026-03-26-160000.sql.gz"
    echo "  $0 /path/to/thairag-backup.sql.gz"
    exit 1
}

# ── Validate arguments ───────────────────────────────────────────────────
if [[ $# -lt 1 ]]; then
    usage
fi

BACKUP_FILE="$1"

if [[ ! -f "$BACKUP_FILE" ]]; then
    die "Backup file not found: ${BACKUP_FILE}"
fi

if [[ "$BACKUP_FILE" != *.sql.gz ]]; then
    die "Expected a .sql.gz file, got: $(basename "$BACKUP_FILE")"
fi

# ── Preflight checks ─────────────────────────────────────────────────────
info "Checking prerequisites..."

if ! command -v docker &>/dev/null; then
    die "docker is not installed or not in PATH."
fi

if ! docker ps --format '{{.Names}}' | grep -q "^${CONTAINER}$"; then
    die "Container '${CONTAINER}' is not running. Start it with: docker compose up -d postgres"
fi

# ── Show backup details ─────────────────────────────────────────────────
FILE_SIZE="$(du -h "$BACKUP_FILE" | cut -f1)"
FILE_DATE="$(stat -f '%Sm' -t '%Y-%m-%d %H:%M:%S' "$BACKUP_FILE" 2>/dev/null \
    || stat -c '%y' "$BACKUP_FILE" 2>/dev/null | cut -d. -f1)"

echo ""
echo -e "${BOLD}${YELLOW}╔══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${YELLOW}║               DATABASE RESTORE                           ║${NC}"
echo -e "${BOLD}${YELLOW}╚══════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  ${BOLD}File:${NC}       $(basename "$BACKUP_FILE")"
echo -e "  ${BOLD}Size:${NC}       ${FILE_SIZE}"
echo -e "  ${BOLD}Modified:${NC}   ${FILE_DATE}"
echo -e "  ${BOLD}Target DB:${NC}  ${DB_NAME} @ ${CONTAINER}"
echo ""

# Count tables currently in the database
current_tables=$(docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -tAc \
    "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public';")
info "Current database has ${current_tables} tables in public schema."

echo ""
echo -e "${RED}${BOLD}WARNING: This will DROP and recreate the '${DB_NAME}' database.${NC}"
echo -e "${RED}${BOLD}All current data will be permanently lost.${NC}"
echo ""
read -rp "$(echo -e "${CYAN}Type 'yes' to confirm restore:${NC} ")" confirm

if [[ "$confirm" != "yes" ]]; then
    info "Restore cancelled."
    exit 0
fi

echo ""

# ── Stop dependent services ─────────────────────────────────────────────
info "Checking for active connections..."

# Terminate all connections to the target database (except our own)
docker exec "$CONTAINER" psql -U "$DB_USER" -d postgres -c \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${DB_NAME}' AND pid <> pg_backend_pid();" \
    >/dev/null 2>&1 || true

success "Active connections terminated."

# ── Drop and recreate database ───────────────────────────────────────────
info "Dropping and recreating database '${DB_NAME}'..."

docker exec "$CONTAINER" psql -U "$DB_USER" -d postgres -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null
docker exec "$CONTAINER" psql -U "$DB_USER" -d postgres -c "CREATE DATABASE ${DB_NAME} OWNER ${DB_USER};" >/dev/null

success "Database recreated."

# ── Restore from backup ─────────────────────────────────────────────────
info "Restoring from $(basename "$BACKUP_FILE")..."

gunzip -c "$BACKUP_FILE" | docker exec -i "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" --quiet >/dev/null 2>&1

success "Database restored."

# ── Verify restore ───────────────────────────────────────────────────────
info "Verifying restore..."

restored_tables=$(docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -tAc \
    "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public';")

echo ""
echo -e "${GREEN}${BOLD}Restore complete!${NC}"
echo -e "  Tables in restored database: ${BOLD}${restored_tables}${NC}"
echo ""

# Show table names and row counts
info "Table summary:"
docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -c \
    "SELECT schemaname || '.' || relname AS table_name, n_live_tup AS row_count
     FROM pg_stat_user_tables ORDER BY relname;" 2>/dev/null || true

echo ""
warn "If ThaiRAG is running, restart it to pick up restored data:"
echo -e "  ${CYAN}docker compose restart thairag${NC}"
echo ""
