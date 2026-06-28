# ── Builder ────────────────────────────────────────────────────────────
# Keep in lockstep with rust-toolchain.toml (pinned channel) so image builds
# match local + CI; a drift here silently builds with a different compiler.
FROM rust:1.95-bookworm AS builder

WORKDIR /app

# 1. Copy workspace manifest + lock + member Cargo.toml files (dependency cache layer)
COPY Cargo.toml Cargo.lock ./
COPY crates/thairag-core/Cargo.toml          crates/thairag-core/Cargo.toml
COPY crates/thairag-config/Cargo.toml        crates/thairag-config/Cargo.toml
COPY crates/thairag-thai/Cargo.toml          crates/thairag-thai/Cargo.toml
COPY crates/thairag-auth/Cargo.toml          crates/thairag-auth/Cargo.toml
COPY crates/thairag-provider-llm/Cargo.toml  crates/thairag-provider-llm/Cargo.toml
COPY crates/thairag-provider-embedding/Cargo.toml crates/thairag-provider-embedding/Cargo.toml
COPY crates/thairag-provider-vectordb/Cargo.toml  crates/thairag-provider-vectordb/Cargo.toml
COPY crates/thairag-provider-search/Cargo.toml    crates/thairag-provider-search/Cargo.toml
COPY crates/thairag-provider-reranker/Cargo.toml  crates/thairag-provider-reranker/Cargo.toml
COPY crates/thairag-document/Cargo.toml      crates/thairag-document/Cargo.toml
COPY crates/thairag-search/Cargo.toml        crates/thairag-search/Cargo.toml
COPY crates/thairag-agent/Cargo.toml         crates/thairag-agent/Cargo.toml
COPY crates/thairag-mcp/Cargo.toml           crates/thairag-mcp/Cargo.toml
COPY crates/thairag-api/Cargo.toml           crates/thairag-api/Cargo.toml

# 2. Create stub source files so cargo can resolve the workspace
RUN for dir in crates/*/; do \
      mkdir -p "$dir/src"; \
      echo "" > "$dir/src/lib.rs"; \
    done && \
    mkdir -p crates/thairag-api/src && \
    echo "fn main() {}" > crates/thairag-api/src/main.rs

# 3. Build dependencies only (cached unless Cargo.toml/lock change)
RUN cargo build --release -p thairag-api 2>/dev/null || true

# 4. Copy real source and rebuild
COPY crates/ crates/
COPY config/ config/
COPY prompts/ prompts/
# Touch all source files to invalidate cargo's fingerprint cache from the stub build
RUN find crates/ -name "*.rs" -exec touch {} + && \
    cargo build --release -p thairag-api

# ── Runtime ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

# poppler-utils provides pdftoppm, used by the legacy PDF vision fallback to
# rasterize image-only PDF pages so a vision LLM can describe them.
# util-linux provides prlimit for per-render memory caps.
# curl is used just below to fetch libpdfium, then purged.
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        poppler-utils \
        util-linux \
        curl && \
    rm -rf /var/lib/apt/lists/*

# Bake the native libpdfium onto the system library path so the smart-PDF
# engine (pdfium-render) can `bind_to_system_library()` at runtime. The Rust
# build.rs downloads this for local dev; in the image we install it explicitly
# for the image's architecture. Pin via PDFIUM_RELEASE (default: latest).
ARG PDFIUM_RELEASE=latest
RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
        amd64) asset=pdfium-linux-x64.tgz ;; \
        arm64) asset=pdfium-linux-arm64.tgz ;; \
        *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    if [ "$PDFIUM_RELEASE" = "latest" ]; then \
        url="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/$asset"; \
    else \
        url="https://github.com/bblanchon/pdfium-binaries/releases/download/$PDFIUM_RELEASE/$asset"; \
    fi; \
    curl -fsSL "$url" -o /tmp/pdfium.tgz; \
    tar -xzf /tmp/pdfium.tgz -C /tmp lib/libpdfium.so; \
    install -m 0644 /tmp/lib/libpdfium.so /usr/lib/libpdfium.so; \
    rm -rf /tmp/pdfium.tgz /tmp/lib; \
    ldconfig; \
    apt-get purge -y --auto-remove curl; \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/thairag-api /usr/local/bin/thairag-api
COPY --from=builder /app/config/ /app/config/
COPY --from=builder /app/prompts/ /app/prompts/

WORKDIR /app

# Tantivy index and other persistent data
RUN mkdir -p /data

ENV THAIRAG_TIER=free
ENV THAIRAG__PROVIDERS__TEXT_SEARCH__INDEX_PATH=/data/tantivy_index

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/thairag-api"]
