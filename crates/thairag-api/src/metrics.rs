use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response};
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry, TextEncoder,
};
use tower::{Layer, Service};

// ── MetricsState ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MetricsState {
    registry: Registry,
    pub http_requests_total: IntCounterVec,
    pub http_request_duration_seconds: HistogramVec,
    pub llm_tokens_total: IntCounterVec,
    pub active_sessions_total: IntGauge,
    pub mcp_sync_runs_total: IntCounterVec,
    pub mcp_sync_items_total: IntCounterVec,
    pub mcp_sync_duration_seconds: HistogramVec,
    /// Number of streaming-output redactions, keyed by violation code and
    /// guard stage. Cardinality bounded by the closed `ViolationCode` enum
    /// (~11 codes) × stages (2) → safe Prometheus label space.
    pub guardrail_streaming_redactions_total: IntCounterVec,
}

impl Default for MetricsState {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsState {
    pub fn new() -> Self {
        let registry = Registry::new();

        let http_requests_total = IntCounterVec::new(
            Opts::new("http_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )
        .unwrap();

        let http_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &["method", "path"],
        )
        .unwrap();

        let llm_tokens_total = IntCounterVec::new(
            Opts::new("llm_tokens_total", "Total LLM tokens consumed"),
            &["type"],
        )
        .unwrap();

        let active_sessions_total =
            IntGauge::new("active_sessions_total", "Number of active sessions").unwrap();

        let mcp_sync_runs_total = IntCounterVec::new(
            Opts::new("mcp_sync_runs_total", "Total MCP sync runs"),
            &["connector", "status"],
        )
        .unwrap();

        let mcp_sync_items_total = IntCounterVec::new(
            Opts::new("mcp_sync_items_total", "Total MCP sync items processed"),
            &["connector", "action"],
        )
        .unwrap();

        let mcp_sync_duration_seconds = HistogramVec::new(
            HistogramOpts::new("mcp_sync_duration_seconds", "MCP sync duration in seconds")
                .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0]),
            &["connector"],
        )
        .unwrap();

        let guardrail_streaming_redactions_total = IntCounterVec::new(
            Opts::new(
                "guardrail_streaming_redactions_total",
                "Number of streaming-output redactions fired by the deterministic guardrail",
            ),
            &["code", "stage"],
        )
        .unwrap();

        registry
            .register(Box::new(http_requests_total.clone()))
            .unwrap();
        registry
            .register(Box::new(http_request_duration_seconds.clone()))
            .unwrap();
        registry
            .register(Box::new(llm_tokens_total.clone()))
            .unwrap();
        registry
            .register(Box::new(active_sessions_total.clone()))
            .unwrap();
        registry
            .register(Box::new(mcp_sync_runs_total.clone()))
            .unwrap();
        registry
            .register(Box::new(mcp_sync_items_total.clone()))
            .unwrap();
        registry
            .register(Box::new(mcp_sync_duration_seconds.clone()))
            .unwrap();
        registry
            .register(Box::new(guardrail_streaming_redactions_total.clone()))
            .unwrap();

        Self {
            registry,
            http_requests_total,
            http_request_duration_seconds,
            llm_tokens_total,
            active_sessions_total,
            mcp_sync_runs_total,
            mcp_sync_items_total,
            mcp_sync_duration_seconds,
            guardrail_streaming_redactions_total,
        }
    }

    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    pub fn record_tokens(&self, prompt: u32, completion: u32) {
        self.llm_tokens_total
            .with_label_values(&["prompt"])
            .inc_by(prompt as u64);
        self.llm_tokens_total
            .with_label_values(&["completion"])
            .inc_by(completion as u64);
    }

    pub fn set_active_sessions(&self, count: usize) {
        self.active_sessions_total.set(count as i64);
    }

    /// Increment the streaming-redaction counter for a single fired code.
    pub fn record_streaming_redaction(&self, code: &str, stage: &str) {
        self.guardrail_streaming_redactions_total
            .with_label_values(&[code, stage])
            .inc();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_sync_run(
        &self,
        connector_name: &str,
        status: &str,
        duration_secs: f64,
        created: u64,
        updated: u64,
        skipped: u64,
        failed: u64,
    ) {
        self.mcp_sync_runs_total
            .with_label_values(&[connector_name, status])
            .inc();
        self.mcp_sync_duration_seconds
            .with_label_values(&[connector_name])
            .observe(duration_secs);
        if created > 0 {
            self.mcp_sync_items_total
                .with_label_values(&[connector_name, "created"])
                .inc_by(created);
        }
        if updated > 0 {
            self.mcp_sync_items_total
                .with_label_values(&[connector_name, "updated"])
                .inc_by(updated);
        }
        if skipped > 0 {
            self.mcp_sync_items_total
                .with_label_values(&[connector_name, "skipped"])
                .inc_by(skipped);
        }
        if failed > 0 {
            self.mcp_sync_items_total
                .with_label_values(&[connector_name, "failed"])
                .inc_by(failed);
        }
    }
}

impl thairag_core::traits::GuardrailMetricsRecorder for MetricsState {
    fn record_streaming_redaction(&self, code: &str, stage: &str) {
        MetricsState::record_streaming_redaction(self, code, stage);
    }
}

// ── Path normalization ──────────────────────────────────────────────

pub fn normalize_path(path: &str) -> &'static str {
    if path == "/health" {
        "/health"
    } else if path == "/v1/models" {
        "/v1/models"
    } else if path == "/v1/chat/completions" {
        "/v1/chat/completions"
    } else if path == "/metrics" {
        "/metrics"
    } else if path.starts_with("/api/auth/") {
        "/api/auth/*"
    } else if path.starts_with("/api/km/") {
        "/api/km/*"
    } else {
        "other"
    }
}

// ── Layer ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MetricsLayer {
    metrics: MetricsState,
}

impl MetricsLayer {
    pub fn new(metrics: MetricsState) -> Self {
        Self { metrics }
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            metrics: self.metrics.clone(),
        }
    }
}

// ── Service ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MetricsService<S> {
    inner: S,
    metrics: MetricsState,
}

impl<S> Service<Request<Body>> for MetricsService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let method = req.method().to_string();
        let path = normalize_path(req.uri().path());
        let metrics = self.metrics.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let start = std::time::Instant::now();
            let response = inner.call(req).await?;
            let duration = start.elapsed().as_secs_f64();
            let status = response.status().as_u16().to_string();

            metrics
                .http_requests_total
                .with_label_values(&[&method, path, &status])
                .inc();
            metrics
                .http_request_duration_seconds
                .with_label_values(&[&method, path])
                .observe(duration);

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_known_routes() {
        assert_eq!(normalize_path("/health"), "/health");
        assert_eq!(normalize_path("/v1/models"), "/v1/models");
        assert_eq!(
            normalize_path("/v1/chat/completions"),
            "/v1/chat/completions"
        );
        assert_eq!(normalize_path("/metrics"), "/metrics");
        assert_eq!(normalize_path("/api/auth/login"), "/api/auth/*");
        assert_eq!(normalize_path("/api/auth/register"), "/api/auth/*");
        assert_eq!(normalize_path("/api/km/orgs"), "/api/km/*");
        assert_eq!(normalize_path("/api/km/orgs/some-uuid/depts"), "/api/km/*");
        assert_eq!(normalize_path("/unknown/path"), "other");
    }

    #[test]
    fn encode_includes_registered_metrics() {
        let state = MetricsState::new();
        // Touch each metric so Prometheus has something to encode
        state
            .http_requests_total
            .with_label_values(&["GET", "/health", "200"])
            .inc();
        state
            .http_request_duration_seconds
            .with_label_values(&["GET", "/health"])
            .observe(0.01);
        state.record_tokens(1, 1);
        state.set_active_sessions(0);

        let output = state.encode();
        assert!(output.contains("http_requests_total"));
        assert!(output.contains("http_request_duration_seconds"));
        assert!(output.contains("llm_tokens_total"));
        assert!(output.contains("active_sessions_total"));
    }

    #[test]
    fn record_tokens_increments() {
        let state = MetricsState::new();
        state.record_tokens(100, 50);
        state.record_tokens(200, 75);

        let output = state.encode();
        assert!(output.contains("llm_tokens_total{type=\"prompt\"} 300"));
        assert!(output.contains("llm_tokens_total{type=\"completion\"} 125"));
    }

    #[test]
    fn record_sync_run_increments() {
        let state = MetricsState::new();
        state.record_sync_run("confluence", "completed", 12.5, 10, 5, 3, 0);
        state.record_sync_run("confluence", "failed", 2.0, 0, 0, 0, 1);

        let output = state.encode();
        assert!(
            output.contains("mcp_sync_runs_total{connector=\"confluence\",status=\"completed\"} 1")
        );
        assert!(
            output.contains("mcp_sync_runs_total{connector=\"confluence\",status=\"failed\"} 1")
        );
        assert!(
            output.contains("mcp_sync_items_total{action=\"created\",connector=\"confluence\"} 10")
        );
        assert!(
            output.contains("mcp_sync_items_total{action=\"updated\",connector=\"confluence\"} 5")
        );
        assert!(
            output.contains("mcp_sync_items_total{action=\"skipped\",connector=\"confluence\"} 3")
        );
        assert!(
            output.contains("mcp_sync_items_total{action=\"failed\",connector=\"confluence\"} 1")
        );
        assert!(output.contains("mcp_sync_duration_seconds"));
    }

    #[test]
    fn set_sessions_gauge() {
        let state = MetricsState::new();
        state.set_active_sessions(42);

        let output = state.encode();
        assert!(output.contains("active_sessions_total 42"));
    }

    #[test]
    fn record_streaming_redaction_increments() {
        let state = MetricsState::new();
        state.record_streaming_redaction("PII_THAI_ID", "output");
        state.record_streaming_redaction("PII_THAI_ID", "output");
        state.record_streaming_redaction("PII_EMAIL", "output");

        let output = state.encode();
        assert!(output.contains("guardrail_streaming_redactions_total"));
        assert!(output.contains(
            "guardrail_streaming_redactions_total{code=\"PII_THAI_ID\",stage=\"output\"} 2"
        ));
        assert!(output.contains(
            "guardrail_streaming_redactions_total{code=\"PII_EMAIL\",stage=\"output\"} 1"
        ));
    }

    #[test]
    fn metrics_state_implements_recorder_trait() {
        // The pipeline only sees the trait — confirm dispatch works through
        // the Arc<dyn ...> coercion that AppState relies on.
        use std::sync::Arc;
        use thairag_core::traits::GuardrailMetricsRecorder;

        let state = Arc::new(MetricsState::new());
        let as_trait: Arc<dyn GuardrailMetricsRecorder> = state.clone();
        as_trait.record_streaming_redaction("SECRET_AWS_KEY", "output");

        let output = state.encode();
        assert!(output.contains(
            "guardrail_streaming_redactions_total{code=\"SECRET_AWS_KEY\",stage=\"output\"} 1"
        ));
    }
}
