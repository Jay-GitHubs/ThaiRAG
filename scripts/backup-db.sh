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
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKUP_DIR="$PROJECT_ROOT/backups"
CONTAINER="thairag-postgres-1"
DB_USER="thairag"
DB_NAME="thairag"
MAX_BACKUPS=10
TIMESTAMP="$(date '+%Y-%m-%d-%H%M%S')"
BACKUP_FILE="$BACKUP_DIR/thairag-${TIMESTAMP}.sql.gz"
JSON_DIR="$BACKUP_DIR/thairag-${TIMESTAMP}-tables"
JSON_TABLES=(identity_providers settings users organizations permissions)

# ── Helpers ───────────────────────────────────────────────────────────────
info()    { echo -e "${BLUE}[INFO]${NC}  $*"; }
success() { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error()   { echo -e "${RED}[ERR]${NC}   $*" >&2; }

die() {
    error "$@"
    exit 1
}

# ── Pre-rebuild mode ─────────────────────────────────────────────────────
PRE_REBUILD=false
if [[ "${1:-}" == "--pre-rebuild" ]]; then
    PRE_REBUILD=true
fi

if $PRE_REBUILD; then
    echo ""
    echo -e "${BOLD}${YELLOW}╔══════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}${YELLOW}║           PRE-REBUILD DATABASE BACKUP                    ║${NC}"
    echo -e "${BOLD}${YELLOW}╚══════════════════════════════════════════════════════════╝${NC}"
    echo ""
    warn "You are about to create a backup before rebuilding containers."
    warn "Make sure the database is in a consistent state."
    echo ""
    read -rp "$(echo -e "${CYAN}Proceed with backup? [y/N]:${NC} ")" confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        info "Backup cancelled."
        exit 0
    fi
    echo ""
fi

# ── Preflight checks ─────────────────────────────────────────────────────
info "Checking prerequisites..."

if ! command -v docker &>/dev/null; then
    die "docker is not installed or not in PATH."
fi

if ! docker ps --format '{{.Names}}' | grep -q "^${CONTAINER}$"; then
    die "Container '${CONTAINER}' is not running. Start it with: docker compose up -d postgres"
fi

# ── Create backup directory ──────────────────────────────────────────────
mkdir -p "$BACKUP_DIR"
success "Backup directory: ${BACKUP_DIR}"

# ── Full database dump ───────────────────────────────────────────────────
info "Dumping full database to ${BOLD}$(basename "$BACKUP_FILE")${NC} ..."

docker exec "$CONTAINER" pg_dump -U "$DB_USER" "$DB_NAME" \
    | gzip > "$BACKUP_FILE"

DUMP_SIZE="$(du -h "$BACKUP_FILE" | cut -f1)"
success "Full dump complete (${DUMP_SIZE})"

# ── Export key tables as JSON ────────────────────────────────────────────
info "Exporting key tables as JSON..."
mkdir -p "$JSON_DIR"

for table in "${JSON_TABLES[@]}"; do
    json_file="$JSON_DIR/${table}.json"

    # Check if table exists before exporting
    table_exists=$(docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -tAc \
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = '${table}');")

    if [[ "$table_exists" == "t" ]]; then
        docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -tAc \
            "SELECT json_agg(t) FROM ${table} t;" > "$json_file"

        row_count=$(docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -tAc \
            "SELECT count(*) FROM ${table};")
        success "  ${table}: ${row_count} rows -> $(basename "$json_file")"
    else
        warn "  ${table}: table does not exist, skipping"
    fi
done

# ── Rotate old backups ───────────────────────────────────────────────────
info "Rotating old backups (keeping last ${MAX_BACKUPS})..."

# Count and remove old .sql.gz files
backup_files=()
while IFS= read -r f; do
    backup_files+=("$f")
done < <(ls -1t "$BACKUP_DIR"/thairag-*.sql.gz 2>/dev/null)

if (( ${#backup_files[@]} > MAX_BACKUPS )); then
    removed=0
    for (( i=MAX_BACKUPS; i<${#backup_files[@]}; i++ )); do
        old_file="${backup_files[$i]}"
        old_basename="$(basename "$old_file" .sql.gz)"
        # Remove the sql.gz file
        rm -f "$old_file"
        # Remove the corresponding JSON directory
        rm -rf "$BACKUP_DIR/${old_basename}-tables"
        ((removed++)) || true
    done
    success "Removed ${removed} old backup(s)"
else
    info "No old backups to remove (${#backup_files[@]}/${MAX_BACKUPS})"
fi

# ── Summary ──────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}${BOLD}Backup complete!${NC}"
echo -e "  Full dump:  ${CYAN}${BACKUP_FILE}${NC}"
echo -e "  JSON files: ${CYAN}${JSON_DIR}/${NC}"
echo ""

if $PRE_REBUILD; then
    echo -e "${BOLD}You can now safely rebuild. To restore later:${NC}"
    echo -e "  ${CYAN}./scripts/restore-db.sh ${BACKUP_FILE}${NC}"
    echo ""
fi
