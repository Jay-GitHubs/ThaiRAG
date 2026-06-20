use std::time::Duration;

use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;
use tracing::{info, instrument};

pub struct OpenAiEmbeddingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    dimension: usize,
    endpoint: String,
}

impl OpenAiEmbeddingProvider {
    pub fn new(api_key: &str, model: &str, dimension: usize, base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build reqwest client");

        // Accept base URLs with or without a trailing `/v1` (e.g.
        // `https://host` or `https://host/v1/`) without producing a duplicated
        // `/v1/v1/embeddings`.
        let base = if base_url.is_empty() {
            "https://api.openai.com"
        } else {
            let trimmed = base_url.trim_end_matches('/');
            trimmed.strip_suffix("/v1").unwrap_or(trimmed)
        };
        let endpoint = format!("{base}/v1/embeddings");

        info!(model, dimension, %endpoint, "Initialized OpenAI embedding provider");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimension,
            endpoint,
        }
    }
}

#[async_trait]
impl EmbeddingModel for OpenAiEmbeddingProvider {
    #[instrument(skip(self, texts), fields(model = %self.model, text_count = texts.len()))]
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
            "dimensions": self.dimension,
        });

        let resp = crate::retry::send_with_retry(
            || {
                self.client
                    .post(&self.endpoint)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
            },
            "openai_embedding.embed",
        )
        .await
        .map_err(|e| ThaiRagError::Embedding(format!("OpenAI embedding request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Embedding(format!(
                "OpenAI embedding returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::Embedding(format!("Failed to parse OpenAI embedding response: {e}"))
        })?;

        let data = json["data"].as_array().ok_or_else(|| {
            ThaiRagError::Embedding("Missing data array in OpenAI embedding response".into())
        })?;

        let mut embeddings: Vec<(usize, Vec<f32>)> = data
            .iter()
            .map(|item| {
                let index = item["index"].as_u64().unwrap_or(0) as usize;
                let embedding: Vec<f32> = item["embedding"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                (index, embedding)
            })
            .collect();

        // Sort by index to match input order
        embeddings.sort_by_key(|(i, _)| *i);

        Ok(embeddings.into_iter().map(|(_, emb)| emb).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::traits::EmbeddingModel;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn endpoint_does_not_double_v1() {
        let p = OpenAiEmbeddingProvider::new(
            "k",
            "embed-qwen3",
            1024,
            "https://llm.jay-tech-ai.com/v1/",
        );
        assert_eq!(p.endpoint, "https://llm.jay-tech-ai.com/v1/embeddings");

        let p = OpenAiEmbeddingProvider::new("k", "m", 1024, "https://host");
        assert_eq!(p.endpoint, "https://host/v1/embeddings");

        let p = OpenAiEmbeddingProvider::new("k", "m", 1024, "");
        assert_eq!(p.endpoint, "https://api.openai.com/v1/embeddings");
    }

    /// A flaky upstream that returns 503 twice then a valid 200 must be
    /// transparently retried so `embed()` still succeeds.
    #[tokio::test]
    async fn retries_transient_5xx_then_succeeds() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            for i in 0..3u32 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await; // drain the request
                let (status, body) = if i < 2 {
                    ("503 Service Unavailable", "{}".to_string())
                } else {
                    (
                        "200 OK",
                        r#"{"data":[{"index":0,"embedding":[0.5,0.25,0.125]}]}"#.to_string(),
                    )
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });

        let provider = OpenAiEmbeddingProvider::new("k", "m", 3, &format!("http://{addr}"));
        let out = provider.embed(&["hello".to_string()]).await.unwrap();
        assert_eq!(out, vec![vec![0.5f32, 0.25, 0.125]]);
        server.await.unwrap();
    }
}
