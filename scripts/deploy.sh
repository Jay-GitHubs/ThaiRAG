#!/usr/bin/env bash
# ── ThaiRAG Production Deployment Script ─────────────────────────────────────
# Usage:
#   ./scripts/deploy.sh                 # full deploy
#   ./scripts/deploy.sh --no-backup     # skip DB backup (not recommended)
#   ./scripts/deploy.sh --no-build      # skip image rebuild (config/env changes only)
#
# Prerequisites: docker, docker compose v2, openssl

set -euo pipefail

# ── Paths ─────────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="$PROJECT_ROOT/docker-compose.prod.yml"
ENV_FILE="$PROJECT_ROOT/.env.prod"
ENV_EXAMPLE="$PROJECT_ROOT/.env.prod.example"
CERT_DIR="$PROJECT_ROOT/nginx/certs"
BACKUP_SCRIPT="$SCRIPT_DIR/backup-db.sh"

# ── Colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# ── Helpers ───────────────────────────────────────────────────────────────────
info()    { echo -e "${BLUE}[INFO]${NC}  $*"; }
success() { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error()   { echo -e "${RED}[ERR]${NC}   $*" >&2; }
die()     { error "$@"; exit 1; }

step() {
    echo ""
    echo -e "${BOLD}${BLUE}━━ $* ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

# ── Flags ────────────────────────────────────────────────────────────────────
SKIP_BACKUP=false
SKIP_BUILD=false

for arg in "$@"; do
    case "$arg" in
        --no-backup) SKIP_BACKUP=true ;;
        --no-build)  SKIP_BUILD=true  ;;
        --help|-h)
            echo "Usage: $0 [--no-backup] [--no-build]"
            echo ""
            echo "  --no-backup  Skip database backup before deploying"
            echo "  --no-build   Skip image rebuild (deploy with existing images)"
            exit 0
            ;;
        *) die "Unknown argument: $arg" ;;
    esac
done

# ── Banner ────────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}${GREEN}╔══════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${GREEN}║        ThaiRAG Production Deployment                ║${NC}"
echo -e "${BOLD}${GREEN}╚══════════════════════════════════════════════════════╝${NC}"
echo ""

# ── Step 1: Prerequisites ─────────────────────────────────────────────────────
step "1/6  Checking prerequisites"

check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        die "'$1' is not installed or not in PATH."
    fi
    success "$1 found: $(command -v "$1")"
}

check_cmd docker
check_cmd openssl

# Docker Compose v2 (plugin) or standalone
if docker compose version &>/dev/null 2>&1; then
    COMPOSE="docker compose"
    success "docker compose (plugin) found"
elif command -v docker-compose &>/dev/null; then
    COMPOSE="docker-compose"
    warn "Using legacy docker-compose binary. Upgrade to Docker Compose v2 when possible."
else
    die "docker compose is not available. Install Docker Desktop or the compose plugin."
fi

# ── Step 2: Environment file ──────────────────────────────────────────────────
step "2/6  Checking environment configuration"

if [[ ! -f "$ENV_FILE" ]]; then
    if [[ ! -f "$ENV_EXAMPLE" ]]; then
        die ".env.prod.example not found at $ENV_EXAMPLE"
    fi
    cp "$ENV_EXAMPLE" "$ENV_FILE"
    warn ".env.prod did not exist — created from template."
    warn "IMPORTANT: Edit $ENV_FILE and replace all CHANGE_ME values before continuing."
    echo ""
    read -rp "$(echo -e "${CYAN}Have you configured .env.prod? [y/N]:${NC} ")" confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        info "Deployment cancelled. Edit .env.prod and re-run."
        exit 0
    fi
fi

# Warn if any CHANGE_ME placeholders remain
if grep -q "CHANGE_ME" "$ENV_FILE" 2>/dev/null; then
    warn "Your .env.prod still contains CHANGE_ME placeholders:"
    grep "CHANGE_ME" "$ENV_FILE" | sed 's/=.*/=<HIDDEN>/' | while IFS= read -r line; do
        warn "  $line"
    done
    echo ""
    read -rp "$(echo -e "${CYAN}Continue anyway? [y/N]:${NC} ")" confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        info "Deployment cancelled."
        exit 0
    fi
fi

success ".env.prod loaded"

# ── Step 3: TLS certificates ──────────────────────────────────────────────────
step "3/6  Checking TLS certificates"

mkdir -p "$CERT_DIR"

if [[ -f "$CERT_DIR/fullchain.pem" && -f "$CERT_DIR/privkey.pem" ]]; then
    # Show expiry info
    EXPIRY=$(openssl x509 -enddate -noout -in "$CERT_DIR/fullchain.pem" 2>/dev/null \
        | sed 's/notAfter=//')
    success "Certificates found (expires: ${EXPIRY})"

    # Warn if expiring within 30 days
    if ! openssl x509 -checkend 2592000 -noout -in "$CERT_DIR/fullchain.pem" &>/dev/null; then
        warn "Certificate expires within 30 days — consider renewing."
    fi
else
    warn "No TLS certificates found at $CERT_DIR/"
    echo ""
    echo -e "  ${CYAN}Options:${NC}"
    echo -e "  ${BOLD}1)${NC} Generate self-signed cert now (for testing / internal use)"
    echo -e "  ${BOLD}2)${NC} Exit and place your own certs manually:"
    echo -e "     ${CYAN}$CERT_DIR/fullchain.pem${NC}  (certificate + intermediates)"
    echo -e "     ${CYAN}$CERT_DIR/privkey.pem${NC}    (private key)"
    echo -e "     Then use certbot/acme.sh for Let's Encrypt in production."
    echo ""
    read -rp "$(echo -e "${CYAN}Generate self-signed cert? [y/N]:${NC} ")" gen_cert

    if [[ "$gen_cert" =~ ^[Yy]$ ]]; then
        # Read domain from .env.prod
        DOMAIN=$(grep "^DOMAIN=" "$ENV_FILE" | cut -d= -f2 | tr -d '"' | tr -d "'")
        DOMAIN="${DOMAIN:-thairag.local}"
        info "Generating self-signed certificate for: $DOMAIN"

        openssl req -x509 -nodes -days 365 \
            -newkey rsa:2048 \
            -keyout "$CERT_DIR/privkey.pem" \
            -out    "$CERT_DIR/fullchain.pem" \
            -subj "/CN=${DOMAIN}/O=ThaiRAG/C=TH" \
            -addext "subjectAltName=DNS:${DOMAIN},DNS:localhost,IP:127.0.0.1" \
            2>/dev/null

        chmod 600 "$CERT_DIR/privkey.pem"
        success "Self-signed certificate generated (valid 365 days)"
        warn "Self-signed certs will show browser warnings. Use Let's Encrypt for production."
    else
        die "Place your certificates in $CERT_DIR/ and re-run."
    fi
fi

# ── Step 4: Database backup ───────────────────────────────────────────────────
step "4/6  Database backup"

if [[ "$SKIP_BACKUP" == "true" ]]; then
    warn "Backup skipped (--no-backup)"
else
    # Only backup if postgres is currently running
    POSTGRES_RUNNING=false
    if $COMPOSE -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps --status running 2>/dev/null \
        | grep -q postgres; then
        POSTGRES_RUNNING=true
    fi

    if $POSTGRES_RUNNING; then
        info "Running database backup before deploy..."
        if [[ -x "$BACKUP_SCRIPT" ]]; then
            "$BACKUP_SCRIPT"
            success "Database backup complete"
        else
            warn "backup-db.sh not found or not executable — skipping backup."
            warn "Re-run with --no-backup to suppress this warning."
        fi
    else
        info "PostgreSQL not currently running — skipping backup (first deploy?)"
    fi
fi

# ── Step 5: Build and deploy ──────────────────────────────────────────────────
step "5/6  Building and deploying"

cd "$PROJECT_ROOT"

if [[ "$SKIP_BUILD" == "true" ]]; then
    info "Skipping image rebuild (--no-build)"
    $COMPOSE -f "$COMPOSE_FILE" --env-file "$ENV_FILE" up -d
else
    info "Building images and starting services..."
    $COMPOSE -f "$COMPOSE_FILE" --env-file "$ENV_FILE" up -d --build
fi

# ── Step 6: Health check ──────────────────────────────────────────────────────
step "6/6  Post-deployment health check"

info "Waiting for services to start..."

# Wait for ThaiRAG API (via nginx on 443)
HEALTHY=false
for i in $(seq 1 40); do
    if curl -skf https://localhost/health -o /dev/null 2>/dev/null; then
        HEALTHY=true
        break
    fi
    sleep 3
done

if $HEALTHY; then
    success "ThaiRAG API is healthy (via nginx HTTPS)"
else
    warn "ThaiRAG API did not respond on https://localhost/health after 120s"
    warn "Check logs with: docker compose -f docker-compose.prod.yml logs --tail=50 thairag"
fi

# Show service status
echo ""
$COMPOSE -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps

# Verify DB record counts if postgres is accessible
POSTGRES_RUNNING=false
if $COMPOSE -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps --status running 2>/dev/null \
    | grep -q postgres; then
    POSTGRES_RUNNING=true
fi

if $POSTGRES_RUNNING; then
    CONTAINER=$($COMPOSE -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps -q postgres 2>/dev/null | head -1)
    if [[ -n "$CONTAINER" ]]; then
        DB_USER=$(grep "^POSTGRES_USER=" "$ENV_FILE" | cut -d= -f2 | tr -d '"')
        DB_NAME=$(grep "^POSTGRES_DB="   "$ENV_FILE" | cut -d= -f2 | tr -d '"')
        USER_COUNT=$(docker exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -tAc \
            "SELECT count(*) FROM users;" 2>/dev/null | tr -d ' ' || echo "?")
        success "Database check: ${USER_COUNT} user(s)"
    fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────
DOMAIN_VAL=$(grep "^DOMAIN=" "$ENV_FILE" | cut -d= -f2 | tr -d '"' | tr -d "'")
DOMAIN_VAL="${DOMAIN_VAL:-<your-domain>}"

echo ""
echo -e "${GREEN}${BOLD}╔══════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}${BOLD}║              Deployment Complete                    ║${NC}"
echo -e "${GREEN}${BOLD}╚══════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  API endpoint:   ${CYAN}https://${DOMAIN_VAL}/v1/${NC}"
echo -e "  Admin UI:       ${CYAN}https://${DOMAIN_VAL}/admin/${NC}"
echo -e "  Health check:   ${CYAN}https://${DOMAIN_VAL}/health${NC}"
echo -e "  Grafana:        ${CYAN}https://${DOMAIN_VAL}/grafana/${NC}  (internal network only)"
echo ""
echo -e "  Logs:           ${CYAN}docker compose -f docker-compose.prod.yml logs -f${NC}"
echo -e "  Stop:           ${CYAN}docker compose -f docker-compose.prod.yml down${NC}"
echo -e "  Backup:         ${CYAN}./scripts/backup-db.sh${NC}"
echo ""
