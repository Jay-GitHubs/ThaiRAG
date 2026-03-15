# OIDC Integration Testing with Keycloak

This guide walks through setting up a local Keycloak instance as an OIDC identity provider to test SSO integration for both the ThaiRAG Admin UI and Open WebUI.

## Architecture

```
Browser (localhost)
  |
  +---> Admin UI nginx (localhost:8081) --proxy /api/--> ThaiRAG API (docker: 8080)
  |         |
  |         +-- OIDC callback: /api/auth/oauth/callback (proxied to ThaiRAG)
  |         +-- SPA: /login#token=... (served by nginx)
  |
  +---> Open WebUI (localhost:3000) ---OIDC---> Keycloak (localhost:9090)
  |
  +---> Keycloak (localhost:9090) <--- identity provider for both UIs
```

Both UIs are registered as separate OIDC clients in the same Keycloak realm. Users sign in once via Keycloak and get access to both applications.

## Key Networking Rule

When running in Docker, there are **two different URL contexts**:

| Context | Who resolves the URL | Use |
|---------|---------------------|-----|
| **Browser URLs** | Your machine | `localhost` works fine |
| **Backend URLs** | ThaiRAG container | Must use `host.docker.internal` (not `localhost`) |

This means:
- **Issuer URL** (ThaiRAG backend → Keycloak for discovery/token exchange): `http://host.docker.internal:9090/realms/thairag`
- **Redirect URI** (browser → Admin UI nginx → ThaiRAG): `http://localhost:8081/api/auth/oauth/callback`
- **Keycloak Valid redirect URIs**: must match the redirect URI exactly

> On macOS, if `host.docker.internal` doesn't resolve in the browser, add it to `/etc/hosts`:
> ```bash
> echo "127.0.0.1 host.docker.internal" | sudo tee -a /etc/hosts
> ```

## Prerequisites

- Docker and Docker Compose v2
- Ollama running locally (for the free tier LLM) or adjust `THAIRAG_TIER`

## 1. Start the Stack

```bash
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up -d
```

This starts: PostgreSQL, ThaiRAG API, Admin UI, Keycloak, and Open WebUI.

| Service      | URL                        | Credentials              |
|-------------|----------------------------|--------------------------|
| Keycloak    | http://localhost:9090       | admin / admin            |
| ThaiRAG API | http://localhost:8080       | (internal, accessed via nginx) |
| Admin UI    | http://localhost:8081       | admin@thairag.local / Admin123 |
| Open WebUI  | http://localhost:3000       | via Keycloak SSO         |

## 2. Configure Keycloak

### 2a. Create the `thairag` Realm

1. Open http://localhost:9090 and log in as `admin` / `admin`
2. Click the realm dropdown (top-left, shows "Keycloak") → **Create realm**
3. Set **Realm name** to `thairag` → **Create**

### 2b. Create the ThaiRAG Admin UI Client

1. Go to **Clients** → **Create client**
2. Configure:
   - **Client ID**: `thairag-admin`
   - **Client type**: OpenID Connect
3. Click **Next**
4. Enable:
   - **Client authentication**: ON (confidential client)
   - **Authorization**: OFF
   - **Authentication flow**: check "Standard flow" only
5. Click **Next**
6. Set:
   - **Root URL**: `http://localhost:8081`
   - **Valid redirect URIs**: `http://localhost:8081/api/auth/oauth/callback`
   - **Web origins**: `http://localhost:8081`
7. Click **Save**
8. Go to the **Credentials** tab → copy the **Client secret** (you'll need this later)

### 2c. Create the Open WebUI Client

1. Go to **Clients** → **Create client**
2. Configure:
   - **Client ID**: `open-webui`
   - **Client type**: OpenID Connect
3. Click **Next**
4. Enable:
   - **Client authentication**: ON
   - **Authentication flow**: check "Standard flow" only
5. Click **Next**
6. Set:
   - **Root URL**: `http://localhost:3000`
   - **Valid redirect URIs**: `http://localhost:3000/oauth/oidc/callback`
   - **Web origins**: `http://localhost:3000`
7. Click **Save**
8. Go to the **Credentials** tab → copy the **Client secret** (Keycloak auto-generates it)
9. Update `OAUTH_CLIENT_SECRET` in `docker-compose.test-idp.yml` with the copied value, then recreate:
   ```bash
   docker compose -f docker-compose.yml -f docker-compose.test-idp.yml up -d --force-recreate open-webui
   ```

### 2d. Create a Test User

1. Go to **Users** → **Add user**
2. Fill in:
   - **Username**: `testuser`
   - **Email**: `test@thairag.local`
   - **First name**: `Test`
   - **Last name**: `User`
   - **Email verified**: ON
3. Click **Create**
4. Go to the **Credentials** tab → **Set password**:
   - **Password**: `test123`
   - **Temporary**: OFF
5. Click **Save**

## 3. Register the IdP in ThaiRAG

### Via Admin UI (recommended)

1. Open http://localhost:8081 and log in with `admin@thairag.local` / `Admin123`
2. Go to **Settings** → **Identity Providers** tab
3. Click **Add Provider** and fill in:
   - **Name**: `Keycloak`
   - **Type**: OIDC
   - **Enabled**: ON
   - **Issuer URL**: `http://host.docker.internal:9090/realms/thairag`
   - **Client ID**: `thairag-admin`
   - **Client Secret**: *(paste from step 2b)*
   - **Scopes**: `openid profile email`
   - **Redirect URI**: `http://localhost:8081/api/auth/oauth/callback`
4. Click **OK**

> **Why different hostnames?**
> - **Issuer URL** uses `host.docker.internal` because ThaiRAG's backend (inside Docker) needs to reach Keycloak for OIDC discovery and token exchange. `localhost` inside a container points to the container itself.
> - **Redirect URI** uses `localhost` because the browser handles this redirect. The browser sends the callback to `localhost:8081` (admin-ui nginx), which proxies to ThaiRAG inside Docker.

### Via API (alternative)

```bash
# Login as super admin
TOKEN=$(curl -s http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@thairag.local","password":"Admin123"}' | jq -r .token)

# Create the identity provider
curl -s http://localhost:8080/api/km/settings/identity-providers \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Keycloak",
    "provider_type": "oidc",
    "enabled": true,
    "config": {
      "issuer_url": "http://host.docker.internal:9090/realms/thairag",
      "client_id": "thairag-admin",
      "client_secret": "YOUR_CLIENT_SECRET_HERE",
      "scopes": "openid profile email",
      "redirect_uri": "http://localhost:8081/api/auth/oauth/callback"
    }
  }' | jq .
```

## 4. Test the OIDC Flow

### ThaiRAG Admin UI

1. Log out of the Admin UI
2. The login page should now show a **"Keycloak (OIDC)"** button below the sign-in form
3. Click it → you'll be redirected to Keycloak's login page at `localhost:9090`
4. Sign in with `testuser` / `test123`
5. You'll be redirected back to the Admin UI at `localhost:8081`, logged in as the test user
6. Check the **Users** page — the test user should appear with provider type "OIDC"

### Open WebUI

1. Open http://localhost:3000
2. Click **"Keycloak"** on the login page
3. If you already authenticated with Keycloak (SSO session active), you'll be logged in automatically
4. Otherwise, sign in with `testuser` / `test123`

## 5. Testing Entra ID Federation (Optional)

To simulate a production setup where Keycloak federates to Microsoft Entra ID:

1. In Keycloak, go to **Identity providers** → **Add provider** → **Microsoft**
2. Configure:
   - **Client ID**: your Azure AD app registration's Application (client) ID
   - **Client Secret**: a client secret from the Azure AD app registration
   - **Tenant**: your Azure AD tenant ID (or `common` for multi-tenant)
3. Click **Save**

Now when users click "Keycloak" on the login page, Keycloak will show a "Microsoft" button that redirects to Entra ID. This mirrors the Duende IdentityServer federation pattern.

## 6. Networking Notes

### URL Summary

| URL | Used by | Hostname | Why |
|-----|---------|----------|-----|
| Issuer URL | ThaiRAG backend (Docker) | `host.docker.internal:9090` | Container must reach Keycloak for discovery + token exchange |
| Redirect URI | Browser | `localhost:8081` | Browser redirects to admin-ui nginx, which proxies to ThaiRAG |
| Keycloak admin console | Browser | `localhost:9090` | Direct browser access |
| Admin UI | Browser | `localhost:8081` | Direct browser access |

### Why not `localhost` for the Issuer URL?

Inside Docker, `localhost` refers to the container itself — not the host machine. ThaiRAG's OIDC discovery call to `localhost:9090` would try to connect to port 9090 on the ThaiRAG container, which doesn't exist. Use `host.docker.internal` which Docker resolves to the host IP.

### Why `localhost` for the Redirect URI?

The redirect URI is a browser-side URL. After authenticating at Keycloak, the browser is redirected to this URL. The browser runs on your machine where `localhost:8081` reaches the admin-ui nginx container via Docker port mapping.

### Why port 8081 (nginx) not 8080 (ThaiRAG) for the Redirect URI?

The admin-ui nginx on port 8081:
1. Proxies `/api/*` requests to ThaiRAG (so the callback reaches the backend)
2. Serves the SPA for all other paths (so `/login#token=...` after callback serves the React app)

If you use port 8080 directly, the `/login#token=...` redirect after callback has no SPA to serve it.

### Running ThaiRAG outside Docker

When running `cargo run` locally (not in Docker), both the backend and browser are on the host. Use `localhost` everywhere:
- **Issuer URL**: `http://localhost:9090/realms/thairag`
- **Redirect URI**: `http://localhost:8081/api/auth/oauth/callback` (or whichever port the admin-ui dev server uses)

### Linux (no Docker Desktop)

`host.docker.internal` may not resolve by default on Linux. Either:
- Add `extra_hosts: ["host.docker.internal:host-gateway"]` to the thairag service in compose
- Or use `keycloak:9090` as the issuer URL and add `127.0.0.1 keycloak` to your host's `/etc/hosts`

### Keycloak Management Port

Keycloak 26.x uses a separate management interface on port **9000** (not exposed by default). The health endpoint is at `http://keycloak:9000/health/ready`. The `docker-compose.test-idp.yml` health check uses this port.

### HTTPS

For production, Keycloak must run behind HTTPS. For local testing, HTTP is fine — Keycloak's `start-dev` mode allows it.

## 7. Cleanup

```bash
docker compose -f docker-compose.yml -f docker-compose.test-idp.yml down -v
```

This removes all containers and volumes (Keycloak data, Open WebUI data, etc.).

## Troubleshooting

| Issue | Solution |
|-------|----------|
| "OIDC discovery failed" | The issuer URL must be reachable from inside the ThaiRAG container. Use `host.docker.internal`, **not** `localhost`. Test: `docker exec thairag-thairag-1 curl http://host.docker.internal:9090/realms/thairag/.well-known/openid-configuration` |
| "Invalid redirect URI" in Keycloak | The redirect URI in Keycloak's client settings must **exactly match** what ThaiRAG sends. Check both: ThaiRAG IdP config redirect_uri and Keycloak client Valid redirect URIs. Use `localhost:8081` (not `host.docker.internal`). |
| `DNS_PROBE_FINISHED_NXDOMAIN` for `host.docker.internal` | Add to `/etc/hosts`: `echo "127.0.0.1 host.docker.internal" \| sudo tee -a /etc/hosts` |
| "Missing authorization header" on callback | The redirect URI must go through **port 8081** (admin-ui nginx), not port 8080 (ThaiRAG directly). Nginx proxies `/api/` to ThaiRAG. |
| "Invalid or expired OAuth state" | The state parameter has a 10-minute TTL. Try the flow again. Also check for clock skew between containers. |
| "Failed to seed super admin" with column error | The Postgres database has an old schema. Rebuild: `docker compose build --no-cache thairag` then restart. The schema migration adds missing columns automatically. |
| Open WebUI shows "Internal Server Error" | Check Open WebUI logs: `docker logs thairag-open-webui-1`. Common issue: `OPENID_PROVIDER_URL` not reachable from within the container. |
| Open WebUI "Invalid client credentials" | The `OAUTH_CLIENT_SECRET` in `docker-compose.test-idp.yml` must match the Keycloak `open-webui` client's secret. Copy from Keycloak → Clients → open-webui → Credentials, update compose, then `docker compose ... up -d --force-recreate open-webui`. |
| User created but not super admin | External OIDC users are created as regular users. Only env-var-seeded users are super admins. |
| Keycloak "temporary admin" warning | Remove the Keycloak volume and restart: `docker volume rm thairag_keycloak-data`. The init script creates a permanent admin. |
