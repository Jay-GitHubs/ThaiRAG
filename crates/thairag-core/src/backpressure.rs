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

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::error::Result;
use crate::traits::LlmProvider;
use crate::types::{ChatMessage, LlmResponse, LlmStreamResponse, VisionMessage};

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
        self.0.generate(messages, max_tokens).await
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
        self.0
            .generate_structured(messages, max_tokens, json_schema)
            .await
    }

    async fn generate_vision(
        &self,
        messages: &[VisionMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let _slot = bulk_semaphore().clone().acquire_owned().await;
        self.0.generate_vision(messages, max_tokens).await
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
