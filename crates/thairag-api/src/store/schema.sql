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
