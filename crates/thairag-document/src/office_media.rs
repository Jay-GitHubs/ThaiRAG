//! Extract embedded raster images from non-PDF documents (DOCX/XLSX/HTML) so
//! the pipeline can describe and persist them like PDF images.
//!
//! DOCX/XLSX are zip containers with images under a `*/media/` directory; HTML
//! carries images inline as `data:` URLs. Remote `<img>` URLs are deliberately
//! NOT fetched (avoids SSRF and network in the ingest path).

use std::io::Read;

/// A raw embedded image with its detected MIME type.
#[derive(Debug, Clone)]
pub struct RawImage {
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Recognised raster extensions → MIME.
const EXT_MIME: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

fn mime_for_name(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    EXT_MIME
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, mime)| *mime)
}

/// Extract images from a zip-based Office file (any `media/` directory).
/// Used for both DOCX (`word/media/`) and XLSX (`xl/media/`).
pub fn extract_office_images(bytes: &[u8]) -> Vec<RawImage> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    let mut images = Vec::new();
    for i in 0..archive.len() {
        let Ok(mut file) = archive.by_index(i) else {
            continue;
        };
        let name = file.name().to_string();
        if !name.contains("media/") {
            continue;
        }
        let Some(mime) = mime_for_name(&name) else {
            continue;
        };
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
            images.push(RawImage {
                bytes: buf,
                mime: mime.to_string(),
            });
        }
    }
    images
}

/// Extract images from HTML `<img src="data:image/...;base64,...">`.
/// Remote URLs are skipped.
pub fn extract_html_images(html: &[u8]) -> Vec<RawImage> {
    use base64::Engine;
    let text = String::from_utf8_lossy(html);
    let doc = scraper::Html::parse_document(&text);
    let selector = scraper::Selector::parse("img[src]").expect("static selector is valid");

    let mut images = Vec::new();
    for el in doc.select(&selector) {
        let Some(src) = el.value().attr("src") else {
            continue;
        };
        let Some(rest) = src.strip_prefix("data:") else {
            continue; // remote URL — not fetched
        };
        // data:image/png;base64,<DATA>
        let Some((meta, b64)) = rest.split_once(',') else {
            continue;
        };
        if !meta.contains("base64") {
            continue;
        }
        let mime = meta.split(';').next().unwrap_or("image/png");
        if !mime.starts_with("image/") {
            continue;
        }
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim())
            && !bytes.is_empty()
        {
            images.push(RawImage {
                bytes,
                mime: mime.to_string(),
            });
        }
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn mime_for_name_maps_extensions() {
        assert_eq!(mime_for_name("word/media/image1.png"), Some("image/png"));
        assert_eq!(mime_for_name("xl/media/image2.JPEG"), Some("image/jpeg"));
        assert_eq!(mime_for_name("word/media/notes.txt"), None);
    }

    #[test]
    fn non_zip_bytes_yield_no_images() {
        assert!(extract_office_images(b"not a zip").is_empty());
    }

    #[test]
    fn html_data_url_image_is_extracted() {
        let png = b"\x89PNG\r\n\x1a\nfake";
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        let html = format!(
            "<html><body><img src=\"data:image/png;base64,{b64}\"><img src=\"https://x/y.png\"></body></html>"
        );
        let imgs = extract_html_images(html.as_bytes());
        assert_eq!(imgs.len(), 1, "only the data: URL should be extracted");
        assert_eq!(imgs[0].mime, "image/png");
        assert_eq!(imgs[0].bytes, png);
    }
}
