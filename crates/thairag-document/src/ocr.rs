//! Deterministic OCR provider (fidelity tier-2) — see
//! `docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md` and `docs/OCR_VS_VLM_SPIKE.md`.
//!
//! Transcribes a rendered page image to text *deterministically* — it reads
//! pixels, it does not hallucinate or describe. Used for OCR-needing pages
//! (scanned / corrupted-CMap text) where a vision LLM is both worse on Thai
//! (94.5% vs 90.1% measured) and prone to fabrication/repetition. The vision LLM
//! stays for *figure description*, which OCR can't do.
//!
//! The only implementation is an HTTP client for the PaddleOCR sidecar
//! (`services/paddleocr-sidecar`). It is wired into the pipeline **default-off**:
//! with no provider configured, extraction is byte-for-byte unchanged.

use std::time::Duration;

use thairag_core::error::{Result, ThaiRagError};

/// A deterministic OCR engine: page image → transcribed text in reading order.
#[async_trait::async_trait]
pub trait OcrProvider: Send + Sync {
    /// Transcribe a PNG-encoded page render. Returns the text, or an error the
    /// caller can fall back from (to the vision LLM, or to the extracted text).
    async fn ocr(&self, png: &[u8]) -> Result<String>;

    /// Provider name for telemetry.
    fn name(&self) -> &str;
}

/// HTTP client for the PaddleOCR Thai sidecar service.
pub struct SidecarOcrProvider {
    /// Base URL, e.g. `http://paddleocr:8086` (no trailing slash).
    base_url: String,
    timeout: Duration,
}

impl SidecarOcrProvider {
    /// Build a provider pointed at the sidecar base URL. Returns `None` for an
    /// empty URL so callers can treat "unconfigured" as "OCR tier off".
    pub fn new(base_url: &str) -> Option<Self> {
        let url = base_url.trim().trim_end_matches('/');
        if url.is_empty() {
            return None;
        }
        Some(Self {
            base_url: url.to_string(),
            timeout: Duration::from_secs(60),
        })
    }
}

#[async_trait::async_trait]
impl OcrProvider for SidecarOcrProvider {
    async fn ocr(&self, png: &[u8]) -> Result<String> {
        let url = format!("{}/ocr", self.base_url);
        let body = png.to_vec();
        let timeout = self.timeout;
        // ureq is blocking — run it off the async runtime.
        tokio::task::spawn_blocking(move || {
            let text = ureq::post(&url)
                .timeout(timeout)
                .set("Content-Type", "image/png")
                .send_bytes(&body)
                .map_err(|e| ThaiRagError::Internal(format!("OCR sidecar request failed: {e}")))?
                .into_string()
                .map_err(|e| ThaiRagError::Internal(format!("OCR sidecar read failed: {e}")))?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| ThaiRagError::Internal(format!("OCR sidecar bad JSON: {e}")))?;
            Ok(json
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string())
        })
        .await
        .map_err(|e| ThaiRagError::Internal(format!("OCR task join: {e}")))?
    }

    fn name(&self) -> &str {
        "paddleocr-sidecar"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_url_is_none() {
        assert!(SidecarOcrProvider::new("").is_none());
        assert!(SidecarOcrProvider::new("   ").is_none());
    }

    #[test]
    fn trailing_slash_trimmed() {
        let p = SidecarOcrProvider::new("http://paddleocr:8086/").unwrap();
        assert_eq!(p.base_url, "http://paddleocr:8086");
        assert_eq!(p.name(), "paddleocr-sidecar");
    }
}
