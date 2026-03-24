CREATE TABLE IF NOT EXISTS organizations (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS departments (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id          TEXT PRIMARY KEY,
    dept_id     TEXT NOT NULL REFERENCES departments(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS documents (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    mime_type     TEXT NOT NULL,
    size_bytes    INTEGER NOT NULL,
    status        TEXT NOT NULL DEFAULT 'ready',
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    error_message    TEXT,
    processing_step  TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id              TEXT PRIMARY KEY,
    email           TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    password_hash   TEXT NOT NULL DEFAULT '',
    auth_provider   TEXT NOT NULL DEFAULT 'local',
    external_id     TEXT,
    is_super_admin  INTEGER NOT NULL DEFAULT 0,
    role            TEXT NOT NULL DEFAULT 'viewer',
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS identity_providers (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    config_json   TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Document content storage (original file + converted markdown)
CREATE TABLE IF NOT EXISTS document_blobs (
    doc_id           TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    original_bytes   BLOB,
    converted_text   TEXT,
    image_count      INTEGER NOT NULL DEFAULT 0,
    table_count      INTEGER NOT NULL DEFAULT 0,
    created_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS permissions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_level    TEXT NOT NULL,
    org_id         TEXT NOT NULL DEFAULT '',
    dept_id        TEXT NOT NULL DEFAULT '',
    workspace_id   TEXT NOT NULL DEFAULT '',
    role           TEXT NOT NULL,
    UNIQUE(user_id, scope_level, org_id, dept_id, workspace_id)
);

-- Document chunks (for Tantivy re-indexing on startup)
CREATE TABLE IF NOT EXISTS document_chunks (
    chunk_id       TEXT NOT NULL PRIMARY KEY,
    doc_id         TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    workspace_id   TEXT NOT NULL,
    content        TEXT NOT NULL,
    chunk_index    INTEGER NOT NULL
);

-- MCP Connectors
CREATE TABLE IF NOT EXISTS mcp_connectors (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    transport       TEXT NOT NULL,
    command         TEXT,
    args            TEXT NOT NULL DEFAULT '[]',
    env             TEXT NOT NULL DEFAULT '{}',
    url             TEXT,
    headers         TEXT NOT NULL DEFAULT '{}',
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    sync_mode       TEXT NOT NULL DEFAULT 'on_demand',
    schedule_cron   TEXT,
    resource_filters TEXT NOT NULL DEFAULT '[]',
    max_items_per_sync INTEGER,
    tool_calls      TEXT NOT NULL DEFAULT '[]',
    webhook_url     TEXT,
    webhook_secret  TEXT,
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mcp_sync_states (
    connector_id    TEXT NOT NULL REFERENCES mcp_connectors(id) ON DELETE CASCADE,
    resource_uri    TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    doc_id          TEXT,
    last_synced_at  TEXT NOT NULL,
    source_metadata TEXT,
    PRIMARY KEY (connector_id, resource_uri)
);

CREATE TABLE IF NOT EXISTS mcp_sync_runs (
    id               TEXT PRIMARY KEY,
    connector_id     TEXT NOT NULL REFERENCES mcp_connectors(id) ON DELETE CASCADE,
    started_at       TEXT NOT NULL,
    completed_at     TEXT,
    status           TEXT NOT NULL,
    items_discovered INTEGER NOT NULL DEFAULT 0,
    items_created    INTEGER NOT NULL DEFAULT 0,
    items_updated    INTEGER NOT NULL DEFAULT 0,
    items_skipped    INTEGER NOT NULL DEFAULT 0,
    items_failed     INTEGER NOT NULL DEFAULT 0,
    error_message    TEXT
);

-- API Key Vault
CREATE TABLE IF NOT EXISTS api_key_vault (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    provider    TEXT NOT NULL,
    encrypted_key TEXT NOT NULL,
    key_prefix  TEXT NOT NULL DEFAULT '',
    key_suffix  TEXT NOT NULL DEFAULT '',
    base_url    TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- LLM Profiles
CREATE TABLE IF NOT EXISTS llm_profiles (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    model       TEXT NOT NULL,
    base_url    TEXT NOT NULL DEFAULT '',
    vault_key_id TEXT REFERENCES api_key_vault(id) ON DELETE SET NULL,
    max_tokens  INTEGER,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
