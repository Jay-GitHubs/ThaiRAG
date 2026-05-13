# Streaming output guardrails: real prevention design

**Status:** Draft — not yet approved. Reviewers, please weigh in on the four open questions before any implementation PR lands.

**Owner:** TBD
**Last updated:** 2026-05-13

---

## 1. Problem

`ChatPipeline::wrap_stream_with_output_guardrails` (in `crates/thairag-agent/src/chat_pipeline.rs`) currently runs the deterministic detector set **after** the stream's EOS marker. By that time every leaked token is already on the SSE wire. The function then appends an audit-style warning chunk so the user is at least notified, but the sensitive content has been delivered.

For PDPA-regulated deployments this is not real prevention. The non-streaming path (`apply_output_guardrails`) is already correct — it can buffer, run detectors, and redact before returning. Only the streaming path has this gap.

## 2. Threat model

In scope:
- **Hallucinated PII.** The LLM emits a plausible-looking Thai national ID, Thai phone number, credit card, or email address that it didn't see in context.
- **RAG leakage.** Retrieval pulls a chunk containing real PII into the context window; the LLM faithfully relays it.
- **Token / secret leakage.** AWS keys, JWTs, GitHub PATs, generic API keys (matching the existing patterns in `crates/thairag-agent/src/guardrails/detectors/secrets.rs`).
- **Operator-defined blocklist phrases.**

Out of scope (separate problem; would need an LLM moderator, not deterministic regex):
- Semantic toxicity, harassment, hate speech.
- Jailbreak-induced content (handled at input stage).
- Policy violations that require world knowledge to detect.

## 3. Current behavior

```
inner stream ──► forward verbatim ──► client
                                       │
              after EOS, run detectors │
                       │               │
                       ▼               ▼
              record in metadata   append audit note
```

This is documented in the existing function's doc-comment (see `chat_pipeline.rs` line ~1878).

## 4. Design space considered

| Option | Real prevention? | Latency cost | UX | Verdict |
|---|---|---|---|---|
| **A.** Buffer entire response, then stream | ✅ | Kills streaming | None | Defeats the streaming UX entirely. Rejected. |
| **B.** Sliding-window hold-back | ✅ for bounded patterns | One window's worth of generation | Slight TTFB delay | **Recommended.** |
| **C.** Per-chunk check + cancel on hit | ⚠️ Partial — sub-token patterns escape | None | One false positive ends the whole response | Brutal blast radius. Rejected. |
| **D.** LLM-side moderation pass | ✅ Semantic too | 2× cost, +RTT | Adds noticeable lag | Out of scope here — would address §2 "out of scope" items, not §2 "in scope". |
| **E.** Generate non-streaming, then re-stream the cleaned text | ✅ | Full response latency before any token | Same as A | Functionally identical to A. Rejected. |

## 5. Recommendation: Option B — sliding-window hold-back

### 5.1 Mechanism

1. Introduce a `StreamingGuardrails` wrapper. It owns a hold-back buffer of bounded size (`streaming_window_chars`, default proposed below).
2. As each inner chunk arrives:
   - Append to the buffer.
   - Run the deterministic detector set on the **whole buffer** (this handles patterns split across chunk boundaries).
   - On match: pass `(buffer, violations)` to `OutputGuardrails::sanitize` (already exists from PR #43), replace the buffer with the redacted text, record violations in metadata.
3. Once the buffer exceeds `streaming_window_chars`, flush the *oldest* characters to the client. Those characters are now "safe": any pattern that included them must have already fired during step 2.
4. On EOS:
   - Run detectors one last time on the remaining buffer.
   - Sanitize if needed; flush everything; emit the existing audit metadata.
5. Failure mode is governed by `GuardrailsConfig::fail_open` (already in the config schema). If a detector panics, fail-open keeps streaming; fail-closed cancels the stream and emits a generic refusal.

### 5.2 Why this works

The longest bounded detector pattern in the current set is the generic API key match (~80 chars max, anchored by `key=`/`token=`/`Bearer` plus 24+ char suffix). Any window ≥ 128 chars covers every bounded pattern. By the time we flush a character, it has stayed in the buffer for `streaming_window_chars` worth of subsequent characters — long enough for any bounded pattern to fully materialize.

### 5.3 Unbounded patterns

JWTs are not bounded — header.payload.signature can be 1KB+. With a 256-char window, a 1KB JWT will stream its first ~750 chars as "safe" before the trailing portion forms a match. Trade-off:

- A truncated JWT is unusable — verification requires the full token including the signature suffix. So leaking a prefix is recoverable, not catastrophic.
- Operators who care about JWT prefix leakage can raise `streaming_window_chars` at the cost of TTFB.

Document this explicitly in the config field doc-comment.

## 6. Latency budget

Default proposal: `streaming_window_chars = 256`.

- At typical LLM streaming rate (~30 tok/s ≈ 150 chars/s in English, ~80 chars/s in Thai), 256 chars = **1.7–3.2 seconds of TTFB delay** before the first visible chunk leaves the server.
- After the warm-up, throughput is identical — each new chunk flushes one chunk's worth of the oldest buffered characters.
- Memory: O(window) per active stream. Negligible.
- CPU: every deterministic detector regex runs once per chunk on a `window + chunk_len` string. For 256 chars × 11 regexes (5 PII + 4 secrets + 1 blocklist + 1 injection-not-run-on-output), well under 1ms per chunk on commodity hardware.

## 7. SSE protocol question (open)

When a redaction fires mid-stream, what does the client see?

- **Option α** — inline: the buffered chars are replaced with `[REDACTED]` (or `policy.redaction_token`) and streamed inline. Client sees the marker as part of the response text. Simple; no client changes needed; works with any SSE-aware UI including Open WebUI.
- **Option β** — out-of-band event: emit a separate SSE event like `event: redacted\ndata: {"code": "PII_THAI_ID"}\n` and silently drop the matched chars from the text stream. Client decides how to render. Admin UI can show a chip; bare clients show nothing.

Option α is mechanically simpler and degrades gracefully on every client. Option β is nicer UX where supported but needs admin-ui changes. **Recommend α for the first iteration**, leave β as a future enhancement.

## 8. Phasing

### PR-1 — Infra
- `streaming_window_chars: usize` field on `GuardrailsConfig` (default 256).
- New `StreamingGuardrails` (or method on `OutputGuardrails`) implementing the hold-back algorithm.
- Swap `wrap_stream_with_output_guardrails` to use it. Keep the post-EOS audit pathway as a final-flush step.
- Tests: window-flush ordering; single-pattern redaction mid-stream; pattern split across chunk boundary; pattern at EOS; multi-pattern overlap (relies on the `redact()` overlap-merge fix from PR #43); fail-open vs fail-closed.

### PR-2 — UX polish (depends on the open question α vs β)
- If α: nothing further needed beyond reusing `policy.redaction_token`.
- If β: SSE event wiring + admin-ui chip + Open WebUI integration test.

### PR-3 — Observability
- Prometheus counter `guardrail_streaming_redactions_total{code, stage="output"}` (cardinality already constrained by the closed `ViolationCode` enum).
- Sampled `tracing::warn` of which detector fired (codes only — never matched text, per existing PDPA-safe convention).

## 9. Open questions

1. **Default window size.** 256 chars (~2 s TTFB at 150 chars/s) or stricter / looser?
2. **SSE protocol for the marker.** Inline `[REDACTED]` (α) vs separate `event: redacted` channel (β)?
3. **Failure semantics.** If a detector panics mid-stream, default to fail-open (current behavior on the non-streaming path) or fail-closed (cancel + refusal)?
4. **Scope of the first PR.** Just the deterministic detectors (current set), or also wire a hook for a future LLM-moderator call so Option D can layer on later without re-touching this code?

## 10. Out of scope for this design

- LLM-side semantic moderation (Option D). Worth a separate design; would extend the `OutputGuardrails` trait but not the streaming wrapper.
- Backpressure on the inner LLM stream. Currently the wrapper is passive (drains chunks as fast as the inner stream produces them).
- Re-generation on detection (the existing `Regenerate` `GuardAction`). Not meaningful in a streaming context — the response is being emitted, not retried — so the existing Regenerate-fallback redaction (PR #43) continues to apply.
