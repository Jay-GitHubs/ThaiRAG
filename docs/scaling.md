# Horizontal Scaling Guide

This guide describes how to deploy multiple ThaiRAG instances behind a load balancer with shared state.

## Architecture

```
                         ┌─────────────────┐
                         │     Clients      │
                         └────────┬─────────┘
                                  │
                         ┌────────▼─────────┐
                         │   nginx (LB)     │
                         │   :8080 / :443   │
                         └────────┬─────────┘
                    ┌─────────────┼─────────────┐
              ┌─────▼─────┐ ┌────▼──────┐ ┌────▼──────┐
              │ ThaiRAG-1 │ │ ThaiRAG-2 │ │ ThaiRAG-3 │
              │   :8080   │ │   :8080   │ │   :8080   │
              └─────┬─────┘ └─────┬─────┘ └─────┬─────┘
                    │             │              │
        ┌───────────┴─────────────┴──────────────┴───────────┐
        │                  Shared backends                    │
        │                                                    │
        │  ┌────────────┐  ┌──────────┐  ┌──────────────┐   │
        │  │ PostgreSQL │  │  Redis   │  │    Qdrant    │   │
        │  │   :5432    │  │  :6379   │  │  :6333/6334  │   │
        │  └────────────┘  └──────────┘  └──────────────┘   │
        └────────────────────────────────────────────────────┘
```

## Prerequisites

Horizontal scaling requires all stateful backends to be externalized. The free tier defaults (in-memory session store, in-memory vector store) will **not** work because each instance would have its own isolated state.

You need:

| Component    | Requirement                        | Why                                             |
|-------------|------------------------------------|-------------------------------------------------|
| PostgreSQL  | Shared instance                    | User accounts, KM metadata, document records    |
| Redis       | Shared instance                    | Sessions, embedding cache, job queue             |
| Qdrant      | Shared instance                    | Vector embeddings for semantic search            |

## Configuration Requirements

All instances must share the same external backends. The `docker-compose.scale.yml` override sets these automatically, but if deploying manually, ensure every instance has:

### 1. Redis backends (required)

```toml
[session]
backend = "redis"

[embedding_cache]
backend = "redis"

[job_queue]
backend = "redis"

[redis]
url = "redis://redis:6379"
```

Or via environment variables:

```sh
THAIRAG__SESSION__BACKEND=redis
THAIRAG__EMBEDDING_CACHE__BACKEND=redis
THAIRAG__JOB_QUEUE__BACKEND=redis
THAIRAG__REDIS__URL=redis://redis:6379
```

### 2. PostgreSQL database (required)

```sh
THAIRAG__DATABASE__URL=postgresql://thairag:thairag@postgres:5432/thairag
```

Do **not** use SQLite -- it does not support concurrent writes from multiple processes.

### 3. Qdrant vector store (required)

```sh
THAIRAG__PROVIDERS__VECTOR_STORE__KIND=qdrant
THAIRAG__PROVIDERS__VECTOR_STORE__URL=http://qdrant:6334
THAIRAG__PROVIDERS__VECTOR_STORE__COLLECTION=thairag_chunks
```

Do **not** use `in_memory` -- each instance would have its own empty vector store.

### 4. Tantivy text search (limitation)

Tantivy uses a local on-disk index. In a multi-instance deployment, each instance maintains its own index, which leads to inconsistent BM25 search results.

**Options:**

- **Disable text search** and rely solely on vector search (set `vector_weight = 1.0`, `text_weight = 0.0`).
- **Shared volume** -- mount the same volume to all instances. Tantivy's write locking means only one writer at a time, so this requires careful coordination.
- **Accept eventual consistency** -- if documents are indexed on one instance, other instances will not see them in text search until they also index locally.

For production multi-instance deployments, disabling text search or using vector-only search is the recommended approach:

```sh
THAIRAG__SEARCH__VECTOR_WEIGHT=1.0
THAIRAG__SEARCH__TEXT_WEIGHT=0.0
```

## Deployment

### Quick start (3 replicas)

```sh
docker compose -f docker-compose.yml -f docker-compose.scale.yml up -d
```

This starts 3 ThaiRAG instances behind an nginx load balancer on port 8080.

### Custom replica count

```sh
docker compose -f docker-compose.yml -f docker-compose.scale.yml up -d --scale thairag=5
```

### Verify deployment

Check that all instances are healthy:

```sh
docker compose -f docker-compose.yml -f docker-compose.scale.yml ps
```

Test the load balancer:

```sh
curl http://localhost:8080/health
curl http://localhost:8080/health?deep=true
```

### Scaling up/down at runtime

```sh
# Scale up to 5
docker compose -f docker-compose.yml -f docker-compose.scale.yml up -d --scale thairag=5 --no-recreate

# Scale down to 2
docker compose -f docker-compose.yml -f docker-compose.scale.yml up -d --scale thairag=2
```

nginx will automatically pick up new instances via Docker DNS resolution.

## Load Balancer Details

The nginx configuration (`nginx/nginx.conf`) provides:

- **ip_hash** upstream for sticky sessions -- the same client IP always routes to the same ThaiRAG instance. This is important for SSE streaming of chat completions.
- **SSE support** -- proxy buffering is disabled on `/v1/chat/completions` so tokens stream immediately to clients.
- **Rate limiting** -- 10 requests/second per IP with burst of 20.
- **WebSocket upgrade** support for future use.
- **Metrics endpoint** restricted to internal Docker networks.
- **TLS termination** placeholder (commented out with setup instructions).

## Monitoring

### Prometheus

The `prometheus.scale.yml` config uses Docker DNS service discovery to automatically scrape all ThaiRAG replicas:

```yaml
dns_sd_configs:
  - names:
      - "tasks.thairag"
    type: A
    port: 8080
```

Each instance is labeled by its container IP so metrics can be viewed per-instance or aggregated.

### Key metrics to watch

| Metric                           | Description                          |
|---------------------------------|--------------------------------------|
| `http_requests_total`           | Request count by instance            |
| `http_request_duration_seconds` | Latency distribution by instance     |
| `llm_tokens_total`             | LLM token consumption                |
| `active_sessions_total`        | Sessions per instance (should be ~0 with Redis) |

### Grafana

Grafana is included in the base `docker-compose.yml` on port 3001. With the scaled Prometheus config, dashboards will show per-instance breakdowns automatically.

## Environment File

Create a `.env` file (or copy from `.env.example`) with at minimum:

```sh
# PostgreSQL
POSTGRES_DB=thairag
POSTGRES_USER=thairag
POSTGRES_PASSWORD=<strong-password>

# ThaiRAG
THAIRAG_TIER=standard
THAIRAG__DATABASE__URL=postgresql://thairag:<strong-password>@postgres:5432/thairag
THAIRAG__AUTH__ENABLED=true
THAIRAG__AUTH__JWT_SECRET=<random-64-char-string>

# LLM provider (example: Claude)
THAIRAG__PROVIDERS__LLM__KIND=claude
THAIRAG__PROVIDERS__LLM__API_KEY=sk-ant-...

# Embedding provider (example: OpenAI)
THAIRAG__PROVIDERS__EMBEDDING__KIND=openai
THAIRAG__PROVIDERS__EMBEDDING__API_KEY=sk-...
```

The `docker-compose.scale.yml` override handles the Redis/Qdrant backend settings, so you do not need to set those in `.env`.

## Known Limitations

1. **Tantivy text search is local per instance.** Each instance has its own BM25 index. See the Tantivy section above for workarounds.

2. **In-memory state is not shared.** If any backend is set to `"memory"` instead of `"redis"`, that state is isolated per instance. The scale override forces Redis backends, but verify your configuration if deploying manually.

3. **MCP sync scheduler runs on every instance.** Leader election is not yet implemented, so all instances will attempt to run scheduled MCP syncs. This is safe (syncs are idempotent) but wasteful. For now, consider running MCP sync on a single dedicated instance or disabling it on replicas:
   ```sh
   THAIRAG__MCP__ENABLED=false  # on all but one instance
   ```

4. **First-user bootstrap.** The super-admin account is created on first startup. With multiple instances starting simultaneously, a race condition is possible. In practice, PostgreSQL's unique constraints prevent duplicates, but you may see transient errors in logs from instances that lose the race.

5. **Graceful shutdown.** The server has a configurable shutdown timeout (`shutdown_timeout_secs = 30`). When scaling down, Docker sends SIGTERM and waits for the grace period. In-flight SSE streams will complete or timeout during this window.

## Production Checklist

- [ ] Use the `standard` or `premium` tier (not `free`)
- [ ] Set a strong `JWT_SECRET` (at least 64 random characters)
- [ ] Set strong PostgreSQL and Redis passwords
- [ ] Enable TLS termination at nginx (see commented section in `nginx/nginx.conf`)
- [ ] Set `THAIRAG__SEARCH__VECTOR_WEIGHT=1.0` and `TEXT_WEIGHT=0.0` to avoid Tantivy inconsistency
- [ ] Configure Prometheus alerting for instance health
- [ ] Set appropriate resource limits in `deploy.resources`
- [ ] Back up PostgreSQL and Qdrant volumes regularly
- [ ] Use Redis persistence (AOF or RDB) for session durability
