# Integration Guide

## Open WebUI Integration

[Open WebUI](https://github.com/open-webui/open-webui) is a self-hosted chat interface that supports OpenAI-compatible APIs. ThaiRAG exposes the standard `/v1/chat/completions` and `/v1/models` endpoints, making it a drop-in backend.

### Setup with Docker Compose (Recommended)

Open WebUI is already configured in `docker-compose.test-idp.yml`. Start the full stack:

```bash
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up --build -d
```

This starts ThaiRAG API, Admin UI, PostgreSQL, Qdrant, Keycloak (OIDC), and Open WebUI.

| Service | URL | Credentials |
|---------|-----|-------------|
| Admin UI | http://localhost:8081 | `admin@thairag.local` / `Admin123` |
| Open WebUI | http://localhost:3000 | Login via Keycloak SSO |
| Keycloak | http://localhost:9090 | `admin` / `admin` |

### API Key Authentication

ThaiRAG supports API key authentication as an alternative to JWT tokens.

API keys use the `X-API-Key` header:
```bash
curl http://localhost:8080/v1/chat/completions \
  -H "X-API-Key: trag_your_api_key_here" \
  -H "Content-Type: application/json" \
  -d '{"model":"ThaiRAG-1.0","messages":[{"role":"user","content":"Hello"}]}'
```

Create API keys via the Admin UI or API:
```bash
curl -X POST http://localhost:8080/api/auth/api-keys \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "My Integration Key"}'
```

### Authentication & User Identity

Open WebUI authenticates to ThaiRAG using a static API key (configured automatically via `THAIRAG_OPENWEBUI_API_KEY` in `.env`). No JWT token management needed.

To use a custom API key, set in your `.env`:
```bash
THAIRAG_OPENWEBUI_API_KEY=sk-your-custom-key
```

### Per-User Permission Enforcement

By default, API key auth grants unrestricted access to all knowledge bases. To enforce per-user workspace permissions through Open WebUI, enable **user identity forwarding**:

```yaml
# In docker-compose.test-idp.yml (or your Open WebUI config)
open-webui:
  environment:
    ENABLE_FORWARD_USER_INFO_HEADERS: "true"
```

When enabled, Open WebUI sends `X-OpenWebUI-User-Email` and `X-OpenWebUI-User-Name` headers with every API request. ThaiRAG resolves the real user from these headers and applies their workspace permissions:

1. **User lookup** — ThaiRAG looks up the user by email in its database
2. **Auto-provisioning** — If the user doesn't exist, they are auto-created with `viewer` role
3. **Permission scoping** — The user's workspace permissions determine which knowledge bases are searched
4. **Session tracking** — Sessions are associated with the resolved user for permission enforcement

> **Without** `ENABLE_FORWARD_USER_INFO_HEADERS`, all Open WebUI users share the API key's unrestricted access — no per-user permission enforcement.

### Permission Revocation Behavior

When an admin revokes a user's workspace permission via the Admin UI:

- **New sessions** — The user immediately loses access to the revoked workspace's content
- **Existing sessions** — Server-side session history and personal memories for the revoked user are cleared to prevent stale context leaks
- **Open WebUI client-side history** — Messages already displayed in Open WebUI's chat window remain visible (client-side cache), but starting a new chat enforces the updated permissions

### Manual Setup (without docker-compose.test-idp.yml)

Add the Open WebUI service to `docker-compose.yml`:

```yaml
open-webui:
  image: ghcr.io/open-webui/open-webui:v0.8.10
  ports:
    - "3000:8080"
  volumes:
    - open-webui-data:/app/backend/data
  environment:
    OPENAI_API_BASE_URLS: "http://thairag:8080/v1"
    OPENAI_API_KEYS: "sk-thairag-openwebui"
    # Enable per-user permission enforcement (recommended)
    ENABLE_FORWARD_USER_INFO_HEADERS: "true"
    # Increase timeout for pipeline responses (multi-agent processing can take 60+ seconds)
    AIOHTTP_CLIENT_TIMEOUT: "600"
  depends_on:
    - thairag
```

And add the matching API key to the thairag service environment:
```yaml
THAIRAG__AUTH__API_KEYS: "sk-thairag-openwebui"
```

### Standalone Open WebUI

If running Open WebUI outside Docker Compose:

```bash
docker run -d \
  -p 3000:8080 \
  -e OPENAI_API_BASE_URLS="http://host.docker.internal:8080/v1" \
  -e OPENAI_API_KEYS="your-jwt-token" \
  -v open-webui-data:/app/backend/data \
  ghcr.io/open-webui/open-webui:main
```

### What Works

- Model listing — Open WebUI discovers "ThaiRAG-1.0" automatically
- Chat completions — Both streaming and non-streaming
- Session management — Conversation history is maintained server-side
- RAG — All responses are automatically augmented with knowledge base content
- Context compaction — Long conversations are automatically summarized to stay within context limits (when enabled)
- Personal memory — Per-user memory retrieval across sessions for personalized responses (when enabled)
- Per-user permissions — When `ENABLE_FORWARD_USER_INFO_HEADERS` is set, each user only sees content from workspaces they have access to
- SSE keepalive — Long pipeline processing (60+ seconds) stays connected via automatic ping comments every 15 seconds

### What Doesn't Work (by design)

- Image generation endpoints — ThaiRAG is text-only
- Function calling / tool use — Not part of the RAG pipeline
- Fine-tuning endpoints — Not applicable

---

## OpenID Connect (OIDC) Integration

ThaiRAG supports managing OIDC identity providers through the Admin UI. The management and configuration layer is fully implemented; actual protocol flows (token exchange, userinfo) are prepared for implementation.

### Supported Identity Providers

| Type | Examples | Status |
|------|----------|--------|
| OIDC | Keycloak, Auth0, Okta, Azure AD | Config management ready |
| OAuth2 | GitHub, Google, custom | Config management ready |
| SAML | OneLogin, ADFS | Config management ready |
| LDAP | Active Directory, OpenLDAP | Config management ready |

### Configuring an OIDC Provider

#### Via Admin UI

1. Log in as super admin
2. Navigate to **Settings** → **Identity Providers** tab
3. Click **Add Provider**
4. Select type: **OIDC**
5. Fill in the configuration:
   - **Name**: Display name (e.g., "Corporate SSO")
   - **Issuer URL**: Your OIDC discovery endpoint (e.g., `https://auth.example.com/realms/main`)
   - **Client ID**: Application client ID
   - **Client Secret**: Application client secret
   - **Scopes**: `openid profile email` (default)
   - **Redirect URI**: `http://your-thairag-host:8080/api/auth/oauth/callback`
6. Toggle **Enabled**
7. Click **Save**
8. Click **Test** to verify connectivity

#### Via API

```bash
curl -X POST http://localhost:8080/api/km/settings/identity-providers \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Corporate SSO",
    "provider_type": "oidc",
    "enabled": true,
    "config": {
      "issuer_url": "https://auth.example.com/realms/main",
      "client_id": "thairag",
      "client_secret": "your-client-secret",
      "scopes": "openid profile email",
      "redirect_uri": "http://localhost:8080/api/auth/oauth/callback"
    }
  }'
```

### Keycloak Setup Example

1. **Create a realm** (or use existing)
2. **Create a client:**
   - Client ID: `thairag`
   - Client Protocol: `openid-connect`
   - Access Type: `confidential`
   - Valid Redirect URIs: `http://your-thairag-host:8080/api/auth/oauth/callback`
3. **Get client secret:** Clients → thairag → Credentials → Secret
4. **Configure in ThaiRAG:**
   - Issuer URL: `http://keycloak:8080/realms/your-realm`
   - Client ID: `thairag`
   - Client Secret: (from step 3)

### Testing with Docker Compose

A test identity provider setup is available via `docker-compose.test-idp.yml` (if present). This spins up a Keycloak instance pre-configured with test users for development.

### Login Flow

Once an OIDC provider is configured and enabled:

1. The login page automatically shows SSO buttons for each enabled provider
2. User clicks the SSO button
3. Browser redirects to the identity provider's login page
4. After authentication, the IdP redirects back to ThaiRAG's callback URL
5. ThaiRAG exchanges the authorization code for tokens
6. User is created/updated in ThaiRAG's user database
7. A ThaiRAG JWT is issued and the user is logged in

### LDAP Configuration

For LDAP/Active Directory:

```json
{
  "name": "Corporate LDAP",
  "provider_type": "ldap",
  "enabled": true,
  "config": {
    "server_url": "ldap://ldap.example.com:389",
    "bind_dn": "cn=admin,dc=example,dc=com",
    "bind_password": "admin-password",
    "search_base": "ou=users,dc=example,dc=com",
    "search_filter": "(uid={username})",
    "tls_enabled": true
  }
}
```

LDAP login uses a different flow — the login page shows an inline username/password form for LDAP providers instead of a redirect button.

### SAML Configuration

```json
{
  "name": "Corporate SAML",
  "provider_type": "saml",
  "enabled": true,
  "config": {
    "idp_entity_id": "https://idp.example.com",
    "sso_url": "https://idp.example.com/sso",
    "slo_url": "https://idp.example.com/slo",
    "certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "sp_entity_id": "http://your-thairag-host:8080"
  }
}
```

---

## Open WebUI with SSO

You can configure both ThaiRAG and Open WebUI to use the same identity provider for a unified SSO experience.

### Docker Compose with OIDC

```yaml
thairag:
  build: .
  ports:
    - "8080:8080"
  env_file: .env

open-webui:
  image: ghcr.io/open-webui/open-webui:main
  ports:
    - "3000:8080"
  environment:
    OPENAI_API_BASE_URLS: "http://thairag:8080/v1"
    OPENAI_API_KEYS: "your-jwt-token"
    # OIDC SSO for Open WebUI
    ENABLE_OAUTH_SIGNUP: "true"
    OAUTH_PROVIDER_NAME: "SSO"
    OAUTH_CLIENT_ID: "open-webui"
    OAUTH_CLIENT_SECRET: "open-webui-secret"
    OPENID_PROVIDER_URL: "https://auth.example.com/.well-known/openid-configuration"
    OAUTH_SCOPES: "openid profile email"
  depends_on:
    - thairag
```

In this setup, both ThaiRAG (Admin UI) and Open WebUI (Chat UI) authenticate against the same OIDC provider, giving users a single sign-on experience.

---

## Custom API Client Integration

ThaiRAG implements the OpenAI Chat Completions API, so any OpenAI-compatible client library works.

### Python (openai library)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/v1",
    api_key="your-jwt-token",  # ThaiRAG JWT
)

response = client.chat.completions.create(
    model="ThaiRAG-1.0",
    messages=[
        {"role": "user", "content": "What documents do we have about security?"}
    ],
    stream=True,
)

for chunk in response:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="")
```

### JavaScript/TypeScript

```typescript
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://localhost:8080/v1',
  apiKey: 'your-jwt-token',
});

const response = await client.chat.completions.create({
  model: 'ThaiRAG-1.0',
  messages: [{ role: 'user', content: 'Summarize our HR policies' }],
});

console.log(response.choices[0].message.content);
```

### curl

```bash
# Non-streaming
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "ThaiRAG-1.0",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": false
  }'

# Streaming
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "ThaiRAG-1.0",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true
  }'
```

---

## Webhook / Programmatic Document Ingestion

Upload documents programmatically for automated pipelines:

```bash
# Upload a PDF
curl -X POST "http://localhost:8080/api/km/workspaces/$WORKSPACE_ID/documents/upload" \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@/path/to/document.pdf" \
  -F "title=Quarterly Report Q1 2026"

# Upload from text
curl -X POST "http://localhost:8080/api/km/workspaces/$WORKSPACE_ID/documents" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "Meeting Notes",
    "content": "Discussion points from the team meeting...",
    "format": "text/plain"
  }'
```

---

## V2 API Integration

ThaiRAG v2 API provides additional metadata with responses:
- Search sources with document IDs and relevance scores
- Intent classification
- Processing time metrics

V2 endpoints: `/v2/chat/completions`, `/v2/search`, `/v2/models`

Version selection via URL path (`/v2/...`) or `X-API-Version: v2` header.

---

## WebSocket Chat

For real-time bidirectional chat, connect to `/ws/chat`:
```javascript
const ws = new WebSocket('ws://localhost:8080/ws/chat?token=your-jwt');
ws.send(JSON.stringify({ type: 'chat', content: 'Hello', session_id: 'optional-uuid' }));
ws.onmessage = (event) => console.log(JSON.parse(event.data));
```

---

## Monitoring Integration

### Prometheus

Scrape metrics from `/metrics`:

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'thairag'
    static_configs:
      - targets: ['thairag:8080']
    metrics_path: '/metrics'
```

Available metrics:
- `http_requests_total{method, path, status}` — Request counter
- `http_request_duration_seconds{method, path}` — Latency histogram
- `llm_tokens_total{type}` — Token usage
- `active_sessions_total` — Active chat sessions

### Grafana

Grafana is included in Docker Compose on port 3001 with pre-configured dashboards:
- API request rate and latency (p50, p95, p99)
- Token consumption over time
- Error rate by endpoint
- Active sessions gauge
- LLM call latency distribution

### Health Checks

Use the deep health check for monitoring:

```bash
curl -f http://localhost:8080/health?deep=true || alert
```

This probes all configured providers and returns non-200 if any are unhealthy.

---

## Python SDK Integration

The ThaiRAG Python SDK provides a high-level client for interacting with the ThaiRAG API, including Phase 6 features like search analytics, document lineage, and multi-tenancy management.

### Installation

```bash
pip install thairag
```

### Basic Usage

```python
from thairag import ThaiRAGClient

client = ThaiRAGClient(
    base_url="http://localhost:8080",
    api_key="trag_your_api_key_here",
)

# Chat completion (uses OpenAI-compatible endpoint internally)
response = client.chat("What documents do we have about security?")
print(response.content)
print(response.sources)  # Document attribution

# Streaming
for chunk in client.chat_stream("Summarize our HR policies"):
    print(chunk.content, end="")
```

### Search Analytics

```python
# Get popular queries over the last 30 days
popular = client.analytics.popular_queries(days=30, limit=10)
for q in popular:
    print(f"{q.query} - {q.count} times")

# Get zero-result queries (queries that returned no documents)
zero_results = client.analytics.zero_result_queries(days=7)
for q in zero_results:
    print(f"No results for: {q.query}")

# Get summary statistics
stats = client.analytics.summary(days=30)
print(f"Total queries: {stats.total_queries}")
print(f"Avg results per query: {stats.avg_results}")
print(f"Zero-result rate: {stats.zero_result_rate:.1%}")
```

### Document Lineage

```python
# Get lineage for a specific response (which chunks contributed)
lineage = client.lineage.get_response_lineage(response_id="uuid-here")
for record in lineage.chunks:
    print(f"Document: {record.document_title}")
    print(f"Chunk: {record.chunk_text[:100]}...")
    print(f"Relevance: {record.relevance_score:.3f}")
```

### Multi-tenancy

```python
# Create a tenant
tenant = client.tenants.create(
    name="Acme Corp",
    slug="acme",
    quota={"max_documents": 5000, "max_storage_mb": 10240, "max_users": 100},
)

# List tenants
tenants = client.tenants.list()

# Switch tenant context for subsequent operations
client.set_tenant("acme")
```

### Document Ingestion

```python
# Upload a file
client.documents.upload(
    workspace_id="ws-uuid",
    file_path="/path/to/report.pdf",
    title="Q1 2026 Report",
)

# Upload from text
client.documents.create(
    workspace_id="ws-uuid",
    title="Meeting Notes",
    content="Discussion points from the team meeting...",
    format="text/plain",
)
```

---

## TypeScript SDK Integration

### Installation

```bash
npm install thairag
# or
yarn add thairag
```

### Basic Usage

```typescript
import { ThaiRAGClient } from 'thairag';

const client = new ThaiRAGClient({
  baseUrl: 'http://localhost:8080',
  apiKey: 'trag_your_api_key_here',
});

// Chat completion
const response = await client.chat('What is our security policy?');
console.log(response.content);
console.log(response.sources); // Document attribution

// Streaming
const stream = client.chatStream('Summarize our HR policies');
for await (const chunk of stream) {
  process.stdout.write(chunk.content);
}
```

### Using the OpenAI-compatible Interface

Since ThaiRAG implements the OpenAI API, you can also use the standard OpenAI SDK directly:

```typescript
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://localhost:8080/v1',
  apiKey: 'trag_your_api_key_here',
});

const response = await client.chat.completions.create({
  model: 'ThaiRAG-1.0',
  messages: [{ role: 'user', content: 'Hello' }],
});
```

### Search Analytics

```typescript
// Popular queries
const popular = await client.analytics.popularQueries({ days: 30, limit: 10 });
popular.forEach((q) => console.log(`${q.query} - ${q.count} times`));

// Zero-result queries
const zeroResults = await client.analytics.zeroResultQueries({ days: 7 });

// Summary stats
const stats = await client.analytics.summary({ days: 30 });
console.log(`Total queries: ${stats.totalQueries}`);
```

### KM Hierarchy Management

```typescript
// Create organization -> department -> workspace
const org = await client.km.createOrg({ name: 'Engineering' });
const dept = await client.km.createDept(org.id, { name: 'Backend' });
const ws = await client.km.createWorkspace(dept.id, { name: 'API Docs' });

// Upload document
await client.documents.upload(ws.id, {
  file: fs.createReadStream('/path/to/doc.pdf'),
  title: 'API Reference',
});
```

---

## Deployment CLI Integration in CI/CD

The ThaiRAG deployment CLI can be integrated into CI/CD pipelines for automated health checks, backups, and deployments.

### GitHub Actions Example

```yaml
name: Deploy ThaiRAG
on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Pre-deployment backup
        run: thairag backup create --output /backups/pre-deploy-${{ github.sha }}

      - name: Validate configuration
        run: thairag config validate

      - name: Deploy new version
        run: thairag deploy --tag ${{ github.sha }}

      - name: Post-deployment health check
        run: |
          sleep 10
          thairag health --deep || {
            echo "Health check failed, rolling back"
            thairag deploy --tag ${{ env.PREVIOUS_TAG }}
            exit 1
          }

      - name: Verify search quality
        run: |
          thairag status | grep -q "index_health: ok"
```

### Scheduled Backup (Cron)

```bash
# /etc/cron.d/thairag-backup
0 2 * * * root thairag backup create --output /backups/$(date +\%Y\%m\%d) 2>&1 | logger -t thairag-backup
```

---

## Search Analytics Integration

Search analytics provides visibility into query patterns, helping you identify knowledge gaps and optimize your document corpus.

### Tracking Query Patterns

Every RAG query automatically records an analytics event when `search_analytics.enabled=true`. Events include:

- Query text
- Number of results returned
- Response time (milliseconds)
- Whether the query was a zero-result query
- Timestamp
- User ID (if authenticated)

### API Endpoints

```bash
# Get popular queries
curl "http://localhost:8080/api/km/analytics/popular-queries?days=30&limit=10" \
  -H "Authorization: Bearer $TOKEN"

# Get zero-result queries (knowledge gaps)
curl "http://localhost:8080/api/km/analytics/zero-result-queries?days=7" \
  -H "Authorization: Bearer $TOKEN"

# Get summary statistics
curl "http://localhost:8080/api/km/analytics/summary?days=30" \
  -H "Authorization: Bearer $TOKEN"
```

### Zero-Result Analysis

Zero-result queries indicate topics that users are asking about but your knowledge base does not cover. Use this data to:

1. Identify missing documents that should be added to the knowledge base.
2. Detect terminology mismatches between user queries and document content (consider adding synonyms or adjusting chunking).
3. Track the zero-result rate over time to measure knowledge base completeness.

### Integrating with External Analytics

Export analytics data for external dashboards (e.g., Grafana, Kibana):

```bash
# Export raw analytics events as JSON
curl "http://localhost:8080/api/km/analytics/export?format=json&from=2026-01-01&to=2026-03-30" \
  -H "Authorization: Bearer $TOKEN" \
  -o analytics-export.json
```

---

## Document Lineage for Compliance

Document lineage tracks which document chunks contributed to each RAG response, providing an attribution chain for compliance and audit purposes.

### How It Works

1. When a RAG query is processed, the search pipeline identifies the most relevant chunks.
2. After the response is generated, a lineage record is created (fire-and-forget via background task) linking the response to the specific chunks used.
3. Each lineage record includes: response ID, chunk IDs, document IDs, relevance scores, and timestamp.

### API Endpoints

```bash
# Get lineage for a specific response
curl "http://localhost:8080/api/km/lineage/responses/$RESPONSE_ID" \
  -H "Authorization: Bearer $TOKEN"

# Get all lineage records for a document (which responses cited this document)
curl "http://localhost:8080/api/km/lineage/documents/$DOCUMENT_ID" \
  -H "Authorization: Bearer $TOKEN"
```

### Response Attribution Format

```json
{
  "response_id": "uuid",
  "query": "What is our data retention policy?",
  "timestamp": "2026-03-30T10:15:00Z",
  "chunks_used": [
    {
      "chunk_id": "uuid",
      "document_id": "uuid",
      "document_title": "Data Governance Policy v3",
      "chunk_text": "Data shall be retained for a minimum of...",
      "relevance_score": 0.923
    }
  ]
}
```

### Compliance Use Cases

- **Regulatory audit** -- demonstrate that AI responses are grounded in approved documents.
- **Source verification** -- allow users to trace any answer back to its source material.
- **Document impact analysis** -- before updating or deleting a document, see which responses have cited it.
- **Quality assurance** -- review lineage records to verify that the RAG pipeline is retrieving relevant content.

---

## Audit Log Export for SIEM Integration

ThaiRAG records structured audit events for security-relevant actions. These logs can be exported for ingestion into SIEM systems (Splunk, Elastic Security, Microsoft Sentinel, etc.).

### Audited Actions

- User login (success and failure)
- User registration
- Permission grants and revocations
- User deletion
- Document upload and deletion
- KM hierarchy changes (org, dept, workspace CRUD)
- Settings changes
- API key creation and revocation
- Identity provider configuration changes

### Export API

```bash
# Export audit logs as JSON (for SIEM ingestion)
curl "http://localhost:8080/api/km/audit/export?format=json&from=2026-03-01&to=2026-03-30" \
  -H "Authorization: Bearer $TOKEN" \
  -o audit-log.json

# Export as CSV (for spreadsheet analysis)
curl "http://localhost:8080/api/km/audit/export?format=csv&from=2026-03-01&to=2026-03-30" \
  -H "Authorization: Bearer $TOKEN" \
  -o audit-log.csv

# Filter by action type
curl "http://localhost:8080/api/km/audit/export?format=json&action=login_failed&from=2026-03-01" \
  -H "Authorization: Bearer $TOKEN"
```

### Audit Log Analytics

```bash
# Get action counts by type (useful for dashboards)
curl "http://localhost:8080/api/km/audit/analytics?from=2026-03-01&to=2026-03-30" \
  -H "Authorization: Bearer $TOKEN"
```

Returns counts grouped by action type, useful for creating security dashboards that track login failures, permission changes, and other security-relevant events.

### SIEM Integration Pattern

For continuous ingestion, set up a scheduled job that exports audit logs since the last export:

```bash
#!/bin/bash
LAST_EXPORT=$(cat /var/lib/thairag/last-audit-export 2>/dev/null || echo "1970-01-01")
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)

curl -s "http://localhost:8080/api/km/audit/export?format=json&from=$LAST_EXPORT&to=$NOW" \
  -H "Authorization: Bearer $TOKEN" \
  | /opt/splunk/bin/splunk add oneshot -sourcetype thairag_audit

echo "$NOW" > /var/lib/thairag/last-audit-export
```

---

## Multi-tenancy Integration Patterns

Multi-tenancy allows a single ThaiRAG deployment to serve multiple isolated organizations.

### Tenant Provisioning

```bash
# Create a new tenant
curl -X POST http://localhost:8080/api/km/tenants \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Acme Corp",
    "slug": "acme",
    "quota": {
      "max_documents": 5000,
      "max_storage_mb": 10240,
      "max_users": 100
    }
  }'

# List all tenants
curl http://localhost:8080/api/km/tenants \
  -H "Authorization: Bearer $TOKEN"

# Get tenant details including usage
curl http://localhost:8080/api/km/tenants/$TENANT_ID \
  -H "Authorization: Bearer $TOKEN"

# Update tenant quota
curl -X PATCH http://localhost:8080/api/km/tenants/$TENANT_ID \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"quota": {"max_documents": 10000}}'
```

### Tenant-scoped Operations

When multi-tenancy is enabled, all KM operations (orgs, departments, workspaces, documents) are scoped to the authenticated user's tenant. Super admins can operate across tenants by specifying the `X-Tenant-ID` header:

```bash
# Super admin: operate within a specific tenant
curl http://localhost:8080/api/km/orgs \
  -H "Authorization: Bearer $SUPER_ADMIN_TOKEN" \
  -H "X-Tenant-ID: $TENANT_ID"
```

### Tenant Isolation Guarantees

- **Data isolation** -- each tenant's documents, embeddings, and search indices are logically separated.
- **Query isolation** -- RAG queries only search within the authenticated user's tenant boundary.
- **User isolation** -- users belong to exactly one tenant; user lists and management are tenant-scoped.
- **Quota enforcement** -- document counts, storage usage, and user counts are enforced per tenant.

### SaaS Integration Pattern

For SaaS applications that provision ThaiRAG tenants programmatically:

```python
# When a new customer signs up in your SaaS app:
import requests

def provision_thairag_tenant(customer_name: str, plan: str):
    quotas = {
        "starter": {"max_documents": 500, "max_storage_mb": 1024, "max_users": 10},
        "business": {"max_documents": 5000, "max_storage_mb": 10240, "max_users": 100},
        "enterprise": {"max_documents": 50000, "max_storage_mb": 102400, "max_users": 1000},
    }

    response = requests.post(
        "http://thairag:8080/api/km/tenants",
        headers={"Authorization": f"Bearer {ADMIN_TOKEN}"},
        json={
            "name": customer_name,
            "slug": customer_name.lower().replace(" ", "-"),
            "quota": quotas[plan],
        },
    )
    return response.json()["id"]
```
