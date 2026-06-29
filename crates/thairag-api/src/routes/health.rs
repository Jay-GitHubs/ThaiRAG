use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app_state::AppState;

#[derive(Deserialize, Default)]
pub struct HealthQuery {
    #[serde(default)]
    pub deep: Option<bool>,
}

pub async fn health(State(state): State<AppState>, Query(query): Query<HealthQuery>) -> Response {
    if query.deep.unwrap_or(false) {
        deep_health(state).await
    } else {
        Json(json!({
            "status": "ok",
            "service": "thairag",
            "version": env!("CARGO_PKG_VERSION"),
        }))
        .into_response()
    }
}

/// HTTP reachability: any response (even 4xx/5xx) means the host is up; a
/// connection refusal or timeout means it's unreachable. Used for endpoints
/// (LLM gateway, OCR sidecar, cloud rerankers) without making a billable call.
async fn reachable_http(url: &str, ms: u64) -> bool {
    if url.is_empty() {
        return false;
    }
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(ms))
        .build()
    {
        Ok(client) => client.get(url).send().await.is_ok(),
        Err(_) => false,
    }
}

/// TCP reachability for non-HTTP services (qdrant gRPC, redis) — connect to the
/// URL's host:port within the timeout.
async fn reachable_tcp(url: &str, ms: u64) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_string) else {
        return false;
    };
    let Some(port) = parsed.port_or_known_default() else {
        return false;
    };
    let dur = std::time::Duration::from_millis(ms);
    tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .map(|addr| std::net::TcpStream::connect_timeout(&addr, dur).is_ok())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// One readiness check: `ok` (probed, reachable), `fail` (configured but
/// unreachable — a real alert), or `not_configured` (off / not in use — neutral).
fn check(status: &str, detail: impl Into<String>) -> Value {
    json!({ "status": status, "detail": detail.into() })
}

async fn deep_health(state: AppState) -> Response {
    use thairag_core::types::{RerankerKind, VectorStoreKind};
    const T: u64 = 2500; // per-probe timeout (ms); probes run concurrently

    let cfg = state.config.clone();
    let p = &cfg.providers;

    // ── Database (SELECT 1) ──
    let database = match state.km_store.health_check() {
        Ok(()) => check("ok", "reachable"),
        Err(e) => check("fail", e.to_string()),
    };

    // ── Embedding (real embed — cheap, and the common failure) ──
    let embedding = async {
        if state
            .providers()
            .embedding
            .embed(&["health check".to_string()])
            .await
            .is_ok()
        {
            check("ok", format!("{:?}", p.embedding.kind).to_lowercase())
        } else {
            check("fail", "embed probe failed")
        }
    };

    // ── Vector store (InMemory = nothing to probe; all others are external) ──
    let vector_store = async {
        if matches!(p.vector_store.kind, VectorStoreKind::InMemory) {
            check("not_configured", "in-memory (no external store)")
        } else {
            let kind = format!("{:?}", p.vector_store.kind).to_lowercase();
            if reachable_tcp(&p.vector_store.url, T).await {
                check("ok", format!("{kind} @ {}", p.vector_store.url))
            } else {
                check(
                    "fail",
                    format!("{kind} unreachable @ {}", p.vector_store.url),
                )
            }
        }
    };

    // ── Reranker ──
    let reranker = async {
        match p.reranker.kind {
            RerankerKind::Passthrough => check("not_configured", "disabled (passthrough)"),
            RerankerKind::Cohere => {
                if reachable_http("https://api.cohere.com", T).await {
                    check("ok", "cohere")
                } else {
                    check("fail", "api.cohere.com unreachable")
                }
            }
            RerankerKind::Jina => {
                let host = if p.reranker.base_url.is_empty() {
                    "https://api.jina.ai".to_string()
                } else {
                    p.reranker.base_url.clone()
                };
                if reachable_http(&host, T).await {
                    check("ok", host)
                } else {
                    check("fail", format!("unreachable @ {host}"))
                }
            }
        }
    };

    // ── LLM (cheap reachability — no token-costing generate) ──
    let llm = async {
        if p.llm.base_url.is_empty() {
            check("ok", format!("{:?} cloud (not probed)", p.llm.kind))
        } else if reachable_http(&p.llm.base_url, T).await {
            check("ok", p.llm.base_url.clone())
        } else {
            check("fail", format!("unreachable @ {}", p.llm.base_url))
        }
    };

    // ── Deterministic OCR sidecar (the opt-in `ocr` profile) ──
    let ocr_sidecar = async {
        let url = cfg.document.ocr_sidecar_url.trim_end_matches('/');
        if url.is_empty() {
            check(
                "not_configured",
                "OCR sidecar off (enable the `ocr` compose profile + set OCR_SIDECAR_URL)",
            )
        } else if reachable_http(&format!("{url}/health"), T).await {
            check("ok", url.to_string())
        } else {
            check("fail", format!("unreachable @ {url}"))
        }
    };

    // ── Redis (only when a redis backend is actually in use) ──
    let redis = async {
        let in_use = cfg.session.backend == "redis" || cfg.embedding_cache.backend == "redis";
        if !in_use {
            check("not_configured", "not in use (in-memory backends)")
        } else if reachable_tcp(&cfg.redis.url, T).await {
            check("ok", cfg.redis.url.clone())
        } else {
            check("fail", format!("unreachable @ {}", cfg.redis.url))
        }
    };

    // Run the network probes concurrently so the endpoint stays fast.
    let (embedding, vector_store, reranker, llm, ocr_sidecar, redis) =
        tokio::join!(embedding, vector_store, reranker, llm, ocr_sidecar, redis);

    let checks: serde_json::Map<String, Value> = [
        ("database", database),
        ("embedding", embedding),
        ("vector_store", vector_store),
        ("reranker", reranker),
        ("llm", llm),
        ("ocr_sidecar", ocr_sidecar),
        ("redis", redis),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    // A configured-but-failing service degrades the system; `not_configured` does not.
    let degraded = checks
        .values()
        .any(|c| c.get("status").and_then(Value::as_str) == Some("fail"));
    let status_code = if degraded {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    let body = json!({
        "status": if degraded { "degraded" } else { "ok" },
        "service": "thairag",
        "version": env!("CARGO_PKG_VERSION"),
        "checks": checks,
    });

    (status_code, Json(body)).into_response()
}
