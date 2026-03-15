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
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
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

-- Document content storage (original file + converted markdown)
CREATE TABLE IF NOT EXISTS document_blobs (
    doc_id           UUID PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    original_bytes   BYTEA,
    converted_text   TEXT,
    image_count      INTEGER NOT NULL DEFAULT 0,
    table_count      INTEGER NOT NULL DEFAULT 0,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
