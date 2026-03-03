# ── Builder ────────────────────────────────────────────────────────────
FROM rust:1.85-bookworm AS builder

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
RUN cargo build --release -p thairag-api

# ── Runtime ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/thairag-api /usr/local/bin/thairag-api
COPY --from=builder /app/config/ /app/config/

WORKDIR /app

# Tantivy index and other persistent data
RUN mkdir -p /data

ENV THAIRAG_TIER=free
ENV THAIRAG__PROVIDERS__TEXT_SEARCH__INDEX_PATH=/data/tantivy_index

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/thairag-api"]
