use std::borrow::Cow;
use std::time::Duration;

use base64::Engine;
use thairag_core::error::{Result, ThaiRagError};
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ImageContent, ImageMetadata, VisionMessage};
use tracing::{info, warn};

/// MIME types recognized as images for multi-modal processing.
pub const IMAGE_MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Max retries for a transient vision-call failure (so up to N+1 attempts).
const VISION_MAX_RETRIES: u32 = 2;
/// Base backoff before the first retry; doubles each attempt (800ms, 1600ms).
const VISION_RETRY_BASE_MS: u64 = 800;

/// Whether a vision-call error looks transient and worth retrying. Upstream
/// gateways (e.g. a flaky llama.cpp/vLLM proxy) return 5xx / 429 / connection
/// resets under load — especially with concurrent page OCR. The OpenAI-compatible
/// provider formats these as `OpenAI returned HTTP 502 Bad Gateway: …` or
/// `OpenAI request failed: …`. We do NOT retry 4xx client errors (400/401/422):
/// a malformed request or bad key won't fix itself.
fn vision_error_is_transient(e: &ThaiRagError) -> bool {
    let s = e.to_string().to_lowercase();
    s.contains("http 502")
        || s.contains("http 503")
        || s.contains("http 500")
        || s.contains("http 504")
        || s.contains("http 429")
        || s.contains("bad gateway")
        || s.contains("service unavailable")
        || s.contains("gateway timeout")
        || s.contains("timed out")
        || s.contains("timeout")
        || s.contains("request failed") // reqwest connect/reset/send errors
        || s.contains("connection")
}

/// Run one vision call with bounded retry on transient upstream failures.
/// Surfaces the final error to the caller (unlike [`describe_image_with_prompt`],
/// which degrades to a placeholder) so OCR callers can fall back to extracted
/// text instead of embedding a placeholder string as page content.
pub async fn describe_image_with_prompt_strict(
    llm: &dyn LlmProvider,
    image_bytes: &[u8],
    mime_type: &str,
    prompt: &str,
    max_tokens: u32,
    max_image_edge: u32,
) -> Result<String> {
    let (vision_bytes, vision_mime) = downscale_for_vision(image_bytes, mime_type, max_image_edge);
    let base64_data = base64::engine::general_purpose::STANDARD.encode(vision_bytes.as_ref());

    let mut attempt = 0u32;
    loop {
        let vision_msg = VisionMessage {
            role: "user".to_string(),
            text: prompt.to_string(),
            images: vec![ImageContent {
                base64_data: base64_data.clone(),
                media_type: vision_mime.to_string(),
            }],
        };
        match llm.generate_vision(&[vision_msg], Some(max_tokens)).await {
            Ok(response) => return Ok(response.content),
            Err(e) if attempt < VISION_MAX_RETRIES && vision_error_is_transient(&e) => {
                attempt += 1;
                let backoff = Duration::from_millis(VISION_RETRY_BASE_MS << (attempt - 1));
                warn!(
                    attempt,
                    model = llm.model_name(),
                    error = %e,
                    backoff_ms = backoff.as_millis() as u64,
                    "vision call transient failure — retrying"
                );
                tokio::time::sleep(backoff).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Downscale `bytes` so its longest edge is at most `max_edge` px, preserving
/// aspect ratio and re-encoding as PNG. Returns the original bytes/mime
/// untouched when `max_edge == 0`, when the image already fits, or when
/// decode/encode fails (a bad decode must never block the vision call).
///
/// This is the single chokepoint that bounds vision token cost/RAM for every
/// describe path — embedded document images, direct uploads, and rasterized
/// PDF pages all flow through [`describe_image_with_prompt`].
fn downscale_for_vision<'a>(
    bytes: &'a [u8],
    mime: &'a str,
    max_edge: u32,
) -> (Cow<'a, [u8]>, &'a str) {
    if max_edge == 0 {
        return (Cow::Borrowed(bytes), mime);
    }
    let img = match ::image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(_) => return (Cow::Borrowed(bytes), mime),
    };
    if img.width().max(img.height()) <= max_edge {
        return (Cow::Borrowed(bytes), mime);
    }
    let resized = img.resize(max_edge, max_edge, ::image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    if resized
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            ::image::ImageFormat::Png,
        )
        .is_err()
    {
        return (Cow::Borrowed(bytes), mime);
    }
    (Cow::Owned(png), "image/png")
}

/// Check if a MIME type is an image type we support.
pub fn is_image_mime(mime_type: &str) -> bool {
    IMAGE_MIME_TYPES.contains(&mime_type)
}

/// Extract basic metadata from image bytes (format, size; dimensions are best-effort).
pub fn extract_image_metadata(image_bytes: &[u8], mime_type: &str) -> ImageMetadata {
    let format = match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        other => other,
    }
    .to_string();

    // Try to extract dimensions from file headers (no heavy deps).
    let (width, height) = extract_dimensions(image_bytes, &format);

    ImageMetadata {
        width,
        height,
        format,
        size_bytes: image_bytes.len(),
    }
}

/// Default prompt for a plain image description (direct image uploads).
const DEFAULT_IMAGE_PROMPT: &str = "Describe this image in detail. Include any text, diagrams, charts, \
     tables, or visual elements you can identify. If there is text in the image, \
     transcribe it accurately.";

/// Describe an image using a vision-capable LLM with the default prompt.
/// If the LLM does not support vision, returns a placeholder description.
///
/// `max_image_edge` caps the longest edge (px) of the image actually sent to
/// the model; `0` disables downscaling. See [`describe_image_with_prompt`].
pub async fn describe_image(
    llm: &dyn LlmProvider,
    image_bytes: &[u8],
    mime_type: &str,
    max_image_edge: u32,
) -> Result<String> {
    describe_image_with_prompt(
        llm,
        image_bytes,
        mime_type,
        DEFAULT_IMAGE_PROMPT,
        1024,
        max_image_edge,
    )
    .await
}

/// Describe an image with a caller-supplied prompt and token budget.
///
/// Used by the smart-PDF engine to apply strategy-specific prompts (full-page
/// transcription, table extraction, OCR) instead of the generic description.
/// If the LLM does not support vision, returns a metadata placeholder.
pub async fn describe_image_with_prompt(
    llm: &dyn LlmProvider,
    image_bytes: &[u8],
    mime_type: &str,
    prompt: &str,
    max_tokens: u32,
    max_image_edge: u32,
) -> Result<String> {
    // Advisory, never enforcing: attempt the vision call (with transient-retry)
    // regardless of whether the model id is in our recommended-vision list — the
    // admin's model may be vision-capable even if we don't recognize the name.
    // Fall back to a metadata placeholder only when the call ultimately fails.
    match describe_image_with_prompt_strict(
        llm,
        image_bytes,
        mime_type,
        prompt,
        max_tokens,
        max_image_edge,
    )
    .await
    {
        Ok(content) => {
            info!(
                description_len = content.len(),
                "Image described via vision LLM"
            );
            Ok(content)
        }
        Err(e) => {
            let metadata = extract_image_metadata(image_bytes, mime_type);
            if llm.supports_vision() {
                warn!(model = llm.model_name(), error = %e,
                    "vision call failed — using metadata placeholder");
            } else {
                warn!(
                    model = llm.model_name(),
                    error = %e,
                    "vision call failed and the model is not in the recommended-vision list \
                     — using a metadata placeholder. If this model does support vision, this \
                     is harmless; otherwise configure a vision-capable model."
                );
            }
            Ok(format_placeholder_description(&metadata))
        }
    }
}

/// Format a placeholder description when vision is not available.
pub fn format_placeholder_description(metadata: &ImageMetadata) -> String {
    let dims = match (metadata.width, metadata.height) {
        (Some(w), Some(h)) => format!("{w}x{h}"),
        _ => "unknown".to_string(),
    };
    format!(
        "[Image: format={}, size={}, {} bytes]",
        metadata.format.to_uppercase(),
        dims,
        metadata.size_bytes
    )
}

/// Best-effort dimension extraction from raw bytes (PNG and JPEG headers only).
/// No external image processing library required.
fn extract_dimensions(data: &[u8], format: &str) -> (Option<u32>, Option<u32>) {
    match format {
        "png" => extract_png_dimensions(data),
        "jpeg" => extract_jpeg_dimensions(data),
        "gif" => extract_gif_dimensions(data),
        _ => (None, None),
    }
}

/// PNG: IHDR chunk starts at byte 16, width at 16..20, height at 20..24 (big-endian u32).
fn extract_png_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    if data.len() < 24 {
        return (None, None);
    }
    // Check PNG signature
    if &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return (None, None);
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (Some(width), Some(height))
}

/// GIF: header is "GIF87a" or "GIF89a", width at bytes 6..8 and height at 8..10 (little-endian u16).
fn extract_gif_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    if data.len() < 10 {
        return (None, None);
    }
    if &data[0..6] != b"GIF87a" && &data[0..6] != b"GIF89a" {
        return (None, None);
    }
    let width = u16::from_le_bytes([data[6], data[7]]) as u32;
    let height = u16::from_le_bytes([data[8], data[9]]) as u32;
    (Some(width), Some(height))
}

/// JPEG: scan for SOF0 marker (0xFF 0xC0) and read height/width.
fn extract_jpeg_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return (None, None);
    }
    let mut i = 2;
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // SOF markers: 0xC0-0xC3 (baseline, extended, progressive, lossless)
        if (0xC0..=0xC3).contains(&marker) && i + 9 < data.len() {
            let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return (Some(width), Some(height));
        }
        // Skip marker segment
        if i + 3 < data.len() {
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            i += 2 + len;
        } else {
            break;
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_mime_recognizes_supported_types() {
        assert!(is_image_mime("image/png"));
        assert!(is_image_mime("image/jpeg"));
        assert!(is_image_mime("image/webp"));
        assert!(is_image_mime("image/gif"));
        assert!(!is_image_mime("text/plain"));
        assert!(!is_image_mime("application/pdf"));
    }

    #[test]
    fn extract_metadata_basic() {
        let meta = extract_image_metadata(b"fake image data", "image/png");
        assert_eq!(meta.format, "png");
        assert_eq!(meta.size_bytes, 15);
        assert!(meta.width.is_none()); // not a real PNG
    }

    #[test]
    fn placeholder_description_format() {
        let meta = ImageMetadata {
            width: Some(800),
            height: Some(600),
            format: "png".into(),
            size_bytes: 12345,
        };
        let desc = format_placeholder_description(&meta);
        assert!(desc.contains("800x600"));
        assert!(desc.contains("PNG"));
        assert!(desc.contains("12345"));
    }

    #[test]
    fn placeholder_description_unknown_dims() {
        let meta = ImageMetadata {
            width: None,
            height: None,
            format: "gif".into(),
            size_bytes: 500,
        };
        let desc = format_placeholder_description(&meta);
        assert!(desc.contains("GIF"));
        assert!(desc.contains("unknown"));
        assert!(desc.contains("500"));
    }

    #[test]
    fn gif_dimensions_from_valid_header() {
        // Minimal GIF89a header
        let mut data = b"GIF89a".to_vec();
        // Width: 320 (little-endian)
        data.extend_from_slice(&320u16.to_le_bytes());
        // Height: 240 (little-endian)
        data.extend_from_slice(&240u16.to_le_bytes());

        let (w, h) = extract_gif_dimensions(&data);
        assert_eq!(w, Some(320));
        assert_eq!(h, Some(240));
    }

    #[test]
    fn gif_dimensions_invalid_signature() {
        let data = b"NOTGIF1234567890".to_vec();
        let (w, h) = extract_gif_dimensions(&data);
        assert!(w.is_none());
        assert!(h.is_none());
    }

    /// Encode a solid-color RGB image of the given size as PNG bytes.
    fn png_of_size(w: u32, h: u32) -> Vec<u8> {
        let img = ::image::DynamicImage::ImageRgb8(::image::RgbImage::new(w, h));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            ::image::ImageFormat::Png,
        )
        .unwrap();
        buf
    }

    #[test]
    fn downscale_disabled_returns_original() {
        let bytes = png_of_size(4000, 3000);
        let (out, mime) = downscale_for_vision(&bytes, "image/png", 0);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out.as_ref(), bytes.as_slice());
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn downscale_passthrough_when_already_small() {
        let bytes = png_of_size(800, 600);
        let (out, _) = downscale_for_vision(&bytes, "image/png", 2048);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out.as_ref(), bytes.as_slice());
    }

    #[test]
    fn downscale_shrinks_oversized_image_preserving_aspect() {
        let bytes = png_of_size(4000, 3000);
        let (out, mime) = downscale_for_vision(&bytes, "image/png", 2048);
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(mime, "image/png");
        let decoded = ::image::load_from_memory(out.as_ref()).unwrap();
        assert_eq!(decoded.width().max(decoded.height()), 2048);
        // 4000x3000 (4:3) scaled to long edge 2048 → 2048x1536.
        assert_eq!(decoded.width(), 2048);
        assert_eq!(decoded.height(), 1536);
    }

    #[test]
    fn downscale_bad_bytes_fall_through_untouched() {
        let bytes = b"not an image".to_vec();
        let (out, mime) = downscale_for_vision(&bytes, "image/png", 2048);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out.as_ref(), bytes.as_slice());
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn png_dimensions_from_valid_header() {
        // Minimal PNG header: signature + IHDR
        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        // IHDR chunk length (13 bytes)
        data.extend_from_slice(&[0, 0, 0, 13]);
        // IHDR type
        data.extend_from_slice(b"IHDR");
        // Width: 1920 = 0x00000780
        data.extend_from_slice(&1920u32.to_be_bytes());
        // Height: 1080 = 0x00000438
        data.extend_from_slice(&1080u32.to_be_bytes());
        // Rest of IHDR (bit depth, color type, etc.)
        data.extend_from_slice(&[8, 6, 0, 0, 0]);

        let (w, h) = extract_png_dimensions(&data);
        assert_eq!(w, Some(1920));
        assert_eq!(h, Some(1080));
    }

    // ── transient-retry behavior for vision calls ──

    use std::sync::atomic::{AtomicUsize, Ordering};
    use thairag_core::traits::LlmProvider;
    use thairag_core::types::{ChatMessage, LlmResponse, VisionMessage};

    /// Vision stub that fails its first `fail_n` calls with `err`, then succeeds.
    struct FlakyVision {
        calls: AtomicUsize,
        fail_n: usize,
        err: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for FlakyVision {
        fn model_name(&self) -> &str {
            "flaky-vision"
        }
        fn supports_vision(&self) -> bool {
            true
        }
        async fn generate(
            &self,
            _m: &[ChatMessage],
            _t: Option<u32>,
        ) -> thairag_core::error::Result<LlmResponse> {
            unreachable!()
        }
        async fn generate_vision(
            &self,
            _m: &[VisionMessage],
            _t: Option<u32>,
        ) -> thairag_core::error::Result<LlmResponse> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_n {
                Err(ThaiRagError::LlmProvider(self.err.clone()))
            } else {
                Ok(LlmResponse {
                    content: "OCR-OK".into(),
                    usage: Default::default(),
                })
            }
        }
    }

    #[test]
    fn transient_502_is_classified_retryable() {
        let e = ThaiRagError::LlmProvider("OpenAI returned HTTP 502 Bad Gateway: x".into());
        assert!(vision_error_is_transient(&e));
    }

    #[test]
    fn client_error_400_is_not_retryable() {
        let e = ThaiRagError::LlmProvider("OpenAI returned HTTP 400 Bad Request: x".into());
        assert!(!vision_error_is_transient(&e));
    }

    #[tokio::test]
    async fn strict_retries_transient_502_then_succeeds() {
        let llm = FlakyVision {
            calls: AtomicUsize::new(0),
            fail_n: 1, // 502 once, then succeed
            err: "OpenAI returned HTTP 502 Bad Gateway".into(),
        };
        let out = describe_image_with_prompt_strict(&llm, b"img", "image/png", "ocr", 64, 0).await;
        assert_eq!(out.unwrap(), "OCR-OK");
        assert_eq!(llm.calls.load(Ordering::SeqCst), 2); // 1 fail + 1 success
    }

    #[tokio::test]
    async fn strict_does_not_retry_client_error() {
        let llm = FlakyVision {
            calls: AtomicUsize::new(0),
            fail_n: 99,
            err: "OpenAI returned HTTP 400 Bad Request".into(),
        };
        let out = describe_image_with_prompt_strict(&llm, b"img", "image/png", "ocr", 64, 0).await;
        assert!(out.is_err());
        assert_eq!(llm.calls.load(Ordering::SeqCst), 1); // no retry on 4xx
    }

    #[tokio::test]
    async fn lenient_wrapper_returns_placeholder_after_retries_exhausted() {
        let llm = FlakyVision {
            calls: AtomicUsize::new(0),
            fail_n: 99,
            err: "OpenAI returned HTTP 503 Service Unavailable".into(),
        };
        let out = describe_image_with_prompt(&llm, b"img", "image/png", "ocr", 64, 0)
            .await
            .unwrap();
        // Degrades to a placeholder; tried initial + VISION_MAX_RETRIES times.
        assert!(out.contains("Image"));
        assert_eq!(
            llm.calls.load(Ordering::SeqCst),
            (VISION_MAX_RETRIES + 1) as usize
        );
    }
}
