use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use thairag_agent::{QueryOrchestrator, RagEngine};
use thairag_auth::JwtService;
use thairag_config::schema::{
    AppConfig, AuthConfig, DatabaseConfig, DocumentConfig, EmbeddingConfig, LlmConfig,
    ProvidersConfig, RerankerConfig, SearchConfig, ServerConfig, TextSearchConfig,
    VectorStoreConfig,
};
use thairag_core::traits::{EmbeddingModel, LlmProvider, Reranker, TextSearch, VectorStore};
use thairag_core::types::{ChatMessage, DocId, DocumentChunk, SearchQuery, SearchResult};
use thairag_document::DocumentPipeline;
use thairag_search::HybridSearchEngine;

use thairag_api::app_state::AppState;
use thairag_api::routes::build_router;
use thairag_api::store::KmStore;

// ── Mock Providers ──────────────────────────────────────────────────

struct MockLlm;

#[async_trait]
impl LlmProvider for MockLlm {
    async fn generate(
        &self,
        _messages: &[ChatMessage],
        _max_tokens: Option<u32>,
    ) -> thairag_core::Result<String> {
        Ok("mock response".into())
    }
    fn model_name(&self) -> &str {
        "mock-llm"
    }
}

struct MockEmbedding;

#[async_trait]
impl EmbeddingModel for MockEmbedding {
    async fn embed(&self, texts: &[String]) -> thairag_core::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0; 4]).collect())
    }
    fn dimension(&self) -> usize {
        4
    }
}

struct MockVectorStore;

#[async_trait]
impl VectorStore for MockVectorStore {
    async fn upsert(&self, _chunks: &[DocumentChunk]) -> thairag_core::Result<()> {
        Ok(())
    }
    async fn search(
        &self,
        _embedding: &[f32],
        _query: &SearchQuery,
    ) -> thairag_core::Result<Vec<SearchResult>> {
        Ok(vec![])
    }
    async fn delete_by_doc(&self, _doc_id: DocId) -> thairag_core::Result<()> {
        Ok(())
    }
}

struct MockTextSearch;

#[async_trait]
impl TextSearch for MockTextSearch {
    async fn index(&self, _chunks: &[DocumentChunk]) -> thairag_core::Result<()> {
        Ok(())
    }
    async fn search(&self, _query: &SearchQuery) -> thairag_core::Result<Vec<SearchResult>> {
        Ok(vec![])
    }
    async fn delete_by_doc(&self, _doc_id: DocId) -> thairag_core::Result<()> {
        Ok(())
    }
}

struct MockReranker;

#[async_trait]
impl Reranker for MockReranker {
    async fn rerank(
        &self,
        _query: &str,
        results: Vec<SearchResult>,
    ) -> thairag_core::Result<Vec<SearchResult>> {
        Ok(results)
    }
}

// ── Test Helpers ────────────────────────────────────────────────────

fn build_test_state(auth_enabled: bool) -> AppState {
    let llm: Arc<dyn LlmProvider> = Arc::new(MockLlm);
    let embedding: Arc<dyn EmbeddingModel> = Arc::new(MockEmbedding);
    let vector_store: Arc<dyn VectorStore> = Arc::new(MockVectorStore);
    let text_search: Arc<dyn TextSearch> = Arc::new(MockTextSearch);
    let reranker: Arc<dyn Reranker> = Arc::new(MockReranker);

    let search_config = SearchConfig {
        top_k: 5,
        rerank_top_k: 3,
        rrf_k: 60,
        vector_weight: 0.5,
        text_weight: 0.5,
    };

    let search_engine = Arc::new(HybridSearchEngine::new(
        embedding,
        vector_store,
        text_search,
        reranker,
        search_config,
    ));

    let rag_engine = Arc::new(RagEngine::new(Arc::clone(&llm), Arc::clone(&search_engine)));
    let orchestrator = Arc::new(QueryOrchestrator::new(Arc::clone(&llm), rag_engine));
    let document_pipeline = Arc::new(DocumentPipeline::new(512, 50));

    let jwt = if auth_enabled {
        Some(Arc::new(JwtService::new("test-secret-key-1234567890", 24)))
    } else {
        None
    };

    let config = AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
        },
        database: DatabaseConfig {
            url: "".into(),
            max_connections: 1,
        },
        auth: AuthConfig {
            enabled: auth_enabled,
            jwt_secret: "test-secret-key-1234567890".into(),
            token_expiry_hours: 24,
        },
        providers: ProvidersConfig {
            llm: LlmConfig {
                kind: thairag_core::types::LlmKind::Ollama,
                model: "mock".into(),
                base_url: "".into(),
                api_key: "".into(),
            },
            embedding: EmbeddingConfig {
                kind: thairag_core::types::EmbeddingKind::Fastembed,
                model: "mock".into(),
                dimension: 4,
                api_key: "".into(),
            },
            vector_store: VectorStoreConfig {
                kind: thairag_core::types::VectorStoreKind::InMemory,
                url: "".into(),
                collection: "".into(),
            },
            text_search: TextSearchConfig {
                kind: thairag_core::types::TextSearchKind::Tantivy,
                index_path: "/tmp/test-tantivy".into(),
            },
            reranker: RerankerConfig {
                kind: thairag_core::types::RerankerKind::Passthrough,
                model: "".into(),
                api_key: "".into(),
            },
        },
        search: SearchConfig {
            top_k: 5,
            rerank_top_k: 3,
            rrf_k: 60,
            vector_weight: 0.5,
            text_weight: 0.5,
        },
        document: DocumentConfig {
            max_chunk_size: 512,
            chunk_overlap: 50,
        },
    };

    AppState {
        config: Arc::new(config),
        jwt,
        orchestrator,
        document_pipeline,
        search_engine,
        km_store: Arc::new(KmStore::new()),
    }
}

fn build_app(auth_enabled: bool) -> Router {
    let state = build_test_state(auth_enabled);
    build_router(state)
}

async fn body_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn json_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn json_request_auth(
    method: &str,
    uri: &str,
    body: serde_json::Value,
    token: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn get_request_auth(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn delete_request_auth(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// Register a user, login, and return the token.
async fn register_and_get_token(
    app: &Router,
    email: &str,
    name: &str,
    password: &str,
) -> String {
    // Register
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({ "email": email, "name": name, "password": password }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Login
    let req = json_request(
        "POST",
        "/api/auth/login",
        serde_json::json!({ "email": email, "password": password }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    body["token"].as_str().unwrap().to_string()
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn health_check() {
    let app = build_app(true);
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn register_and_login() {
    let app = build_app(true);

    // Register
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({
            "email": "alice@test.com",
            "name": "Alice",
            "password": "secret123"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["email"], "alice@test.com");
    assert_eq!(body["name"], "Alice");

    // Login
    let req = json_request(
        "POST",
        "/api/auth/login",
        serde_json::json!({
            "email": "alice@test.com",
            "password": "secret123"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert!(body["token"].as_str().is_some());
    assert_eq!(body["user"]["email"], "alice@test.com");
}

#[tokio::test]
async fn login_wrong_password() {
    let app = build_app(true);

    // Register first
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({
            "email": "bob@test.com",
            "name": "Bob",
            "password": "correct-password"
        }),
    );
    app.clone().oneshot(req).await.unwrap();

    // Login with wrong password
    let req = json_request(
        "POST",
        "/api/auth/login",
        serde_json::json!({
            "email": "bob@test.com",
            "password": "wrong-password"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn km_requires_auth() {
    let app = build_app(true);

    // POST without auth header should get 401
    let req = json_request(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "TestOrg" }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn km_crud_flow() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "admin@test.com", "Admin", "pass123").await;

    // Create org
    let req = json_request_auth(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "TestOrg" }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let org_id = body["id"].as_str().unwrap().to_string();

    // Create dept
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts"),
        serde_json::json!({ "name": "Engineering" }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let dept_id = body["id"].as_str().unwrap().to_string();

    // Create workspace
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        serde_json::json!({ "name": "Main" }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let ws_id = body["id"].as_str().unwrap().to_string();

    // List orgs
    let req = get_request_auth("/api/km/orgs", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // List depts
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/depts"), &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // List workspaces
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // Delete workspace
    let req = delete_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify workspace gone
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn permission_enforcement() {
    let app = build_app(true);

    // User A creates org (becomes Owner)
    let token_a = register_and_get_token(&app, "a@test.com", "A", "pass").await;
    let req = json_request_auth(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "OrgA" }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let org_id = body["id"].as_str().unwrap().to_string();

    // User B registers (no permissions on OrgA)
    let token_b = register_and_get_token(&app, "b@test.com", "B", "pass").await;

    // User B can still list orgs (no specific permission needed)
    let req = get_request_auth("/api/km/orgs", &token_b);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // User B tries to read the org → 403 (no permissions)
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}"), &token_b);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // User B tries to delete org → 403
    let req = delete_request_auth(&format!("/api/km/orgs/{org_id}"), &token_b);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // User A can read their own org
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}"), &token_a);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // User A can delete their own org (Owner)
    let req = delete_request_auth(&format!("/api/km/orgs/{org_id}"), &token_a);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn anonymous_access() {
    let app = build_app(false);

    // KM ops work without token when auth is disabled
    let req = json_request(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "AnonOrg" }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let org_id = body["id"].as_str().unwrap().to_string();

    // List orgs without auth
    let req = Request::builder()
        .uri("/api/km/orgs")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // Get org without auth
    let req = Request::builder()
        .uri(&format!("/api/km/orgs/{org_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Delete org without auth
    let req = Request::builder()
        .method("DELETE")
        .uri(&format!("/api/km/orgs/{org_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn document_crud() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "doc@test.com", "DocUser", "pass").await;

    // Create full hierarchy: org → dept → workspace
    let req = json_request_auth(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "DocOrg" }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let org_id = body["id"].as_str().unwrap().to_string();

    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts"),
        serde_json::json!({ "name": "Eng" }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let dept_id = body["id"].as_str().unwrap().to_string();

    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        serde_json::json!({ "name": "Main" }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let ws_id = body["id"].as_str().unwrap().to_string();

    // Ingest document
    let req = json_request_auth(
        "POST",
        &format!("/api/km/workspaces/{ws_id}/documents"),
        serde_json::json!({
            "title": "Test Doc",
            "content": "Hello world this is a test document.",
            "mime_type": "text/plain"
        }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let doc_id = body["doc_id"].as_str().unwrap().to_string();
    assert!(body["chunks"].as_u64().unwrap() >= 1);

    // List documents
    let req = get_request_auth(
        &format!("/api/km/workspaces/{ws_id}/documents"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // Get document
    let req = get_request_auth(
        &format!("/api/km/workspaces/{ws_id}/documents/{doc_id}"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["title"], "Test Doc");

    // Delete document
    let req = delete_request_auth(
        &format!("/api/km/workspaces/{ws_id}/documents/{doc_id}"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify gone
    let req = get_request_auth(
        &format!("/api/km/workspaces/{ws_id}/documents"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);
}
