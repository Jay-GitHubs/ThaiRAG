CREATE TABLE IF NOT EXISTS organizations (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS departments (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_departments_org_id ON departments(org_id);

CREATE TABLE IF NOT EXISTS workspaces (
    id          UUID PRIMARY KEY,
    dept_id     UUID NOT NULL REFERENCES departments(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workspaces_dept_id ON workspaces(dept_id);

CREATE TABLE IF NOT EXISTS documents (
    id            UUID PRIMARY KEY,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    mime_type     TEXT NOT NULL,
    size_bytes    BIGINT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'ready',
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    error_message    TEXT,
    processing_step  TEXT,
    created_at       TIMESTAMPTZ NOT NULL,
    updated_at       TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_documents_workspace_id ON documents(workspace_id);

CREATE TABLE IF NOT EXISTS users (
    id              UUID PRIMARY KEY,
    email           TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    password_hash   TEXT NOT NULL DEFAULT '',
    auth_provider   TEXT NOT NULL DEFAULT 'local',
    external_id     TEXT,
    is_super_admin  BOOLEAN NOT NULL DEFAULT FALSE,
    role            TEXT NOT NULL DEFAULT 'viewer',
    created_at      TIMESTAMPTZ NOT NULL
);

-- Migrations for existing databases
ALTER TABLE users ADD COLUMN IF NOT EXISTS auth_provider TEXT NOT NULL DEFAULT 'local';
ALTER TABLE users ADD COLUMN IF NOT EXISTS external_id TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_super_admin BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE users ALTER COLUMN password_hash SET DEFAULT '';
ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'viewer';
UPDATE users SET role = 'super_admin' WHERE is_super_admin = TRUE AND role = 'viewer';

ALTER TABLE documents ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'ready';
ALTER TABLE documents ADD COLUMN IF NOT EXISTS chunk_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS error_message TEXT;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS processing_step TEXT;

CREATE TABLE IF NOT EXISTS identity_providers (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL,
    enabled       BOOLEAN NOT NULL DEFAULT TRUE,
    config_json   TEXT NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key         TEXT NOT NULL,
    scope_type  TEXT NOT NULL DEFAULT 'global',
    scope_id    TEXT NOT NULL DEFAULT '',
    value       TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (key, scope_type, scope_id)
);

CREATE TABLE IF NOT EXISTS permissions (
    id             SERIAL PRIMARY KEY,
    user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_level    TEXT NOT NULL,
    org_id         TEXT NOT NULL DEFAULT '',
    dept_id        TEXT NOT NULL DEFAULT '',
    workspace_id   TEXT NOT NULL DEFAULT '',
    role           TEXT NOT NULL,
    UNIQUE(user_id, scope_level, org_id, dept_id, workspace_id)
);
CREATE INDEX IF NOT EXISTS idx_permissions_user_id ON permissions(user_id);
CREATE INDEX IF NOT EXISTS idx_permissions_org_id ON permissions(org_id);

-- Migration: settings table from single-PK to composite-PK for scoped settings
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'settings' AND column_name = 'scope_type'
    ) THEN
        ALTER TABLE settings ADD COLUMN scope_type TEXT NOT NULL DEFAULT 'global';
        ALTER TABLE settings ADD COLUMN scope_id TEXT NOT NULL DEFAULT '';
        ALTER TABLE settings DROP CONSTRAINT settings_pkey;
        ALTER TABLE settings ADD PRIMARY KEY (key, scope_type, scope_id);
    END IF;
END $$;

-- Document content storage (original file + converted markdown)
CREATE TABLE IF NOT EXISTS document_blobs (
    doc_id           UUID PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    original_bytes   BYTEA,
    converted_text   TEXT,
    image_count      INTEGER NOT NULL DEFAULT 0,
    table_count      INTEGER NOT NULL DEFAULT 0,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Document chunks (for Tantivy re-indexing on startup)
CREATE TABLE IF NOT EXISTS document_chunks (
    chunk_id       UUID NOT NULL,
    doc_id         UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    workspace_id   UUID NOT NULL,
    content        TEXT NOT NULL,
    chunk_index    INTEGER NOT NULL,
    PRIMARY KEY (chunk_id)
);
CREATE INDEX IF NOT EXISTS idx_document_chunks_doc_id ON document_chunks(doc_id);

-- MCP Connectors
CREATE TABLE IF NOT EXISTS mcp_connectors (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    transport       TEXT NOT NULL,
    command         TEXT,
    args            TEXT NOT NULL DEFAULT '[]',
    env             TEXT NOT NULL DEFAULT '{}',
    url             TEXT,
    headers         TEXT NOT NULL DEFAULT '{}',
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    sync_mode       TEXT NOT NULL DEFAULT 'on_demand',
    schedule_cron   TEXT,
    resource_filters TEXT NOT NULL DEFAULT '[]',
    max_items_per_sync INTEGER,
    tool_calls      TEXT NOT NULL DEFAULT '[]',
    webhook_url     TEXT,
    webhook_secret  TEXT,
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mcp_connectors_workspace_id ON mcp_connectors(workspace_id);

CREATE TABLE IF NOT EXISTS mcp_sync_states (
    connector_id    UUID NOT NULL REFERENCES mcp_connectors(id) ON DELETE CASCADE,
    resource_uri    TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    doc_id          UUID,
    last_synced_at  TIMESTAMPTZ NOT NULL,
    source_metadata TEXT,
    PRIMARY KEY (connector_id, resource_uri)
);

CREATE TABLE IF NOT EXISTS mcp_sync_runs (
    id               UUID PRIMARY KEY,
    connector_id     UUID NOT NULL REFERENCES mcp_connectors(id) ON DELETE CASCADE,
    started_at       TIMESTAMPTZ NOT NULL,
    completed_at     TIMESTAMPTZ,
    status           TEXT NOT NULL,
    items_discovered INTEGER NOT NULL DEFAULT 0,
    items_created    INTEGER NOT NULL DEFAULT 0,
    items_updated    INTEGER NOT NULL DEFAULT 0,
    items_skipped    INTEGER NOT NULL DEFAULT 0,
    items_failed     INTEGER NOT NULL DEFAULT 0,
    error_message    TEXT
);
CREATE INDEX IF NOT EXISTS idx_mcp_sync_runs_connector_id ON mcp_sync_runs(connector_id);

-- API Key Vault
CREATE TABLE IF NOT EXISTS api_key_vault (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    provider    TEXT NOT NULL,
    encrypted_key TEXT NOT NULL,
    key_prefix  TEXT NOT NULL DEFAULT '',
    key_suffix  TEXT NOT NULL DEFAULT '',
    base_url    TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
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
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);
