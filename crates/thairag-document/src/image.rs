use base64::Engine;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ImageContent, ImageMetadata, VisionMessage};
use tracing::info;

/// MIME types recognized as images for multi-modal processing.
pub const IMAGE_MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

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

/// Describe an image using a vision-capable LLM.
/// If the LLM does not support vision, returns a placeholder description with metadata.
pub async fn describe_image(
    llm: &dyn LlmProvider,
    image_bytes: &[u8],
    mime_type: &str,
) -> Result<String> {
    let metadata = extract_image_metadata(image_bytes, mime_type);

    if !llm.supports_vision() {
        info!(
            format = %metadata.format,
            size = metadata.size_bytes,
            "LLM does not support vision; returning metadata-only description"
        );
        return Ok(format_placeholder_description(&metadata));
    }

    let base64_data = base64::engine::general_purpose::STANDARD.encode(image_bytes);

    let vision_msg = VisionMessage {
        role: "user".to_string(),
        text: "Describe this image in detail. Include any text, diagrams, charts, \
               tables, or visual elements you can identify. If there is text in the image, \
               transcribe it accurately."
            .to_string(),
        images: vec![ImageContent {
            base64_data,
            media_type: mime_type.to_string(),
        }],
    };

    let response = llm.generate_vision(&[vision_msg], Some(1024)).await?;

    info!(
        format = %metadata.format,
        size = metadata.size_bytes,
        description_len = response.content.len(),
        "Image described via vision LLM"
    );

    Ok(response.content)
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
}
