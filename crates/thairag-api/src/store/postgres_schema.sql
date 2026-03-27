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
    disabled        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL
);

-- Migrations for existing databases
ALTER TABLE users ADD COLUMN IF NOT EXISTS auth_provider TEXT NOT NULL DEFAULT 'local';
ALTER TABLE users ADD COLUMN IF NOT EXISTS external_id TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_super_admin BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE users ALTER COLUMN password_hash SET DEFAULT '';
ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'viewer';
ALTER TABLE users ADD COLUMN IF NOT EXISTS disabled BOOLEAN NOT NULL DEFAULT FALSE;
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

-- Document version history
CREATE TABLE IF NOT EXISTS document_versions (
    id              UUID PRIMARY KEY,
    doc_id          UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    version_number  INTEGER NOT NULL,
    title           TEXT NOT NULL,
    content         TEXT,
    content_hash    TEXT NOT NULL,
    mime_type       TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL,
    created_by      UUID,
    UNIQUE(doc_id, version_number)
);
CREATE INDEX IF NOT EXISTS idx_document_versions_doc_id ON document_versions(doc_id);

-- Add version and content_hash columns to documents
ALTER TABLE documents ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS content_hash TEXT;

-- Add scheduled refresh columns to documents
ALTER TABLE documents ADD COLUMN IF NOT EXISTS source_url TEXT;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS refresh_schedule TEXT;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS last_refreshed_at TIMESTAMPTZ;

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

-- Inference Logs (per-request LLM call telemetry)
CREATE TABLE IF NOT EXISTS inference_logs (
    id                TEXT PRIMARY KEY,
    timestamp         TIMESTAMPTZ NOT NULL,
    user_id           TEXT,
    workspace_id      TEXT,
    org_id            TEXT,
    dept_id           TEXT,
    session_id        TEXT,
    response_id       TEXT NOT NULL,
    query_text        TEXT NOT NULL,
    detected_language TEXT,
    intent            TEXT,
    complexity        TEXT,
    llm_kind          TEXT NOT NULL,
    llm_model         TEXT NOT NULL,
    settings_scope    TEXT NOT NULL DEFAULT 'global',
    prompt_tokens     INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_ms          INTEGER NOT NULL DEFAULT 0,
    search_ms         INTEGER,
    generation_ms     INTEGER,
    chunks_retrieved  INTEGER,
    avg_chunk_score   REAL,
    self_rag_decision TEXT,
    self_rag_confidence REAL,
    quality_guard_pass BOOLEAN,
    relevance_score    REAL,
    hallucination_score REAL,
    completeness_score REAL,
    pipeline_route    TEXT,
    agents_used       TEXT NOT NULL DEFAULT '[]',
    status            TEXT NOT NULL DEFAULT 'success',
    error_message     TEXT,
    response_length   INTEGER NOT NULL DEFAULT 0,
    feedback_score    INTEGER
);
CREATE INDEX IF NOT EXISTS idx_inference_logs_timestamp ON inference_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_inference_logs_workspace_id ON inference_logs(workspace_id);
CREATE INDEX IF NOT EXISTS idx_inference_logs_response_id ON inference_logs(response_id);

-- API Keys (M2M authentication)
CREATE TABLE IF NOT EXISTS api_keys (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    key_hash      TEXT NOT NULL UNIQUE,
    key_prefix    TEXT NOT NULL DEFAULT '',
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role          TEXT NOT NULL DEFAULT 'viewer',
    created_at    TIMESTAMPTZ NOT NULL,
    last_used_at  TIMESTAMPTZ,
    is_active     BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_user_id ON api_keys(user_id);

-- Workspace ACLs (fine-grained workspace-level access control)
CREATE TABLE IF NOT EXISTS workspace_acls (
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    permission    TEXT NOT NULL DEFAULT 'read',
    granted_at    TIMESTAMPTZ NOT NULL,
    granted_by    UUID,
    UNIQUE(user_id, workspace_id)
);
CREATE INDEX IF NOT EXISTS idx_workspace_acls_workspace_id ON workspace_acls(workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_acls_user_id ON workspace_acls(user_id);

-- Document ACLs (fine-grained document-level access control)
CREATE TABLE IF NOT EXISTS document_acls (
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    doc_id        UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    permission    TEXT NOT NULL DEFAULT 'read',
    granted_at    TIMESTAMPTZ NOT NULL,
    UNIQUE(user_id, doc_id)
);
CREATE INDEX IF NOT EXISTS idx_document_acls_doc_id ON document_acls(doc_id);
CREATE INDEX IF NOT EXISTS idx_document_acls_user_id ON document_acls(user_id);

-- Knowledge Graph: Entities
CREATE TABLE IF NOT EXISTS entities (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    entity_type   TEXT NOT NULL,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    metadata      TEXT NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_entities_workspace_id ON entities(workspace_id);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);

-- Knowledge Graph: Entity-Document links (many-to-many)
CREATE TABLE IF NOT EXISTS entity_doc_links (
    entity_id     UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    doc_id        UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    PRIMARY KEY (entity_id, doc_id)
);
CREATE INDEX IF NOT EXISTS idx_entity_doc_links_doc_id ON entity_doc_links(doc_id);

-- Knowledge Graph: Relations between entities
CREATE TABLE IF NOT EXISTS relations (
    id              UUID PRIMARY KEY,
    from_entity_id  UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    to_entity_id    UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relation_type   TEXT NOT NULL,
    confidence      REAL NOT NULL DEFAULT 1.0,
    doc_id          UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_relations_from ON relations(from_entity_id);
CREATE INDEX IF NOT EXISTS idx_relations_to ON relations(to_entity_id);
CREATE INDEX IF NOT EXISTS idx_relations_doc_id ON relations(doc_id);

-- Search Analytics Events
CREATE TABLE IF NOT EXISTS search_analytics_events (
    id             TEXT PRIMARY KEY,
    timestamp      TIMESTAMPTZ NOT NULL,
    query_text     TEXT NOT NULL,
    user_id        TEXT,
    workspace_id   TEXT,
    result_count   INTEGER NOT NULL DEFAULT 0,
    latency_ms     BIGINT NOT NULL DEFAULT 0,
    zero_results   BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS idx_search_events_timestamp ON search_analytics_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_search_events_workspace_id ON search_analytics_events(workspace_id);

-- Document Lineage Records
CREATE TABLE IF NOT EXISTS lineage_records (
    id                  TEXT PRIMARY KEY,
    response_id         TEXT NOT NULL,
    timestamp           TIMESTAMPTZ NOT NULL,
    query_text          TEXT NOT NULL,
    chunk_id            TEXT NOT NULL,
    doc_id              TEXT NOT NULL,
    doc_title           TEXT,
    chunk_text_preview  TEXT NOT NULL DEFAULT '',
    score               DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    rank                INTEGER NOT NULL DEFAULT 0,
    contributed         BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS idx_lineage_response_id ON lineage_records(response_id);
CREATE INDEX IF NOT EXISTS idx_lineage_doc_id ON lineage_records(doc_id);

-- Personal Memory (DB-backed per-user memory)
CREATE TABLE IF NOT EXISTS personal_memories (
    id                TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    memory_type       TEXT NOT NULL DEFAULT 'general',
    summary           TEXT NOT NULL,
    topics            TEXT NOT NULL DEFAULT '[]',
    importance        DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    relevance_score   DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    created_at        TIMESTAMPTZ NOT NULL,
    last_accessed_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_personal_memories_user_id ON personal_memories(user_id);

-- Multi-tenancy
CREATE TABLE IF NOT EXISTS tenants (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    plan        TEXT NOT NULL DEFAULT 'free',
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS tenant_quotas (
    tenant_id           TEXT PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    max_documents       BIGINT NOT NULL DEFAULT 1000,
    max_storage_bytes   BIGINT NOT NULL DEFAULT 10737418240,
    max_queries_per_day BIGINT NOT NULL DEFAULT 10000,
    max_users           BIGINT NOT NULL DEFAULT 50,
    max_workspaces      BIGINT NOT NULL DEFAULT 20
);

CREATE TABLE IF NOT EXISTS tenant_org_mapping (
    org_id      TEXT PRIMARY KEY,
    tenant_id   TEXT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE
);

-- RBAC v2: Custom Roles
CREATE TABLE IF NOT EXISTS custom_roles (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    permissions TEXT NOT NULL DEFAULT '[]',
    is_system   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL
);

-- Document Collaboration
CREATE TABLE IF NOT EXISTS document_comments (
    id          TEXT PRIMARY KEY,
    doc_id      TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    user_name   TEXT,
    text        TEXT NOT NULL,
    parent_id   TEXT,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_doc_comments_doc_id ON document_comments(doc_id);

CREATE TABLE IF NOT EXISTS document_annotations (
    id              TEXT PRIMARY KEY,
    doc_id          TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    user_name       TEXT,
    chunk_id        TEXT,
    text            TEXT NOT NULL,
    highlight_start INTEGER,
    highlight_end   INTEGER,
    created_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_doc_annotations_doc_id ON document_annotations(doc_id);

CREATE TABLE IF NOT EXISTS document_reviews (
    id            TEXT PRIMARY KEY,
    doc_id        TEXT NOT NULL,
    reviewer_id   TEXT NOT NULL,
    reviewer_name TEXT,
    status        TEXT NOT NULL DEFAULT 'pending',
    comments      TEXT,
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_doc_reviews_doc_id ON document_reviews(doc_id);

-- Search Quality Regression
CREATE TABLE IF NOT EXISTS regression_runs (
    id              TEXT PRIMARY KEY,
    timestamp       TIMESTAMPTZ NOT NULL,
    query_set_id    TEXT NOT NULL,
    baseline_score  DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    current_score   DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    degradation     DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    passed          BOOLEAN NOT NULL DEFAULT TRUE,
    details         TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_regression_runs_timestamp ON regression_runs(timestamp DESC);

-- Prompt Marketplace
CREATE TABLE IF NOT EXISTS prompt_templates (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    category        TEXT NOT NULL DEFAULT 'general',
    content         TEXT NOT NULL,
    variables       TEXT NOT NULL DEFAULT '[]',
    author_id       TEXT,
    author_name     TEXT,
    version         INTEGER NOT NULL DEFAULT 1,
    is_public       BOOLEAN NOT NULL DEFAULT TRUE,
    rating_avg      DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    rating_count    INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_prompt_templates_category ON prompt_templates(category);
CREATE INDEX IF NOT EXISTS idx_prompt_templates_author ON prompt_templates(author_id);

CREATE TABLE IF NOT EXISTS prompt_ratings (
    template_id TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    rating      INTEGER NOT NULL,
    PRIMARY KEY (template_id, user_id)
);

-- Embedding Fine-tuning
CREATE TABLE IF NOT EXISTS training_datasets (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    pair_count  INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS training_pairs (
    id           TEXT PRIMARY KEY,
    dataset_id   TEXT NOT NULL REFERENCES training_datasets(id) ON DELETE CASCADE,
    query        TEXT NOT NULL,
    positive_doc TEXT NOT NULL,
    negative_doc TEXT,
    created_at   TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_training_pairs_dataset ON training_pairs(dataset_id);

CREATE TABLE IF NOT EXISTS finetune_jobs (
    id                  TEXT PRIMARY KEY,
    dataset_id          TEXT NOT NULL,
    base_model          TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending',
    metrics             TEXT,
    output_model_path   TEXT,
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_finetune_jobs_dataset ON finetune_jobs(dataset_id);
