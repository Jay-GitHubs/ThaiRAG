//! Backup & restore engine — creates/restores ZIP archives of system state.

use std::io::{Cursor, Read as _, Write as _};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zip::ZipArchive;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, Document, IdentityProvider, Organization, User, UserPermission, Workspace,
};
use thairag_core::types::{BackupIncludes, BackupManifest, BackupStats};

use crate::store::KmStoreTrait;

/// Current backup format version.
const BACKUP_VERSION: &str = "1.0";

// ── Backup Data Structures ──────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SettingsData {
    global: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct UsersData {
    users: Vec<User>,
}

#[derive(Serialize, Deserialize)]
struct OrgStructureData {
    orgs: Vec<Organization>,
    depts: Vec<Department>,
    workspaces: Vec<Workspace>,
    permissions: Vec<UserPermission>,
    identity_providers: Vec<IdentityProvider>,
}

#[derive(Serialize, Deserialize)]
struct DocumentsData {
    documents: Vec<Document>,
}

// ── Restore Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct RestoreOptions {
    #[serde(default = "default_true")]
    pub settings: bool,
    #[serde(default = "default_true")]
    pub users: bool,
    #[serde(default = "default_true")]
    pub org_structure: bool,
    #[serde(default)]
    pub skip_existing: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            settings: true,
            users: true,
            org_structure: true,
            skip_existing: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RestoreResult {
    pub restored_settings: usize,
    pub restored_users: usize,
    pub restored_orgs: usize,
    pub restored_depts: usize,
    pub restored_workspaces: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

// ── Preview Result ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct BackupPreview {
    pub manifest: BackupManifest,
    pub files: Vec<String>,
}

// ── Create Backup ───────────────────────────────────────────────────

pub fn create_backup(
    store: &dyn KmStoreTrait,
    includes: &BackupIncludes,
) -> Result<Vec<u8>, ThaiRagError> {
    let buf = Vec::new();
    let mut zip = ZipWriter::new(Cursor::new(buf));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let mut stats = BackupStats::default();

    // ── Settings ────────────────────────────────────────────────────
    if includes.settings {
        let all_settings = store.list_all_settings();
        stats.settings_count = all_settings.len();

        let data = SettingsData {
            global: all_settings,
        };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| ThaiRagError::Internal(format!("Failed to serialize settings: {e}")))?;
        zip.start_file("settings.json", options)
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
        zip.write_all(json.as_bytes())
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
    }

    // ── Users ───────────────────────────────────────────────────────
    if includes.users {
        let users = store.list_users();
        stats.users_count = users.len();

        let data = UsersData { users };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| ThaiRagError::Internal(format!("Failed to serialize users: {e}")))?;
        zip.start_file("users.json", options)
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
        zip.write_all(json.as_bytes())
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
    }

    // ── Org Structure ───────────────────────────────────────────────
    if includes.org_structure {
        let orgs = store.list_orgs();
        stats.orgs_count = orgs.len();

        let mut depts = Vec::new();
        for org in &orgs {
            depts.extend(store.list_depts_in_org(org.id));
        }
        stats.depts_count = depts.len();

        let workspaces = store.list_workspaces_all();
        stats.workspaces_count = workspaces.len();

        // Collect all permissions across all orgs
        let mut permissions = Vec::new();
        for org in &orgs {
            permissions.extend(store.list_permissions_for_org(org.id));
        }

        let identity_providers = store.list_identity_providers();

        let data = OrgStructureData {
            orgs,
            depts,
            workspaces,
            permissions,
            identity_providers,
        };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| ThaiRagError::Internal(format!("Failed to serialize orgs: {e}")))?;
        zip.start_file("orgs.json", options)
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
        zip.write_all(json.as_bytes())
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
    }

    // ── Documents (metadata only) ───────────────────────────────────
    if includes.documents {
        let workspaces = store.list_workspaces_all();
        let mut documents = Vec::new();
        for ws in &workspaces {
            documents.extend(store.list_documents_in_workspace(ws.id));
        }
        stats.documents_count = documents.len();

        let data = DocumentsData { documents };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| ThaiRagError::Internal(format!("Failed to serialize documents: {e}")))?;
        zip.start_file("documents.json", options)
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
        zip.write_all(json.as_bytes())
            .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
    }

    // ── Manifest ────────────────────────────────────────────────────
    let manifest = BackupManifest {
        version: BACKUP_VERSION.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        includes: includes.clone(),
        stats,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| ThaiRagError::Internal(format!("Failed to serialize manifest: {e}")))?;
    zip.start_file("manifest.json", options)
        .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;
    zip.write_all(manifest_json.as_bytes())
        .map_err(|e| ThaiRagError::Internal(format!("ZIP write error: {e}")))?;

    let cursor = zip
        .finish()
        .map_err(|e| ThaiRagError::Internal(format!("ZIP finalize error: {e}")))?;
    Ok(cursor.into_inner())
}

// ── Preview Backup (dry-run) ────────────────────────────────────────

pub fn preview_backup(data: &[u8]) -> Result<BackupPreview, ThaiRagError> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| ThaiRagError::Validation(format!("Invalid ZIP archive: {e}")))?;

    // Collect file names
    let files: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    // Read and parse manifest
    let manifest = read_manifest(&mut archive)?;

    Ok(BackupPreview { manifest, files })
}

// ── Restore Backup ──────────────────────────────────────────────────

pub fn restore_backup(
    store: Arc<dyn KmStoreTrait>,
    data: &[u8],
    options: &RestoreOptions,
) -> Result<RestoreResult, ThaiRagError> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| ThaiRagError::Validation(format!("Invalid ZIP archive: {e}")))?;

    let manifest = read_manifest(&mut archive)?;

    // Validate version compatibility
    if !manifest.version.starts_with("1.") {
        return Err(ThaiRagError::Validation(format!(
            "Unsupported backup version: {}. This server supports version 1.x",
            manifest.version
        )));
    }

    let mut result = RestoreResult::default();

    // ── Restore Settings ────────────────────────────────────────────
    if options.settings
        && manifest.includes.settings
        && let Some(data) = read_zip_file(&mut archive, "settings.json")?
    {
        let settings_data: SettingsData = serde_json::from_str(&data)
            .map_err(|e| ThaiRagError::Validation(format!("Invalid settings.json: {e}")))?;

        for (key, value) in &settings_data.global {
            if options.skip_existing && store.get_setting(key).is_some() {
                result.skipped += 1;
                continue;
            }
            store.set_setting(key, value);
            result.restored_settings += 1;
        }
    }

    // ── Restore Users ───────────────────────────────────────────────
    if options.users
        && manifest.includes.users
        && let Some(data) = read_zip_file(&mut archive, "users.json")?
    {
        let users_data: UsersData = serde_json::from_str(&data)
            .map_err(|e| ThaiRagError::Validation(format!("Invalid users.json: {e}")))?;

        for user in &users_data.users {
            if options.skip_existing && store.get_user(user.id).is_ok() {
                result.skipped += 1;
                continue;
            }
            // Use upsert to handle both insert and update cases
            match store.upsert_user_by_email(
                user.email.clone(),
                user.name.clone(),
                // We don't have the password hash in the User model export,
                // so we set a placeholder. Admin will need to reset passwords.
                "RESTORED_NO_PASSWORD".to_string(),
                user.is_super_admin,
                user.role.clone(),
            ) {
                Ok(_) => result.restored_users += 1,
                Err(e) => result
                    .errors
                    .push(format!("Failed to restore user {}: {e}", user.email)),
            }
        }
    }

    // ── Restore Org Structure ───────────────────────────────────────
    if options.org_structure
        && manifest.includes.org_structure
        && let Some(data) = read_zip_file(&mut archive, "orgs.json")?
    {
        let org_data: OrgStructureData = serde_json::from_str(&data)
            .map_err(|e| ThaiRagError::Validation(format!("Invalid orgs.json: {e}")))?;

        // Restore orgs
        for org in &org_data.orgs {
            if options.skip_existing && store.get_org(org.id).is_ok() {
                result.skipped += 1;
                continue;
            }
            match store.insert_org(org.name.clone()) {
                Ok(_) => result.restored_orgs += 1,
                Err(e) => result
                    .errors
                    .push(format!("Failed to restore org {}: {e}", org.name)),
            }
        }

        // Restore departments
        for dept in &org_data.depts {
            if options.skip_existing && store.get_dept(dept.id).is_ok() {
                result.skipped += 1;
                continue;
            }
            match store.insert_dept(dept.org_id, dept.name.clone()) {
                Ok(_) => result.restored_depts += 1,
                Err(e) => result
                    .errors
                    .push(format!("Failed to restore dept {}: {e}", dept.name)),
            }
        }

        // Restore workspaces
        for ws in &org_data.workspaces {
            if options.skip_existing && store.get_workspace(ws.id).is_ok() {
                result.skipped += 1;
                continue;
            }
            match store.insert_workspace(ws.dept_id, ws.name.clone()) {
                Ok(_) => result.restored_workspaces += 1,
                Err(e) => result
                    .errors
                    .push(format!("Failed to restore workspace {}: {e}", ws.name)),
            }
        }

        // Restore permissions (fire and forget — duplicates are harmless)
        for perm in &org_data.permissions {
            store.upsert_permission(perm.clone());
        }

        // Restore identity providers
        for idp in &org_data.identity_providers {
            if options.skip_existing && store.get_identity_provider(idp.id).is_ok() {
                result.skipped += 1;
                continue;
            }
            match store.insert_identity_provider(
                idp.name.clone(),
                idp.provider_type.clone(),
                idp.enabled,
                idp.config.clone(),
            ) {
                Ok(_) => {}
                Err(e) => result
                    .errors
                    .push(format!("Failed to restore IDP {}: {e}", idp.name)),
            }
        }
    }

    Ok(result)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn read_manifest(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<BackupManifest, ThaiRagError> {
    let data = read_zip_file(archive, "manifest.json")?.ok_or_else(|| {
        ThaiRagError::Validation("Invalid backup: missing manifest.json".to_string())
    })?;
    serde_json::from_str(&data)
        .map_err(|e| ThaiRagError::Validation(format!("Invalid manifest.json: {e}")))
}

fn read_zip_file(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>, ThaiRagError> {
    let mut file = match archive.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => {
            return Err(ThaiRagError::Internal(format!(
                "Failed to read {name} from ZIP: {e}"
            )));
        }
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ThaiRagError::Internal(format!("Failed to read {name}: {e}")))?;
    Ok(Some(contents))
}
