use std::time::Duration;

use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;
use tracing::{info, instrument};

pub struct OllamaEmbeddingProvider {
    client: reqwest::Client,
    model: String,
    dimension: usize,
    endpoint: String,
    keep_alive: Option<serde_json::Value>,
}

impl OllamaEmbeddingProvider {
    pub fn new(base_url: &str, model: &str, dimension: usize) -> Self {
        Self::with_keep_alive(base_url, model, dimension, None)
    }

    pub fn with_keep_alive(
        base_url: &str,
        model: &str,
        dimension: usize,
        keep_alive: Option<&str>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to build reqwest client");

        let base = base_url.trim_end_matches('/');
        let endpoint = format!("{base}/api/embed");

        let keep_alive_val = keep_alive.map(|s| {
            if let Ok(n) = s.parse::<i64>() {
                serde_json::Value::Number(serde_json::Number::from(n))
            } else {
                serde_json::Value::String(s.to_string())
            }
        });

        info!(model, dimension, %endpoint, ?keep_alive, "Initialized Ollama embedding provider");

        Self {
            client,
            model: model.to_string(),
            dimension,
            endpoint,
            keep_alive: keep_alive_val,
        }
    }
}

/// Max cumulative BYTES per /api/embed request, and per-input truncation cap.
///
/// The runner's `truncate` only applies per sequence — a BATCH whose total
/// tokens exceed the runner's ~8K context KILLS the runner (HTTP 400 /
/// internal EOF) instead of truncating; reproduced deterministically with
/// real Thai government PDFs. Char- or ratio-based budgets cannot bound this:
/// legacy Thai PDF fonts emit Private Use Area codepoints (U+F70x) that
/// byte-fallback-tokenize at up to 3 tokens per character. The only bound BPE
/// guarantees is tokens ≤ BYTES, so we budget bytes: 6,000 < 8,192 holds for
/// ANY content. A 1024-dim embedding gains nothing past this length anyway;
/// BM25 still indexes the full text.
const MAX_BATCH_BYTES: usize = 6_000;
const MAX_INPUT_BYTES: usize = 6_000;
/// Explicit context window for embed requests. The runner otherwise
/// preallocates KV for the model's full 32K context (~5.8 GB observed for a
/// 0.6B embedder), which dies sporadically under Metal memory pressure when a
/// large chat model is resident. 8K halves it twice and still exceeds
/// MAX_INPUT_CHARS worth of tokens.
const EMBED_NUM_CTX: usize = 8_192;

impl OllamaEmbeddingProvider {
    async fn embed_one_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut body = serde_json::json!({
            "model": self.model,
            "input": texts,
            "truncate": true,
            "options": {"num_ctx": EMBED_NUM_CTX},
        });
        if let Some(ref ka) = self.keep_alive {
            body["keep_alive"] = ka.clone();
        }

        // Ollama's embedding runner can die under memory pressure (surfaced
        // as HTTP 400 wrapping an internal "EOF"); it respawns on the next
        // request, so one failure must not fail a whole document ingest.
        for attempt in 0..3u32 {
            match self.embed_request(&body).await {
                Ok(v) => return Ok(v),
                Err(e) if attempt < 2 && e.to_string().contains("EOF") => {
                    tracing::warn!(attempt, error = %e, "Ollama embed runner died; retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(2 * (attempt as u64 + 1)))
                        .await;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!("loop returns on attempt 2")
    }

    async fn embed_request(&self, body: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
        let resp = self
            .client
            .post(&self.endpoint)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                let msg = if e.is_connect() {
                    format!(
                        "Cannot connect to Ollama embedding service at {}. \
                         Is Ollama running? Check that the URL is correct.",
                        self.endpoint
                    )
                } else if e.is_timeout() {
                    format!(
                        "Ollama embedding request timed out ({}). \
                         The model may be loading or the server is overloaded.",
                        self.endpoint
                    )
                } else {
                    format!("Ollama embedding request failed: {e}")
                };
                ThaiRagError::Embedding(msg)
            })?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Embedding(format!(
                "Ollama embedding returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::Embedding(format!("Failed to parse Ollama embedding response: {e}"))
        })?;

        let embeddings_arr = json["embeddings"].as_array().ok_or_else(|| {
            ThaiRagError::Embedding("Missing embeddings array in Ollama response".into())
        })?;

        let embeddings: Vec<Vec<f32>> = embeddings_arr
            .iter()
            .map(|arr| {
                arr.as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
            .collect();

        Ok(embeddings)
    }
}

#[async_trait]
impl EmbeddingModel for OllamaEmbeddingProvider {
    #[instrument(skip(self, texts), fields(model = %self.model, text_count = texts.len()))]
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let texts: Vec<String> = texts
            .iter()
            .map(|t| truncate_bytes(sanitize_degenerate_thai(t), MAX_INPUT_BYTES))
            .collect();

        let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        let mut batch: Vec<String> = Vec::new();
        let mut batch_bytes = 0usize;
        for t in texts {
            let len = t.len();
            if !batch.is_empty() && batch_bytes + len > MAX_BATCH_BYTES {
                out.extend(self.embed_one_batch(&batch).await?);
                batch.clear();
                batch_bytes = 0;
            }
            batch_bytes += len;
            batch.push(t);
        }
        if !batch.is_empty() {
            out.extend(self.embed_one_batch(&batch).await?);
        }
        Ok(out)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Truncate to at most `max_bytes`, respecting char boundaries.
fn truncate_bytes(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Collapse degenerate runs of Thai combining marks before embedding.
///
/// Garbled extractions (e.g. a rotated PDF cover page) can emit thousands of
/// consecutive base-less vowel/tone marks ("\u{0E35}\u{0E48}\u{0E34}..."),
/// which crashes Ollama's embedding runner (HTTP 400 / connection EOF,
/// reproduced at ~1500+ marks) and fails ingestion of the WHOLE document.
/// Legitimate Thai stacks at most ~3 combining marks per base consonant, so
/// runs are capped at 3 and the remainder dropped; normal text is unchanged.
fn sanitize_degenerate_thai(text: &str) -> String {
    fn is_mark(c: char) -> bool {
        matches!(c,
            '\u{0E31}'
            | '\u{0E34}'..='\u{0E3A}'
            | '\u{0E47}'..='\u{0E4E}'
        )
    }
    let mut out = String::with_capacity(text.len());
    let mut run = 0usize;
    for c in text.chars() {
        if is_mark(c) {
            run += 1;
            if run > 3 {
                continue;
            }
        } else {
            run = 0;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_degenerate_thai;

    #[test]
    fn normal_thai_unchanged() {
        let s = "ผู้ประกอบการต้องตรวจสอบใบอนุญาตที่นี่";
        assert_eq!(sanitize_degenerate_thai(s), s);
    }

    #[test]
    fn degenerate_mark_runs_are_capped() {
        let garbled: String = "\u{0E35}\u{0E48}\u{0E34}\u{0E38}\u{0E39}".repeat(400);
        let out = sanitize_degenerate_thai(&garbled);
        assert!(out.chars().count() <= 3, "run capped, got {}", out.len());
    }

    #[test]
    fn marks_reset_after_base_consonant() {
        // Each base consonant legitimately carries marks; no truncation.
        let s = "กี่บ้านน้ำ";
        assert_eq!(sanitize_degenerate_thai(s), s);
    }
}
