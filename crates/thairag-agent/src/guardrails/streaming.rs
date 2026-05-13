//! Streaming output guardrails: sliding-window hold-back for real prevention.
//!
//! See `docs/STREAMING_GUARDRAILS_DESIGN.md` for the design rationale.
//!
//! Each chunk emitted by the inner LLM stream is appended to a hold-back
//! buffer. After every append the deterministic detector set runs against
//! the **entire** buffer (so patterns that straddle chunk boundaries are
//! still caught). On match, the buffer is redacted in place via
//! `OutputGuardrails::sanitize` so the configured `redaction_token`
//! (default `[REDACTED]`) lands inline in the SSE text stream.
//!
//! Characters that have aged past `policy.streaming_window_chars` are
//! flushed to the client — those characters are now safe because any
//! bounded pattern that contained them must have already fired against
//! the larger buffer that included them.
//!
//! On EOS (or an upstream error), a final detect-and-flush runs to catch
//! patterns that complete in the last chunk and to drain the buffer.
//!
//! Unbounded patterns (very long JWTs) may stream a prefix before the
//! suffix arrives — see §5.3 of the design doc. A truncated JWT is
//! unusable, so leaking a prefix is recoverable.

use std::sync::Arc;

use thairag_core::error::ThaiRagError;
use thairag_core::types::{GuardrailViolationMeta, LlmStreamResponse};
use tokio_stream::StreamExt;
use tracing::debug;

use crate::guardrails::OutputGuardrails;
use crate::guardrails::types::GuardAction;
use crate::guardrails::violations_to_meta;

/// Callback invoked with the violation-meta records each time the streaming
/// guard fires. Kept as a generic closure so this module doesn't need to know
/// about `PipelineMetadata` / `MetadataCell` (which live in the pipeline).
pub type ViolationsObserver = Arc<dyn Fn(Vec<GuardrailViolationMeta>) + Send + Sync>;

/// Wrap an outgoing stream with the sliding-window output guardrail.
///
/// The returned `LlmStreamResponse` preserves the inner stream's `usage`
/// handle so callers downstream can still read prompt/completion token
/// counts when the inner provider eventually fills it.
pub fn wrap_stream_with_holdback(
    inner: LlmStreamResponse,
    guard: Arc<OutputGuardrails>,
    on_violations: ViolationsObserver,
) -> LlmStreamResponse {
    let usage = inner.usage.clone();
    let stream = async_stream::stream! {
        let mut inner_stream = inner.stream;
        let mut buffer = String::new();
        let window = guard.policy().streaming_window_chars;

        while let Some(item) = inner_stream.next().await {
            match item {
                Ok(text) => {
                    buffer.push_str(&text);
                    apply_detectors(&mut buffer, &guard, &on_violations);

                    // Flush any chars that have aged past the window. They're
                    // safe to release: any bounded pattern containing them
                    // would have completed inside the larger buffer that held
                    // them and been redacted before this point.
                    let len_chars = buffer.chars().count();
                    if len_chars > window {
                        let drop_n = len_chars - window;
                        let split = buffer
                            .char_indices()
                            .nth(drop_n)
                            .map(|(i, _)| i)
                            .unwrap_or(buffer.len());
                        let flushed: String = buffer.drain(..split).collect();
                        if !flushed.is_empty() {
                            yield Ok::<_, ThaiRagError>(flushed);
                        }
                    }
                }
                Err(e) => {
                    // Inner stream errored. Drain the redacted residual first
                    // so the client doesn't lose buffered content that's
                    // already been scanned; then surface the error.
                    if !buffer.is_empty() {
                        apply_detectors(&mut buffer, &guard, &on_violations);
                        yield Ok::<_, ThaiRagError>(std::mem::take(&mut buffer));
                    }
                    yield Err(e);
                    return;
                }
            }
        }

        // EOS: run detection on whatever's still buffered, then flush it all.
        if !buffer.is_empty() {
            apply_detectors(&mut buffer, &guard, &on_violations);
            yield Ok::<_, ThaiRagError>(std::mem::take(&mut buffer));
        }
    };

    LlmStreamResponse {
        stream: Box::pin(stream),
        usage,
    }
}

/// Run the deterministic detector set on `buffer`. If it fires, redact in
/// place (via `OutputGuardrails::sanitize`) and notify the observer with the
/// wire-safe violation metadata.
///
/// `Block` and `Regenerate` actions are downgraded to redact in the streaming
/// path: we've already started emitting content to the client, so refusing
/// would be confusing UX and re-generating isn't available here.
fn apply_detectors(
    buffer: &mut String,
    guard: &OutputGuardrails,
    on_violations: &ViolationsObserver,
) {
    let verdict = guard.check(buffer);
    if verdict.passed() {
        return;
    }
    let codes: Vec<&str> = verdict.violations.iter().map(|v| v.code.as_str()).collect();
    debug!(?codes, "Streaming guardrails: redacted in window");

    on_violations(violations_to_meta(&verdict.violations));

    match verdict.action {
        GuardAction::Sanitize(redacted) => *buffer = redacted,
        // For Block / Regenerate / Pass-but-non-empty, fall through to a
        // direct call to `sanitize` so we always get redacted text back.
        _ => *buffer = guard.sanitize(buffer, &verdict.violations),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use thairag_config::schema::GuardrailsConfig;
    use thairag_core::types::LlmUsage;
    use tokio_stream::{StreamExt, iter as stream_iter};

    fn config(window: usize, build: impl FnOnce(&mut GuardrailsConfig)) -> GuardrailsConfig {
        let mut c = GuardrailsConfig {
            streaming_window_chars: window,
            ..Default::default()
        };
        build(&mut c);
        c
    }

    fn wrap_chunks(
        chunks: Vec<&'static str>,
        cfg: GuardrailsConfig,
    ) -> (
        Vec<Result<String, ThaiRagError>>,
        Vec<GuardrailViolationMeta>,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let guard = Arc::new(OutputGuardrails::new(cfg));
            let recorded = Arc::new(Mutex::new(Vec::new()));
            let recorded_clone = Arc::clone(&recorded);
            let observer: ViolationsObserver = Arc::new(move |v| {
                recorded_clone.lock().unwrap().extend(v);
            });

            let inner_stream = stream_iter(
                chunks
                    .into_iter()
                    .map(|c| Ok::<String, ThaiRagError>(c.to_string())),
            );
            let inner = LlmStreamResponse {
                stream: Box::pin(inner_stream),
                usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
            };

            let mut wrapped = wrap_stream_with_holdback(inner, guard, observer);
            let mut out = Vec::new();
            while let Some(chunk) = wrapped.stream.next().await {
                out.push(chunk);
            }
            let v = recorded.lock().unwrap().clone();
            (out, v)
        })
    }

    fn collect_ok(items: Vec<Result<String, ThaiRagError>>) -> String {
        items.into_iter().filter_map(|r| r.ok()).collect()
    }

    #[test]
    fn passthrough_when_no_detectors_enabled() {
        let cfg = config(8, |_| {});
        let (chunks, violations) = wrap_chunks(vec!["hello ", "world ", "this is fine"], cfg);
        assert_eq!(collect_ok(chunks), "hello world this is fine");
        assert!(violations.is_empty());
    }

    #[test]
    fn redacts_email_mid_stream_before_emitting() {
        // Window large enough to hold the whole email before it ages out.
        let cfg = config(64, |c| {
            c.detect_email = true;
            c.output_on_violation = "redact".into();
            c.redaction_token = "[REDACTED]".into();
        });
        let (chunks, violations) = wrap_chunks(
            vec![
                "Contact me at ",
                "alice@example.com",
                " thanks.",
                " padding ".repeat(20).leak(),
            ],
            cfg,
        );
        let full = collect_ok(chunks);
        assert!(
            !full.contains("alice@example.com"),
            "raw email leaked: {full}"
        );
        assert!(full.contains("[REDACTED]"), "no redaction marker: {full}");
        assert!(!violations.is_empty());
    }

    #[test]
    fn detects_pattern_split_across_chunk_boundary() {
        let cfg = config(64, |c| {
            c.detect_email = true;
            c.output_on_violation = "redact".into();
        });
        let (chunks, violations) = wrap_chunks(
            // Split the email so neither chunk alone matches the regex.
            vec![
                "Ping ",
                "bob@exam",
                "ple.com",
                " done.",
                " tail tail tail tail tail tail tail tail",
            ],
            cfg,
        );
        let full = collect_ok(chunks);
        assert!(!full.contains("bob@example.com"));
        assert!(full.contains("[REDACTED]"));
        assert!(!violations.is_empty());
    }

    #[test]
    fn detects_pattern_at_eos_via_final_flush() {
        // Pattern only completes in the very last chunk; window never overflows
        // before EOS, so the EOS path must do the detection.
        let cfg = config(256, |c| {
            c.detect_thai_id = true;
            c.output_on_violation = "redact".into();
        });
        let (chunks, violations) = wrap_chunks(vec!["My ID is 110170023", "0708 thanks"], cfg);
        let full = collect_ok(chunks);
        assert!(!full.contains("1101700230708"));
        assert!(full.contains("[REDACTED]"));
        assert!(!violations.is_empty());
    }

    #[test]
    fn block_policy_downgraded_to_redact_in_stream() {
        // Streaming path must NEVER emit unredacted content even if the
        // operator chose `output_on_violation = "block"` for non-streaming.
        let cfg = config(64, |c| {
            c.detect_email = true;
            c.output_on_violation = "block".into();
        });
        let (chunks, violations) = wrap_chunks(
            vec![
                "hi ",
                "bad@example.com",
                " end ",
                " tail tail tail tail tail tail tail tail ",
            ],
            cfg,
        );
        let full = collect_ok(chunks);
        assert!(!full.contains("bad@example.com"));
        assert!(full.contains("[REDACTED]"));
        assert!(!violations.is_empty());
    }

    #[test]
    fn flushes_safe_prefix_once_window_exceeded() {
        // No detectors enabled — verify the windowing alone behaves: 32-char
        // window means after we've buffered >32 chars, the prefix flushes.
        let cfg = config(8, |_| {});
        let (chunks, _) = wrap_chunks(vec!["abcdefgh", "ijklmnop", "qrstuvwx", "yz"], cfg);
        // Output equals input in order, no reordering.
        assert_eq!(collect_ok(chunks), "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn forwards_inner_stream_error() {
        // Inner stream error mid-flight: the wrapper must drain the safe
        // residual and then surface the error.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let cfg = config(8, |_| {});
            let guard = Arc::new(OutputGuardrails::new(cfg));
            let observer: ViolationsObserver = Arc::new(|_| {});

            let items: Vec<Result<String, ThaiRagError>> = vec![
                Ok("hello".into()),
                Err(ThaiRagError::Internal("simulated upstream".into())),
            ];
            let inner = LlmStreamResponse {
                stream: Box::pin(stream_iter(items)),
                usage: Arc::new(Mutex::new(Some(LlmUsage::default()))),
            };
            let mut wrapped = wrap_stream_with_holdback(inner, guard, observer);
            let mut got_text = String::new();
            let mut got_err = false;
            while let Some(item) = wrapped.stream.next().await {
                match item {
                    Ok(s) => got_text.push_str(&s),
                    Err(_) => {
                        got_err = true;
                        break;
                    }
                }
            }
            assert_eq!(got_text, "hello");
            assert!(got_err);
        });
    }
}
