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

Import or create dashboards using the Prometheus metrics above. Key panels:
- Request rate and latency (p50, p95, p99)
- Token consumption over time
- Error rate by endpoint
- Active sessions gauge

### Health Checks

Use the deep health check for monitoring:

```bash
curl -f http://localhost:8080/health?deep=true || alert
```

This probes all configured providers and returns non-200 if any are unhealthy.
