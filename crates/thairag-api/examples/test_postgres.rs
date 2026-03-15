//! Quick smoke test for PostgresKmStore against a running Postgres.
//! Run: cargo run --example test-postgres
//! Requires: docker compose up -d postgres

use chrono::Utc;
use thairag_api::store::postgres::PostgresKmStore;
use thairag_api::store::KmStoreTrait;
use thairag_core::models::{DocStatus, Document, PermissionScope, UserPermission};
use thairag_core::permission::Role;
use thairag_core::types::DocId;

#[tokio::main]
async fn main() {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://thairag:thairag@localhost:5432/thairag".into());

    println!("Connecting to {db_url} ...");
    let store = PostgresKmStore::new(&db_url, 5).await.expect("connect failed");
    println!("Connected. Running tests...\n");

    // ── Org CRUD ────────────────────────────────────────────────────
    let org = store.insert_org("TestOrg".into()).unwrap();
    println!("[OK] insert_org: {}", org.id);
    let fetched = store.get_org(org.id).unwrap();
    assert_eq!(fetched.name, "TestOrg");
    println!("[OK] get_org");
    assert_eq!(store.list_orgs().len(), 1);
    println!("[OK] list_orgs");

    // ── Dept CRUD ───────────────────────────────────────────────────
    let dept = store.insert_dept(org.id, "Engineering".into()).unwrap();
    println!("[OK] insert_dept: {}", dept.id);
    let fetched = store.get_dept(dept.id).unwrap();
    assert_eq!(fetched.name, "Engineering");
    println!("[OK] get_dept");
    assert_eq!(store.list_depts_in_org(org.id).len(), 1);
    println!("[OK] list_depts_in_org");

    // ── Workspace CRUD ──────────────────────────────────────────────
    let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
    println!("[OK] insert_workspace: {}", ws.id);
    let fetched = store.get_workspace(ws.id).unwrap();
    assert_eq!(fetched.name, "Main");
    println!("[OK] get_workspace");
    assert_eq!(store.list_workspaces_in_dept(dept.id).len(), 1);
    println!("[OK] list_workspaces_in_dept");

    // ── Document CRUD ───────────────────────────────────────────────
    let now = Utc::now();
    let doc = Document {
        id: DocId::new(),
        workspace_id: ws.id,
        title: "test.txt".into(),
        mime_type: "text/plain".into(),
        size_bytes: 42,
        status: DocStatus::Ready,
        chunk_count: 0,
        error_message: None,
        processing_step: None,
        created_at: now,
        updated_at: now,
    };
    let doc = store.insert_document(doc).unwrap();
    println!("[OK] insert_document: {}", doc.id);
    let fetched = store.get_document(doc.id).unwrap();
    assert_eq!(fetched.title, "test.txt");
    assert_eq!(fetched.size_bytes, 42);
    println!("[OK] get_document");
    assert_eq!(store.list_documents_in_workspace(ws.id).len(), 1);
    println!("[OK] list_documents_in_workspace");

    // ── User CRUD ───────────────────────────────────────────────────
    let user = store
        .insert_user("Alice@Test.com".into(), "Alice".into(), "hash123".into())
        .unwrap();
    assert_eq!(user.email, "alice@test.com");
    println!("[OK] insert_user: {}", user.id);

    let record = store.get_user_by_email("alice@test.com").unwrap();
    assert_eq!(record.password_hash, "hash123");
    println!("[OK] get_user_by_email");

    let fetched = store.get_user(user.id).unwrap();
    assert_eq!(fetched.name, "Alice");
    println!("[OK] get_user");

    // duplicate email
    let dup = store.insert_user("ALICE@test.com".into(), "A2".into(), "h".into());
    assert!(dup.is_err());
    println!("[OK] duplicate email rejected");

    assert_eq!(store.list_users().len(), 1);
    println!("[OK] list_users");

    // ── Permissions ─────────────────────────────────────────────────
    store.add_permission(UserPermission {
        user_id: user.id,
        scope: PermissionScope::Org { org_id: org.id },
        role: Role::Owner,
    });
    println!("[OK] add_permission");

    assert_eq!(store.count_org_owners(org.id), 1);
    println!("[OK] count_org_owners");

    assert_eq!(
        store.get_user_role_for_org(user.id, org.id),
        Some(Role::Owner)
    );
    println!("[OK] get_user_role_for_org");

    let perms = store.list_permissions_for_org(org.id);
    assert_eq!(perms.len(), 1);
    println!("[OK] list_permissions_for_org");

    // upsert
    let updated = store.upsert_permission(UserPermission {
        user_id: user.id,
        scope: PermissionScope::Org { org_id: org.id },
        role: Role::Admin,
    });
    assert!(updated);
    assert_eq!(
        store.get_user_role_for_org(user.id, org.id),
        Some(Role::Admin)
    );
    println!("[OK] upsert_permission");

    // workspace ids
    let ws_ids = store.get_user_workspace_ids(user.id);
    assert!(ws_ids.contains(&ws.id));
    println!("[OK] get_user_workspace_ids");

    // ── Traversal ───────────────────────────────────────────────────
    let found_org = store.org_id_for_workspace(ws.id).unwrap();
    assert_eq!(found_org, org.id);
    println!("[OK] org_id_for_workspace");

    // ── Cascade delete org ──────────────────────────────────────────
    let deleted_docs = store.cascade_delete_org(org.id).unwrap();
    assert_eq!(deleted_docs.len(), 1);
    assert_eq!(deleted_docs[0], doc.id);
    println!("[OK] cascade_delete_org (returned {} doc ids)", deleted_docs.len());

    // Verify everything is gone
    assert!(store.get_org(org.id).is_err());
    assert!(store.get_dept(dept.id).is_err());
    assert!(store.get_workspace(ws.id).is_err());
    assert!(store.get_document(doc.id).is_err());
    println!("[OK] all children cascaded");

    // Clean up user
    println!("\n All 24 checks passed!");
}
