use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use thairag_agent::{QueryOrchestrator, RagEngine};
use thairag_auth::JwtService;
use thairag_config::schema::{
    AppConfig, AuthConfig, DatabaseConfig, DocumentConfig, EmbeddingConfig, LlmConfig,
    ProvidersConfig, RateLimitConfig, RerankerConfig, SearchConfig, ServerConfig, TextSearchConfig,
    VectorStoreConfig,
};
use thairag_core::traits::{EmbeddingModel, LlmProvider, Reranker, TextSearch, VectorStore};
use thairag_core::types::{
    ChatMessage, DocId, DocumentChunk, LlmResponse, LlmStreamResponse, LlmUsage, SearchQuery,
    SearchResult,
};
use thairag_document::DocumentPipeline;
use thairag_search::HybridSearchEngine;

use thairag_api::app_state::{AppState, ProviderBundle};
use thairag_api::routes::build_router;
use thairag_api::store::KmStoreTrait;
use thairag_api::store::memory::MemoryKmStore;

// ── Mock Providers ──────────────────────────────────────────────────

struct MockLlm;

#[async_trait]
impl LlmProvider for MockLlm {
    async fn generate(
        &self,
        _messages: &[ChatMessage],
        _max_tokens: Option<u32>,
    ) -> thairag_core::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "mock response".into(),
            usage: LlmUsage::default(),
        })
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
        Arc::clone(&embedding),
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
            shutdown_timeout_secs: 5,
            rate_limit: RateLimitConfig {
                enabled: false,
                requests_per_second: 10,
                burst_size: 20,
            },
            cors_origins: vec![],
            trust_proxy: false,
            max_chat_messages: 50,
            max_message_length: 32000,
            request_timeout_secs: 600,
        },
        database: DatabaseConfig {
            url: "".into(),
            max_connections: 1,
        },
        auth: AuthConfig {
            enabled: auth_enabled,
            jwt_secret: "test-secret-key-1234567890".into(),
            token_expiry_hours: 24,
            password_min_length: 8,
            max_login_attempts: 5,
            lockout_duration_secs: 300,
            api_keys: String::new(),
        },
        providers: ProvidersConfig {
            llm: LlmConfig {
                kind: thairag_core::types::LlmKind::Ollama,
                model: "mock".into(),
                base_url: "".into(),
                api_key: "".into(),
                max_tokens: None,
                profile_id: None,
            },
            embedding: EmbeddingConfig {
                kind: thairag_core::types::EmbeddingKind::Fastembed,
                model: "mock".into(),
                dimension: 4,
                base_url: "".into(),
                api_key: "".into(),
            },
            vector_store: VectorStoreConfig {
                kind: thairag_core::types::VectorStoreKind::InMemory,
                url: "".into(),
                collection: "".into(),
                api_key: "".into(),
                isolation: Default::default(),
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
            max_upload_size_mb: 50,
            ai_preprocessing: Default::default(),
        },
        chat_pipeline: Default::default(),
        mcp: Default::default(),
        session: Default::default(),
        embedding_cache: Default::default(),
        job_queue: Default::default(),
        redis: Default::default(),
    };

    let bundle = ProviderBundle {
        providers_config: config.providers.clone(),
        chat_pipeline_config: config.chat_pipeline.clone(),
        orchestrator,
        chat_pipeline: None,
        document_pipeline,
        search_engine,
        embedding,
        context_compactor: None,
        personal_memory_manager: None,
    };

    AppState::from_parts(
        Arc::new(config),
        jwt,
        Arc::new(MemoryKmStore::new()) as Arc<dyn KmStoreTrait>,
        bundle,
    )
}

fn build_app(auth_enabled: bool) -> Router {
    let state = build_test_state(auth_enabled);
    build_router(state, None)
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

fn delete_json_request_auth(uri: &str, body: serde_json::Value, token: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
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
async fn register_and_get_token(app: &Router, email: &str, name: &str, password: &str) -> String {
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
            "password": "Secret123"
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
            "password": "Secret123"
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
            "password": "Correct1pass"
        }),
    );
    app.clone().oneshot(req).await.unwrap();

    // Login with wrong password
    let req = json_request(
        "POST",
        "/api/auth/login",
        serde_json::json!({
            "email": "bob@test.com",
            "password": "Wrong1pass"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Password policy tests ───────────────────────────────────────────

#[tokio::test]
async fn register_rejects_short_password() {
    let app = build_app(true);
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({
            "email": "short@test.com",
            "name": "Short",
            "password": "Ab1"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = body_json(resp.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at least")
    );
}

#[tokio::test]
async fn register_rejects_no_uppercase() {
    let app = build_app(true);
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({
            "email": "noup@test.com",
            "name": "NoUp",
            "password": "lowercase123"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = body_json(resp.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("uppercase")
    );
}

#[tokio::test]
async fn register_rejects_no_digit() {
    let app = build_app(true);
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({
            "email": "nodigit@test.com",
            "name": "NoDigit",
            "password": "NoDigitHere"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = body_json(resp.into_body()).await;
    assert!(body["error"]["message"].as_str().unwrap().contains("digit"));
}

// ── Brute-force protection tests ────────────────────────────────────

#[tokio::test]
async fn login_locks_after_max_attempts() {
    let app = build_app(true);

    // Register a valid user first
    let req = json_request(
        "POST",
        "/api/auth/register",
        serde_json::json!({
            "email": "lockme@test.com",
            "name": "LockMe",
            "password": "Correct1pass"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Make 5 failed login attempts (max_login_attempts = 5 in test config)
    for _ in 0..5 {
        let req = json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({
                "email": "lockme@test.com",
                "password": "Wrong1pass"
            }),
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // 6th attempt should be locked out (even with correct password)
    let req = json_request(
        "POST",
        "/api/auth/login",
        serde_json::json!({
            "email": "lockme@test.com",
            "password": "Correct1pass"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = body_json(resp.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("locked")
    );
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
    let token = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;

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
    let token_a = register_and_get_token(&app, "a@test.com", "A", "Pass1234").await;
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
    let token_b = register_and_get_token(&app, "b@test.com", "B", "Pass1234").await;

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
    let token = register_and_get_token(&app, "doc@test.com", "DocUser", "Pass1234").await;

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
    let req = get_request_auth(&format!("/api/km/workspaces/{ws_id}/documents"), &token);
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
    let req = get_request_auth(&format!("/api/km/workspaces/{ws_id}/documents"), &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);
}

// ── Permission Management Tests ─────────────────────────────────────

/// Helper: create an org as the given user and return the org_id.
async fn create_org(app: &Router, token: &str) -> String {
    let req = json_request_auth(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "TestOrg" }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn grant_and_list_permissions() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    // Register user B
    let _token_b = register_and_get_token(&app, "viewer@test.com", "Viewer", "Pass1234").await;

    // Owner grants Viewer to user B
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "viewer@test.com",
            "role": "viewer",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // List permissions — should show Owner + Viewer = 2
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/permissions"), &token_a);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);
}

#[tokio::test]
async fn grant_upserts_existing() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    let _token_b = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant Viewer
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "user@test.com",
            "role": "viewer",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Grant Editor to same user at same scope (upsert)
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "user@test.com",
            "role": "editor",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // List — should still be 2 (owner + user), not 3
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/permissions"), &token_a);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);

    // The user's role should be editor now
    let data = body["data"].as_array().unwrap();
    let user_perm = data.iter().find(|p| p["email"] == "user@test.com").unwrap();
    assert_eq!(user_perm["role"], "editor");
}

#[tokio::test]
async fn revoke_permission() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    let _token_b = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant then revoke
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "user@test.com",
            "role": "viewer",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    app.clone().oneshot(req).await.unwrap();

    let req = delete_json_request_auth(
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "user@test.com",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // List — only the owner remains
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/permissions"), &token_a);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
}

#[tokio::test]
async fn cannot_remove_last_owner() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    // Try to revoke the sole owner
    let req = delete_json_request_auth(
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "owner@test.com",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cannot_grant_role_above_own() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    // Register admin user, grant Admin
    let token_b = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "admin@test.com",
            "role": "admin",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    app.clone().oneshot(req).await.unwrap();

    // Register target user
    let _token_c = register_and_get_token(&app, "target@test.com", "Target", "Pass1234").await;

    // Admin tries to grant Owner → 403
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "target@test.com",
            "role": "owner",
            "scope": { "level": "Org" }
        }),
        &token_b,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn viewer_cannot_manage_permissions() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    // Grant Viewer to user B
    let token_b = register_and_get_token(&app, "viewer@test.com", "Viewer", "Pass1234").await;
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({
            "email": "viewer@test.com",
            "role": "viewer",
            "scope": { "level": "Org" }
        }),
        &token_a,
    );
    app.clone().oneshot(req).await.unwrap();

    // Viewer tries to list permissions → 403
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/permissions"), &token_b);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ── Pagination Tests ────────────────────────────────────────────────

/// Helper: create an org with a custom name and return the org_id.
async fn create_named_org(app: &Router, token: &str, name: &str) -> String {
    let req = json_request_auth(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": name }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn pagination_limit_offset() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;

    // Create 5 orgs
    for i in 0..5 {
        create_named_org(&app, &token, &format!("Org-{i}")).await;
    }

    // Default: all 5
    let req = get_request_auth("/api/km/orgs", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 5);
    assert_eq!(body["data"].as_array().unwrap().len(), 5);

    // limit=2, offset=1 → data.len()=2, total=5
    let req = get_request_auth("/api/km/orgs?limit=2&offset=1", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 5);
    assert_eq!(body["data"].as_array().unwrap().len(), 2);

    // limit=3, offset=0
    let req = get_request_auth("/api/km/orgs?limit=3", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 5);
    assert_eq!(body["data"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn pagination_beyond_end() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;

    // Create 2 orgs
    create_named_org(&app, &token, "A").await;
    create_named_org(&app, &token, "B").await;

    // offset=10 → data is empty, total=2
    let req = get_request_auth("/api/km/orgs?offset=10", &token);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn pagination_on_documents() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "doc@test.com", "Doc", "Pass1234").await;

    // Setup hierarchy
    let org_id = create_named_org(&app, &token, "DocOrg").await;
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

    // Ingest 3 documents
    for i in 0..3 {
        let req = json_request_auth(
            "POST",
            &format!("/api/km/workspaces/{ws_id}/documents"),
            serde_json::json!({
                "title": format!("Doc {i}"),
                "content": "Hello world test content.",
                "mime_type": "text/plain"
            }),
            &token,
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Paginate: limit=2
    let req = get_request_auth(
        &format!("/api/km/workspaces/{ws_id}/documents?limit=2"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 3);
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn pagination_on_permissions() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let org_id = create_org(&app, &token_a).await;

    // Grant permissions to 3 additional users
    for i in 0..3 {
        let email = format!("user{i}@test.com");
        let _tok = register_and_get_token(&app, &email, &format!("User{i}"), "Pass1234").await;
        let req = json_request_auth(
            "POST",
            &format!("/api/km/orgs/{org_id}/permissions"),
            serde_json::json!({
                "email": email,
                "role": "viewer",
                "scope": { "level": "Org" }
            }),
            &token_a,
        );
        app.clone().oneshot(req).await.unwrap();
    }

    // Total = 4 (owner auto-grant + 3 viewers), limit=2
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/permissions?limit=2"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 4);
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
}

// ── Scoped Permission Tests ─────────────────────────────────────────

/// Helper: create org + dept + workspace, return (org_id, dept_id, ws_id).
async fn create_hierarchy(app: &Router, token: &str) -> (String, String, String) {
    let org_id = create_org(app, token).await;

    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts"),
        serde_json::json!({ "name": "Eng" }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let dept_id = body["id"].as_str().unwrap().to_string();

    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        serde_json::json!({ "name": "Main" }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let ws_id = body["id"].as_str().unwrap().to_string();

    (org_id, dept_id, ws_id)
}

#[tokio::test]
async fn grant_and_list_dept_permissions() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_id, _ws_id) = create_hierarchy(&app, &token_a).await;

    let _token_b = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant dept-scoped permission
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "editor" }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // List dept permissions — should show 1 (dept-scoped only)
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/permissions"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["email"], "user@test.com");
    assert_eq!(body["data"][0]["role"], "editor");
}

#[tokio::test]
async fn grant_and_list_workspace_permissions() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_id, ws_id) = create_hierarchy(&app, &token_a).await;

    let _token_b = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant workspace-scoped permission
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "viewer" }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // List workspace permissions — 1 entry
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["role"], "viewer");
}

#[tokio::test]
async fn dept_permission_isolation() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_id, ws_id) = create_hierarchy(&app, &token_a).await;

    let _token_b = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant at dept level
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "editor" }),
        &token_a,
    );
    app.clone().oneshot(req).await.unwrap();

    // Dept-level list shows 1
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/permissions"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // Workspace-level list shows 0 (isolated)
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);

    // Org-level list shows 2 (owner auto-grant + dept grant)
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/permissions"), &token_a);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);
}

#[tokio::test]
async fn scoped_permission_revoke() {
    let app = build_app(true);
    let token_a = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_id, ws_id) = create_hierarchy(&app, &token_a).await;

    let _token_b = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant at workspace level
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "viewer" }),
        &token_a,
    );
    app.clone().oneshot(req).await.unwrap();

    // Verify it exists
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);

    // Revoke it
    let req = delete_json_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        serde_json::json!({ "email": "user@test.com" }),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify gone
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions"),
        &token_a,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);
}

// ── MIME Validation Tests ───────────────────────────────────────────

#[tokio::test]
async fn ingest_unsupported_mime_type_returns_400() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "doc@test.com", "Doc", "Pass1234").await;

    // Setup hierarchy
    let (org_id, dept_id, _) = create_hierarchy(&app, &token).await;
    // Need a workspace id for the documents endpoint
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let ws_id = body["data"][0]["id"].as_str().unwrap().to_string();

    // Ingest with unsupported MIME type
    let req = json_request_auth(
        "POST",
        &format!("/api/km/workspaces/{ws_id}/documents"),
        serde_json::json!({
            "title": "Bad Doc",
            "content": "some content",
            "mime_type": "application/x-custom-unsupported"
        }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unsupported MIME type")
    );
}

#[tokio::test]
async fn ingest_response_includes_enriched_fields() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "doc@test.com", "Doc", "Pass1234").await;

    let (org_id, dept_id, _) = create_hierarchy(&app, &token).await;
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let ws_id = body["data"][0]["id"].as_str().unwrap().to_string();

    let content = "Hello world test document content.";
    let req = json_request_auth(
        "POST",
        &format!("/api/km/workspaces/{ws_id}/documents"),
        serde_json::json!({
            "title": "Test Doc",
            "content": content,
            "mime_type": "text/plain"
        }),
        &token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;

    // Enriched fields present
    assert_eq!(body["mime_type"], "text/plain");
    assert_eq!(body["size_bytes"], content.len() as i64);
    assert!(body["doc_id"].as_str().is_some());
    assert!(body["chunks"].as_u64().unwrap() >= 1);
    // filename is null for JSON ingest
    assert!(body["filename"].is_null());
}

// ── Rate Limiting Tests ─────────────────────────────────────────────

fn build_rate_limited_app(burst: u64) -> Router {
    let state = build_test_state(false);
    let limiter = thairag_api::rate_limit::RateLimiter::new(1, burst);
    build_router(state, Some(limiter))
}

#[tokio::test]
async fn rate_limit_returns_429() {
    let app = build_rate_limited_app(2);

    // First 2 requests should succeed (burst=2)
    for _ in 0..2 {
        let req = Request::builder()
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Third request should be rate-limited
    let req = Request::builder()
        .uri("/v1/models")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().get("retry-after").is_some());
}

#[tokio::test]
async fn health_not_rate_limited() {
    let app = build_rate_limited_app(1);

    // Exhaust the rate limit on a normal endpoint
    let req = Request::builder()
        .uri("/v1/models")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Next normal request is rate-limited
    let req = Request::builder()
        .uri("/v1/models")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // But /health always succeeds
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Streaming Mock & Tests ──────────────────────────────────────────

/// An LLM mock that streams multiple tokens and populates non-zero usage.
struct MockStreamingLlm;

#[async_trait]
impl LlmProvider for MockStreamingLlm {
    async fn generate(
        &self,
        _messages: &[ChatMessage],
        _max_tokens: Option<u32>,
    ) -> thairag_core::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Hello world".into(),
            usage: LlmUsage {
                prompt_tokens: 10,
                completion_tokens: 3,
            },
        })
    }

    async fn generate_stream(
        &self,
        _messages: &[ChatMessage],
        _max_tokens: Option<u32>,
    ) -> thairag_core::Result<LlmStreamResponse> {
        let usage = Arc::new(Mutex::new(None));
        let usage_clone = Arc::clone(&usage);

        let tokens = vec!["Hello", " ", "world"];
        let stream = tokio_stream::iter(tokens.into_iter().map(|t| Ok(t.to_string())));

        // Populate usage after creating the stream — the handler reads it once the stream ends.
        *usage_clone.lock().unwrap() = Some(LlmUsage {
            prompt_tokens: 10,
            completion_tokens: 3,
        });

        Ok(LlmStreamResponse {
            stream: Box::pin(stream),
            usage,
        })
    }

    fn model_name(&self) -> &str {
        "mock-streaming-llm"
    }
}

fn build_streaming_test_app() -> Router {
    let llm: Arc<dyn LlmProvider> = Arc::new(MockStreamingLlm);
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
        Arc::clone(&embedding),
        vector_store,
        text_search,
        reranker,
        search_config,
    ));

    let rag_engine = Arc::new(RagEngine::new(Arc::clone(&llm), Arc::clone(&search_engine)));
    let orchestrator = Arc::new(QueryOrchestrator::new(Arc::clone(&llm), rag_engine));
    let document_pipeline = Arc::new(DocumentPipeline::new(512, 50));

    let config = AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
            shutdown_timeout_secs: 5,
            rate_limit: RateLimitConfig {
                enabled: false,
                requests_per_second: 10,
                burst_size: 20,
            },
            cors_origins: vec![],
            trust_proxy: false,
            max_chat_messages: 50,
            max_message_length: 32000,
            request_timeout_secs: 600,
        },
        database: DatabaseConfig {
            url: "".into(),
            max_connections: 1,
        },
        auth: AuthConfig {
            enabled: false,
            jwt_secret: "test-secret".into(),
            token_expiry_hours: 24,
            password_min_length: 8,
            max_login_attempts: 5,
            lockout_duration_secs: 300,
            api_keys: String::new(),
        },
        providers: ProvidersConfig {
            llm: LlmConfig {
                kind: thairag_core::types::LlmKind::Ollama,
                model: "mock".into(),
                base_url: "".into(),
                api_key: "".into(),
                max_tokens: None,
                profile_id: None,
            },
            embedding: EmbeddingConfig {
                kind: thairag_core::types::EmbeddingKind::Fastembed,
                model: "mock".into(),
                dimension: 4,
                base_url: "".into(),
                api_key: "".into(),
            },
            vector_store: VectorStoreConfig {
                kind: thairag_core::types::VectorStoreKind::InMemory,
                url: "".into(),
                collection: "".into(),
                api_key: "".into(),
                isolation: Default::default(),
            },
            text_search: TextSearchConfig {
                kind: thairag_core::types::TextSearchKind::Tantivy,
                index_path: "/tmp/test-tantivy-stream".into(),
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
            max_upload_size_mb: 50,
            ai_preprocessing: Default::default(),
        },
        chat_pipeline: Default::default(),
        mcp: Default::default(),
        session: Default::default(),
        embedding_cache: Default::default(),
        job_queue: Default::default(),
        redis: Default::default(),
    };

    let bundle = ProviderBundle {
        providers_config: config.providers.clone(),
        chat_pipeline_config: config.chat_pipeline.clone(),
        orchestrator,
        chat_pipeline: None,
        document_pipeline,
        search_engine,
        embedding,
        context_compactor: None,
        personal_memory_manager: None,
    };

    let state = AppState::from_parts(
        Arc::new(config),
        None,
        Arc::new(MemoryKmStore::new()) as Arc<dyn KmStoreTrait>,
        bundle,
    );

    build_router(state, None)
}

/// Parse SSE body text into a vec of JSON values (skipping [DONE]).
fn parse_sse_chunks(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

#[tokio::test]
async fn streaming_chat_returns_sse_with_usage() {
    let app = build_streaming_test_app();

    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true
        }),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_text = String::from_utf8(body_bytes.to_vec()).unwrap();

    // Verify [DONE] is present
    assert!(
        body_text.contains("data: [DONE]"),
        "SSE stream must end with [DONE]"
    );

    let chunks = parse_sse_chunks(&body_text);
    assert!(
        chunks.len() >= 3,
        "Expected at least 3 chunks (role + content + finish + usage), got {}",
        chunks.len()
    );

    // 1. First chunk: role = "assistant", usage is absent/null
    let first = &chunks[0];
    assert_eq!(first["choices"][0]["delta"]["role"], "assistant");
    assert!(
        first.get("usage").is_none() || first["usage"].is_null(),
        "First chunk should not have usage"
    );

    // 2. Content chunks: have delta.content, usage absent/null
    let content_chunks: Vec<&serde_json::Value> = chunks
        .iter()
        .filter(|c| {
            c["choices"]
                .get(0)
                .and_then(|ch| ch["delta"]["content"].as_str())
                .is_some()
        })
        .collect();
    assert!(
        !content_chunks.is_empty(),
        "Should have at least one content chunk"
    );
    for cc in &content_chunks {
        assert!(
            cc.get("usage").is_none() || cc["usage"].is_null(),
            "Content chunks should not have usage"
        );
    }

    // 3. Finish chunk: finish_reason = "stop", usage absent/null
    let finish_chunks: Vec<&serde_json::Value> = chunks
        .iter()
        .filter(|c| {
            c["choices"]
                .get(0)
                .and_then(|ch| ch["finish_reason"].as_str())
                == Some("stop")
        })
        .collect();
    assert_eq!(
        finish_chunks.len(),
        1,
        "Should have exactly one finish chunk"
    );
    let finish = finish_chunks[0];
    assert!(
        finish.get("usage").is_none() || finish["usage"].is_null(),
        "Finish chunk should not have usage"
    );

    // 4. Usage chunk: choices is empty, usage has correct token counts
    let usage_chunks: Vec<&serde_json::Value> = chunks
        .iter()
        .filter(|c| {
            c["choices"].as_array().is_some_and(|arr| arr.is_empty()) && !c["usage"].is_null()
        })
        .collect();
    assert_eq!(usage_chunks.len(), 1, "Should have exactly one usage chunk");
    let usage = &usage_chunks[0]["usage"];
    assert_eq!(usage["prompt_tokens"], 10);
    assert_eq!(usage["completion_tokens"], 3);
    assert_eq!(usage["total_tokens"], 13);
}

#[tokio::test]
async fn streaming_chat_content_matches_tokens() {
    let app = build_streaming_test_app();

    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true
        }),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_text = String::from_utf8(body_bytes.to_vec()).unwrap();

    let chunks = parse_sse_chunks(&body_text);

    let full_content: String = chunks
        .iter()
        .filter_map(|c| {
            c["choices"]
                .get(0)
                .and_then(|ch| ch["delta"]["content"].as_str())
        })
        .collect();

    assert_eq!(
        full_content, "Hello world",
        "Concatenated stream content should match expected output"
    );
}

// ── Session Management Tests ────────────────────────────────────────

#[tokio::test]
async fn session_none_is_stateless() {
    // Without session_id, no state is stored
    let app = build_app(false);

    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": [{"role": "user", "content": "Hello"}]
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    // No session_id in response when not provided
    assert!(body.get("session_id").is_none());
}

#[tokio::test]
async fn session_accumulates_history() {
    let app = build_app(false);
    let session_id = uuid::Uuid::new_v4().to_string();

    // First request with session_id
    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": [{"role": "user", "content": "Hello"}],
            "session_id": session_id
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["session_id"].as_str().unwrap(), session_id);

    // Second request with same session_id
    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": [{"role": "user", "content": "Follow up"}],
            "session_id": session_id
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["session_id"].as_str().unwrap(), session_id);
}

#[tokio::test]
async fn invalid_session_id_returns_400() {
    let app = build_app(false);

    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": [{"role": "user", "content": "Hello"}],
            "session_id": "not-a-uuid"
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Request Validation Tests ────────────────────────────────────────

#[tokio::test]
async fn empty_messages_returns_400() {
    let app = build_app(false);

    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "ThaiRAG-1.0",
            "messages": []
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("messages must not be empty")
    );
}

#[tokio::test]
async fn wrong_model_returns_400() {
    let app = build_app(false);

    let req = json_request(
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        }),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("model not found")
    );
}

// ── Deep Health Check Tests ─────────────────────────────────────────

#[tokio::test]
async fn health_shallow_still_works() {
    let app = build_app(false);
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["status"], "ok");
    // Shallow check should not have "checks" field
    assert!(body.get("checks").is_none());
}

#[tokio::test]
async fn health_deep_returns_checks() {
    let app = build_app(false);
    let req = Request::builder()
        .uri("/health?deep=true")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["status"], "ok");
    assert!(body["checks"]["embedding"].as_str().is_some());
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_format() {
    let app = build_app(false);

    // Make a request first so the MetricsLayer records something
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Now check /metrics
    let req = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();

    assert!(
        body.contains("http_requests_total"),
        "Should contain http_requests_total metric"
    );
    assert!(
        body.contains("http_request_duration_seconds"),
        "Should contain http_request_duration_seconds metric"
    );
    assert!(
        body.contains("active_sessions_total"),
        "Should contain active_sessions_total metric"
    );
}

// ── Scoped Permission Isolation Tests ───────────────────────────────

/// Helper: create a second hierarchy (org + dept + workspace) under a different org.
async fn create_second_hierarchy(app: &Router, token: &str) -> (String, String, String) {
    let req = json_request_auth(
        "POST",
        "/api/km/orgs",
        serde_json::json!({ "name": "OrgB" }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp.into_body()).await;
    let org_id = body["id"].as_str().unwrap().to_string();

    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts"),
        serde_json::json!({ "name": "Sales" }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let dept_id = body["id"].as_str().unwrap().to_string();

    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        serde_json::json!({ "name": "WsB" }),
        token,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let ws_id = body["id"].as_str().unwrap().to_string();

    (org_id, dept_id, ws_id)
}

#[tokio::test]
async fn list_orgs_filtered_by_permission() {
    let app = build_app(true);
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;

    // Owner creates two orgs (gets Owner perm on both)
    let (org_a, _, _) = create_hierarchy(&app, &token_owner).await;
    let (_org_b, _, _) = create_second_hierarchy(&app, &token_owner).await;

    // Owner sees both orgs
    let req = get_request_auth("/api/km/orgs", &token_owner);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);

    // Register user with NO permissions
    let token_user = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // User sees 0 orgs
    let req = get_request_auth("/api/km/orgs", &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);

    // Grant user Viewer on org_a
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_a}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "viewer", "scope": { "level": "Org" } }),
        &token_owner,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // User now sees only org_a
    let req = get_request_auth("/api/km/orgs", &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], org_a);
}

#[tokio::test]
async fn list_depts_filtered_by_scope() {
    let app = build_app(true);
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_a, ws_a) = create_hierarchy(&app, &token_owner).await;

    // Create second dept in same org
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts"),
        serde_json::json!({ "name": "HR" }),
        &token_owner,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let _dept_b = body["id"].as_str().unwrap().to_string();

    let token_user = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant user workspace-level perm in dept_a's workspace
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_a}/workspaces/{ws_a}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "editor" }),
        &token_owner,
    );
    app.clone().oneshot(req).await.unwrap();

    // User lists depts: should see only dept_a (via workspace-level perm), NOT dept_b
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/depts"), &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], dept_a);

    // Grant user org-level Viewer — should now see both depts
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "viewer", "scope": { "level": "Org" } }),
        &token_owner,
    );
    app.clone().oneshot(req).await.unwrap();

    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/depts"), &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);
}

#[tokio::test]
async fn list_workspaces_filtered_by_scope() {
    let app = build_app(true);
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_id, ws_a) = create_hierarchy(&app, &token_owner).await;

    // Create second workspace in same dept
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        serde_json::json!({ "name": "WsB" }),
        &token_owner,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let _ws_b = body["id"].as_str().unwrap().to_string();

    let token_user = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant user workspace-level perm on ws_a only
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_a}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "viewer" }),
        &token_owner,
    );
    app.clone().oneshot(req).await.unwrap();

    // User lists workspaces: should see only ws_a
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        &token_user,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["id"], ws_a);

    // Grant dept-level perm → should now see both
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "viewer" }),
        &token_owner,
    );
    app.clone().oneshot(req).await.unwrap();

    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        &token_user,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 2);
}

#[tokio::test]
async fn workspace_user_cannot_access_other_workspace() {
    let app = build_app(true);
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_id, ws_a) = create_hierarchy(&app, &token_owner).await;

    // Create ws_b
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
        serde_json::json!({ "name": "WsB" }),
        &token_owner,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let ws_b = body["id"].as_str().unwrap().to_string();

    let token_user = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant user editor on ws_a only
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_a}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "editor" }),
        &token_owner,
    );
    app.clone().oneshot(req).await.unwrap();

    // User can access ws_a
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_a}"),
        &token_user,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // User CANNOT access ws_b
    let req = get_request_auth(
        &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_b}"),
        &token_user,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn dept_user_cannot_create_in_other_dept() {
    let app = build_app(true);
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, dept_a, _) = create_hierarchy(&app, &token_owner).await;

    // Create dept_b
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts"),
        serde_json::json!({ "name": "HR" }),
        &token_owner,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    let dept_b = body["id"].as_str().unwrap().to_string();

    let token_user = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Grant user editor on dept_a
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_a}/permissions"),
        serde_json::json!({ "email": "user@test.com", "role": "editor" }),
        &token_owner,
    );
    app.clone().oneshot(req).await.unwrap();

    // User can create workspace in dept_a
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_a}/workspaces"),
        serde_json::json!({ "name": "New WS" }),
        &token_user,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // User CANNOT create workspace in dept_b
    let req = json_request_auth(
        "POST",
        &format!("/api/km/orgs/{org_id}/depts/{dept_b}/workspaces"),
        serde_json::json!({ "name": "Illegal WS" }),
        &token_user,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_permission_user_sees_empty_lists() {
    let app = build_app(true);
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (org_id, _dept_id, _ws_id) = create_hierarchy(&app, &token_owner).await;

    let token_user = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // User sees 0 orgs
    let req = get_request_auth("/api/km/orgs", &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 0);

    // User cannot get org
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}"), &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // User cannot list depts (no perm in org)
    let req = get_request_auth(&format!("/api/km/orgs/{org_id}/depts"), &token_user);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn super_admin_sees_everything() {
    let state = build_test_state(true);
    // Seed a super admin directly
    state
        .km_store
        .upsert_user_by_email(
            "admin@test.com".into(),
            "Admin".into(),
            "$argon2id$v=19$m=19456,t=2,p=1$dummy$dummyhash".into(),
            true,
            "super_admin".into(),
        )
        .unwrap();

    let app = build_router(state.clone(), None);

    // Create a regular user who creates an org
    let token_owner = register_and_get_token(&app, "owner@test.com", "Owner", "Pass1234").await;
    let (_org_id, _dept_id, _ws_id) = create_hierarchy(&app, &token_owner).await;

    // Generate JWT for super admin
    let admin_jwt = state.jwt.as_ref().unwrap();
    let admin_user = state.km_store.get_user_by_email("admin@test.com").unwrap();
    let admin_token = admin_jwt
        .encode(&admin_user.user.id.0.to_string(), &admin_user.user.email)
        .unwrap();

    // Super admin sees all orgs
    let req = get_request_auth("/api/km/orgs", &admin_token);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["total"], 1);
}

// ── Connector CRUD Tests ────────────────────────────────────────────

#[tokio::test]
async fn connector_crud() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;

    // First create an org, dept, workspace to get a valid workspace_id
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/orgs",
            serde_json::json!({"name": "TestOrg"}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let org = body_json(resp.into_body()).await;
    let org_id = org["id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/orgs/{org_id}/depts"),
            serde_json::json!({"name": "TestDept"}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let dept = body_json(resp.into_body()).await;
    let dept_id = dept["id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
            serde_json::json!({"name": "TestWS"}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ws = body_json(resp.into_body()).await;
    let ws_id = ws["id"].as_str().unwrap();

    // Create connector
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/connectors",
            serde_json::json!({
                "name": "Test MCP",
                "transport": "stdio",
                "command": "/usr/bin/echo",
                "args": ["hello"],
                "workspace_id": ws_id,
            }),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let connector = body_json(resp.into_body()).await;
    let connector_id = connector["id"].as_str().unwrap();
    assert_eq!(connector["name"], "Test MCP");
    assert_eq!(connector["transport"], "stdio");
    assert_eq!(connector["status"], "active");

    // List connectors
    let resp = app
        .clone()
        .oneshot(get_request_auth("/api/km/connectors", &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp.into_body()).await;
    assert_eq!(list["total"], 1);

    // Get connector
    let resp = app
        .clone()
        .oneshot(get_request_auth(
            &format!("/api/km/connectors/{connector_id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let fetched = body_json(resp.into_body()).await;
    assert_eq!(fetched["name"], "Test MCP");

    // Update connector
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "PUT",
            &format!("/api/km/connectors/{connector_id}"),
            serde_json::json!({"name": "Updated MCP"}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp.into_body()).await;
    assert_eq!(updated["name"], "Updated MCP");

    // Pause connector
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/connectors/{connector_id}/pause"),
            serde_json::json!({}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify paused
    let resp = app
        .clone()
        .oneshot(get_request_auth(
            &format!("/api/km/connectors/{connector_id}"),
            &token,
        ))
        .await
        .unwrap();
    let paused = body_json(resp.into_body()).await;
    assert_eq!(paused["status"], "paused");

    // Resume connector
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/connectors/{connector_id}/resume"),
            serde_json::json!({}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // List sync runs (empty)
    let resp = app
        .clone()
        .oneshot(get_request_auth(
            &format!("/api/km/connectors/{connector_id}/sync-runs"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let runs = body_json(resp.into_body()).await;
    assert_eq!(runs["total"], 0);

    // Trigger sync (should fail because MCP is disabled)
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/connectors/{connector_id}/sync"),
            serde_json::json!({}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Test connection (should fail because MCP is disabled)
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/connectors/{connector_id}/test"),
            serde_json::json!({}),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Delete connector
    let resp = app
        .clone()
        .oneshot(delete_request_auth(
            &format!("/api/km/connectors/{connector_id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify deleted
    let resp = app
        .clone()
        .oneshot(get_request_auth("/api/km/connectors", &token))
        .await
        .unwrap();
    let list = body_json(resp.into_body()).await;
    assert_eq!(list["total"], 0);
}

#[tokio::test]
async fn connector_validation() {
    let app = build_app(true);
    let token = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;

    // Create org/dept/workspace
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/orgs",
            serde_json::json!({"name": "Org"}),
            &token,
        ))
        .await
        .unwrap();
    let org = body_json(resp.into_body()).await;
    let org_id = org["id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/orgs/{org_id}/depts"),
            serde_json::json!({"name": "Dept"}),
            &token,
        ))
        .await
        .unwrap();
    let dept = body_json(resp.into_body()).await;
    let dept_id = dept["id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            &format!("/api/km/orgs/{org_id}/depts/{dept_id}/workspaces"),
            serde_json::json!({"name": "WS"}),
            &token,
        ))
        .await
        .unwrap();
    let ws = body_json(resp.into_body()).await;
    let ws_id = ws["id"].as_str().unwrap();

    // Empty name
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/connectors",
            serde_json::json!({
                "name": "",
                "transport": "stdio",
                "command": "echo",
                "workspace_id": ws_id,
            }),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Invalid transport
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/connectors",
            serde_json::json!({
                "name": "Bad",
                "transport": "invalid",
                "workspace_id": ws_id,
            }),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // stdio without command
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/connectors",
            serde_json::json!({
                "name": "Bad",
                "transport": "stdio",
                "workspace_id": ws_id,
            }),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // sse without url
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/connectors",
            serde_json::json!({
                "name": "Bad",
                "transport": "sse",
                "workspace_id": ws_id,
            }),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // scheduled without cron
    let resp = app
        .clone()
        .oneshot(json_request_auth(
            "POST",
            "/api/km/connectors",
            serde_json::json!({
                "name": "Bad",
                "transport": "stdio",
                "command": "echo",
                "workspace_id": ws_id,
                "sync_mode": "scheduled",
            }),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn connector_requires_auth() {
    let app = build_app(true);

    // No auth token → should fail
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/km/connectors")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn connector_non_admin_rejected() {
    let app = build_app(true);
    // First user = super admin
    let _admin = register_and_get_token(&app, "admin@test.com", "Admin", "Pass1234").await;
    // Second user = regular user
    let user_token = register_and_get_token(&app, "user@test.com", "User", "Pass1234").await;

    // Regular user should be rejected
    let resp = app
        .clone()
        .oneshot(get_request_auth("/api/km/connectors", &user_token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
