#!/usr/bin/env bash
# Nightly backup, container-side. Mirrors scripts/backup-db.sh (full dump +
# key-table JSON + Qdrant snapshot + rotation) but runs INSIDE the stack so
# macOS TCC can never block it. Writes the same /backups dir the host script
# uses, and appends a BACKUP-VERIFY line to backups/cron.log so the existing
# "grep BACKUP-VERIFY" verification convention keeps working.
#
# Usage: backup-loop.sh          # sleep→fire daily at $BACKUP_AT (default 03:30)
#        backup-loop.sh once     # run a single backup now and exit (live tests)
set -uo pipefail

BACKUP_DIR="${BACKUP_DIR:-/backups}"
MAX_BACKUPS="${MAX_BACKUPS:-10}"
BACKUP_AT="${BACKUP_AT:-03:30}"
QDRANT_URL="${QDRANT_URL:-http://qdrant:6333}"
QDRANT_COLLECTION="${QDRANT_COLLECTION:-thairag_chunks}"
JSON_TABLES=(identity_providers settings users organizations permissions)
LOG="$BACKUP_DIR/cron.log"

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') [backup] $*" | tee -a "$LOG"; }

run_backup() {
    local ts file json_dir ok=true
    ts="$(date '+%Y-%m-%d-%H%M%S')"
    file="$BACKUP_DIR/thairag-${ts}.sql.gz"
    json_dir="$BACKUP_DIR/thairag-${ts}-tables"

    log "starting nightly backup ${ts}"

    # 1) Full dump (pg_dump connects directly; PGHOST/PGUSER/PGPASSWORD/
    #    PGDATABASE come from the environment).
    if pg_dump | gzip > "$file" && [ "$(stat -c%s "$file")" -gt 100000 ]; then
        log "full dump: $(basename "$file") ($(stat -c%s "$file") bytes)"
    else
        ok=false
        log "ERROR: pg_dump failed or dump implausibly small"
    fi

    # 2) Key tables as JSON (quick single-table restores / inspection).
    mkdir -p "$json_dir"
    for t in "${JSON_TABLES[@]}"; do
        if [ "$(psql -tAc "SELECT to_regclass('${t}') IS NOT NULL")" = "t" ]; then
            psql -tAc "SELECT json_agg(x) FROM ${t} x;" > "$json_dir/${t}.json" 2>>"$LOG" \
                || log "WARN: JSON export failed for ${t}"
        fi
    done

    # 3) Qdrant snapshot (belt-and-braces; reindex-from-chunks stays the
    #    canonical vector recovery).
    if curl -sf -m 5 "${QDRANT_URL}/collections/${QDRANT_COLLECTION}" >/dev/null; then
        local snap
        snap=$(curl -sf -m 120 -X POST \
            "${QDRANT_URL}/collections/${QDRANT_COLLECTION}/snapshots" \
            | sed -n 's/.*"name":"\([^"]*\)".*/\1/p')
        if [ -n "$snap" ] && curl -sf -m 600 \
            "${QDRANT_URL}/collections/${QDRANT_COLLECTION}/snapshots/${snap}" \
            -o "$BACKUP_DIR/${ts}-qdrant.snapshot"; then
            log "qdrant snapshot: ${ts}-qdrant.snapshot"
            curl -sf -m 30 -X DELETE \
                "${QDRANT_URL}/collections/${QDRANT_COLLECTION}/snapshots/${snap}" \
                >/dev/null || true
        else
            log "WARN: qdrant snapshot failed — dump still valid; vectors recoverable via reindex"
        fi
    else
        log "WARN: qdrant unreachable — skipping vector snapshot"
    fi

    # 4) Rotation: keep the newest MAX_BACKUPS dumps; delete each old dump's
    #    tables dir and qdrant snapshot with it (the host script leaked
    #    snapshots on rotation — fixed here).
    ls -1t "$BACKUP_DIR"/thairag-*.sql.gz 2>/dev/null | tail -n "+$((MAX_BACKUPS + 1))" \
    | while read -r old; do
        local base stamp
        base="$(basename "$old" .sql.gz)"     # thairag-YYYY-MM-DD-HHMMSS
        stamp="${base#thairag-}"
        rm -f "$old" "$BACKUP_DIR/${stamp}-qdrant.snapshot"
        rm -rf "$BACKUP_DIR/${base}-tables"
        log "rotated out ${base}"
    done

    # 5) Self-verify line — same convention the host cron used, so
    #    `grep BACKUP-VERIFY backups/cron.log` answers "did last night run?".
    if $ok; then
        log "BACKUP-VERIFY OK $(basename "$file") ($(stat -c%s "$file") bytes)"
    else
        log "BACKUP-VERIFY FAIL: see errors above"
    fi
}

if [ "${1:-}" = "once" ]; then
    run_backup
    exit 0
fi

log "scheduler up — daily at ${BACKUP_AT} (${TZ:-UTC}), keeping ${MAX_BACKUPS}"
while true; do
    now=$(date +%s)
    target=$(date -d "$(date '+%Y-%m-%d') ${BACKUP_AT}" +%s)
    [ "$target" -le "$now" ] && target=$((target + 86400))
    sleep $((target - now))
    run_backup
done
