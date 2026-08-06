//! Bulk-lane backpressure for upstream LLM gateways.
//!
//! Document ingestion fans out aggressively (converter segments × chunker
//! windows × enricher chunks, plus concurrently-processing documents), and an
//! OpenAI-compatible gateway has a finite worker-slot pool. Unbounded fan-out
//! doesn't fail fast — the gateway 503-storms ("timed out waiting for a free
//! slot") and every caller burns its retry budget in lockstep. Measured live:
//! a single-worker e2e suite generated 280 gateway 503s in 30 minutes purely
//! from its own ingestion fan-out.
//!
//! The first attempt at a fix — a GLOBAL per-host cap at the provider layer —
//! eliminated the 503s but created head-of-line blocking: interactive chat
//! requests queued behind hundreds of bulk ingestion calls (measured live:
//! chat e2e pass rate collapsed 40/42 → 29/42). The correct shape is a
//! PRIORITY split: only BULK work queues; interactive traffic never waits.
//!
//! [`Throttled`] wraps an [`LlmProvider`] with a process-global bulk
//! semaphore. Apply it to the providers that serve ingestion (preprocessing
//! agents, page-OCR/vision, facet extraction) and leave chat/query providers
//! unwrapped. Deploy-time knob: `THAIRAG_INGEST_MAX_CONCURRENT` (default 2 —
//! leaves gateway slots free for interactive traffic at all times).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::error::Result;
use crate::traits::LlmProvider;
use crate::types::{ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, VisionMessage};

/// Accumulates LLM token usage during a single ingestion, so the true
/// preprocessing cost (analyzer/converter/enricher/vision LLM calls) can be
/// attributed to the document. Threaded via a task-local rather than every
/// pipeline return type: ingestion runs in one task and its LLM stages are
/// awaited inline, so a scope set at the top of the ingest task sees them all.
#[derive(Debug, Default)]
pub struct IngestTokenTally {
    prompt: AtomicU64,
    completion: AtomicU64,
}

impl IngestTokenTally {
    pub fn add(&self, usage: &LlmUsage) {
        self.prompt
            .fetch_add(usage.prompt_tokens as u64, Ordering::Relaxed);
        self.completion
            .fetch_add(usage.completion_tokens as u64, Ordering::Relaxed);
    }

    /// (prompt_tokens, completion_tokens) accumulated so far.
    pub fn totals(&self) -> (u64, u64) {
        (
            self.prompt.load(Ordering::Relaxed),
            self.completion.load(Ordering::Relaxed),
        )
    }
}

tokio::task_local! {
    static INGEST_TALLY: Arc<IngestTokenTally>;
}

/// Run `fut` with an ingest token tally in scope. Every [`Throttled`] LLM call
/// made within (the ingestion chokepoint) accumulates its usage into `tally`.
/// Chat requests never set this scope, so metering is a no-op for them.
pub async fn with_ingest_tally<F, T>(tally: Arc<IngestTokenTally>, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    INGEST_TALLY.scope(tally, fut).await
}

/// Add usage to the active ingest tally if one is in scope; no-op otherwise.
fn record_ingest_usage(usage: &LlmUsage) {
    let _ = INGEST_TALLY.try_with(|t| t.add(usage));
}

fn bulk_semaphore() -> &'static Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| {
        let limit = std::env::var("THAIRAG_INGEST_MAX_CONCURRENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &usize| *v > 0)
            .unwrap_or(2);
        Arc::new(Semaphore::new(limit))
    })
}

/// Wraps an LLM provider so every call takes a slot in the process-global
/// BULK lane. See module docs for why only bulk providers get wrapped.
/// IMPORTANT: wrap each ingestion provider handle exactly ONCE — a
/// double-wrapped provider acquires two permits per call, which can deadlock
/// the whole lane at small limits.
pub struct Throttled(pub Arc<dyn LlmProvider>);

#[async_trait]
impl LlmProvider for Throttled {
    async fn generate(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let _slot = bulk_semaphore().clone().acquire_owned().await;
        let resp = self.0.generate(messages, max_tokens).await?;
        record_ingest_usage(&resp.usage);
        Ok(resp)
    }

    async fn generate_stream(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmStreamResponse> {
        // Permit covers the initial request only (streams outlive the call).
        let _slot = bulk_semaphore().clone().acquire_owned().await;
        self.0.generate_stream(messages, max_tokens).await
    }

    async fn generate_structured(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
        json_schema: &serde_json::Value,
    ) -> Result<LlmResponse> {
        let _slot = bulk_semaphore().clone().acquire_owned().await;
        let resp = self
            .0
            .generate_structured(messages, max_tokens, json_schema)
            .await?;
        record_ingest_usage(&resp.usage);
        Ok(resp)
    }

    async fn generate_vision(
        &self,
        messages: &[VisionMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let _slot = bulk_semaphore().clone().acquire_owned().await;
        let resp = self.0.generate_vision(messages, max_tokens).await?;
        record_ingest_usage(&resp.usage);
        Ok(resp)
    }

    fn model_name(&self) -> &str {
        self.0.model_name()
    }

    fn supports_vision(&self) -> bool {
        self.0.supports_vision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Stub that reports fixed token usage on every call.
    struct TokenStub {
        prompt: u32,
        completion: u32,
    }

    #[async_trait]
    impl LlmProvider for TokenStub {
        async fn generate(&self, _m: &[ChatMessage], _t: Option<u32>) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: "ok".into(),
                usage: LlmUsage {
                    prompt_tokens: self.prompt,
                    completion_tokens: self.completion,
                },
            })
        }
        fn model_name(&self) -> &str {
            "token-stub"
        }
    }

    #[tokio::test]
    async fn ingest_tally_accumulates_within_scope() {
        let provider = Throttled(Arc::new(TokenStub {
            prompt: 100,
            completion: 20,
        }));
        let tally = Arc::new(IngestTokenTally::default());
        with_ingest_tally(Arc::clone(&tally), async {
            provider.generate(&[], None).await.unwrap();
            provider.generate(&[], None).await.unwrap();
            provider
                .generate_structured(&[], None, &serde_json::json!({}))
                .await
                .unwrap();
        })
        .await;
        // 3 calls x (100 prompt, 20 completion)
        assert_eq!(tally.totals(), (300, 60));
    }

    #[tokio::test]
    async fn no_scope_is_passthrough_no_panic() {
        // Without a tally scope (the chat path), metering is a silent no-op
        // and the response is returned unchanged.
        let provider = Throttled(Arc::new(TokenStub {
            prompt: 100,
            completion: 20,
        }));
        let resp = provider.generate(&[], None).await.unwrap();
        assert_eq!(resp.usage.prompt_tokens, 100);
        assert_eq!(resp.content, "ok");
    }

    struct SlowStub {
        in_flight: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmProvider for SlowStub {
        async fn generate(&self, _m: &[ChatMessage], _t: Option<u32>) -> Result<LlmResponse> {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(LlmResponse {
                content: "ok".into(),
                usage: Default::default(),
            })
        }
        fn model_name(&self) -> &str {
            "stub"
        }
    }

    #[tokio::test]
    async fn bulk_lane_caps_concurrent_calls() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let p = Arc::new(Throttled(Arc::new(SlowStub {
            in_flight: Arc::clone(&in_flight),
            peak: Arc::clone(&peak),
        })));
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let p = Arc::clone(&p);
            tasks.push(tokio::spawn(async move {
                p.generate(&[], None).await.unwrap();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        // Default bulk limit is 2 (no env override in tests).
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "bulk lane exceeded cap: {}",
            peak.load(Ordering::SeqCst)
        );
    }
}
