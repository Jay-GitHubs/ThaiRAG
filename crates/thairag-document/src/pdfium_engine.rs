//! Native PDF page extraction via pdfium (Chromium's PDF engine), wrapping the
//! `pdfium-render` crate. Mirrors `Jay-RAG-Tools/crates/core/src/pdf.rs`.
//!
//! pdfium's handles are `!Send`, so every method here is synchronous and must
//! be driven from a `tokio::task::spawn_blocking` task — never hold a
//! [`PdfEngine`], document, or page across an `.await`.
//!
//! The native `libpdfium` is provisioned by `build.rs` (downloaded from
//! bblanchon/pdfium-binaries) for local builds and baked onto the system
//! library path in Docker. When it cannot be loaded, [`is_available`] returns
//! `false` and the pipeline falls back to the legacy `pdf-extract` path.

use image::DynamicImage;
use pdfium_render::prelude::*;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;

/// Cheap per-page signals used to pick an extraction strategy.
#[derive(Debug, Clone)]
pub struct PageSignals {
    /// 0-indexed page number.
    pub index: usize,
    /// Fraction of page area covered by image objects (0.0..=1.0).
    pub coverage: f64,
    /// Extracted text, trimmed.
    pub text: String,
}

/// A PNG-encoded image extracted (or rendered) from a page.
#[derive(Debug, Clone)]
pub struct ExtractedImage {
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 1-indexed position on the page; `0` for a full-page render.
    pub index: u32,
}

/// A single positioned glyph: the character plus its bounding box, in PDF
/// point space (origin bottom-left, y increases upward). The exact, never-OCR'd
/// raw material for deterministic table reconstruction.
#[derive(Debug, Clone)]
pub struct PositionedChar {
    pub ch: char,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// An axis-aligned ruling-line segment (table border) in PDF point space.
#[derive(Debug, Clone)]
pub struct RuleLine {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Per-page geometry: glyph positions + ruling lines + page size. This is the
/// deterministic input to lattice table reconstruction — no model, no OCR, so
/// cell content (numbers) is exact and cannot be fabricated.
#[derive(Debug, Clone)]
pub struct PageGeometry {
    pub width: f32,
    pub height: f32,
    pub chars: Vec<PositionedChar>,
    pub lines: Vec<RuleLine>,
}

fn err(msg: impl Into<String>) -> ThaiRagError {
    ThaiRagError::Validation(msg.into())
}

/// Bind to libpdfium: build-time path → system library → current directory.
fn bind() -> Result<Pdfium> {
    // 1. Path baked in by build.rs (downloaded binary) — best for local dev.
    if let Some(path) = option_env!("PDFIUM_DYLIB_PATH")
        && let Ok(bindings) = Pdfium::bind_to_library(path)
    {
        return Ok(Pdfium::new(bindings));
    }
    // 2. System library (Docker bakes libpdfium into /usr/lib).
    if let Ok(bindings) = Pdfium::bind_to_system_library() {
        return Ok(Pdfium::new(bindings));
    }
    // 3. Current working directory.
    if let Ok(bindings) = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("."))
    {
        return Ok(Pdfium::new(bindings));
    }
    Err(err(
        "libpdfium not found — install it on the system library path or set \
         PDFIUM_DYLIB_PATH (see OPERATOR_GUIDE). Smart-PDF extraction is disabled.",
    ))
}

/// `true` when libpdfium can be loaded. Gate the smart-PDF path on this.
pub fn is_available() -> bool {
    bind().is_ok()
}

/// One-shot per-page text extraction via pdfium, loading and dropping the
/// engine in a single call. 1-indexed page numbers; includes pages with no
/// text (empty string) so callers can align page indices. Errors when
/// libpdfium is unavailable or the PDF cannot be parsed — callers fall back to
/// the `pdf-extract` path.
///
/// pdfium decodes `ToUnicode`/CID font mappings far more faithfully than
/// `pdf-extract`, which is why it is the preferred text path for Thai (whose
/// subsetted fonts and tone/vowel marks `pdf-extract` frequently mangles).
pub fn extract_text_by_pages(pdf: &[u8]) -> Result<Vec<(usize, String)>> {
    PdfEngine::new()?.text_by_pages(pdf)
}

/// Sharpen + contrast boost to help Thai OCR (mirrors Jay-RAG-Tools).
fn enhance(img: DynamicImage) -> DynamicImage {
    img.adjust_contrast(20.0).unsharpen(1.5, 3)
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| err(format!("PNG encode failed: {e}")))?;
    Ok(png)
}

/// A loaded pdfium binding. Construct once inside a `spawn_blocking` task and
/// reuse for every page of a document.
pub struct PdfEngine {
    pdfium: Pdfium,
}

impl PdfEngine {
    /// Load libpdfium. Errors if the native library can't be found.
    pub fn new() -> Result<Self> {
        Ok(Self { pdfium: bind()? })
    }

    fn load<'a>(&'a self, pdf: &'a [u8]) -> Result<PdfDocument<'a>> {
        // pdfium loads the slice by reference (zero-copy), so `pdf` must
        // outlive the returned document — hence the shared `'a`.
        self.pdfium
            .load_pdf_from_byte_slice(pdf, None)
            .map_err(|e| err(format!("failed to open PDF: {e}")))
    }

    /// Number of pages in the document.
    pub fn page_count(&self, pdf: &[u8]) -> Result<usize> {
        Ok(self.load(pdf)?.pages().len() as usize)
    }

    /// Per-page text for every page, in one load. 1-indexed; includes empty
    /// pages so the caller can align page indices for vision fallback.
    pub fn text_by_pages(&self, pdf: &[u8]) -> Result<Vec<(usize, String)>> {
        let doc = self.load(pdf)?;
        Ok(doc
            .pages()
            .iter()
            .enumerate()
            .map(|(i, page)| (i + 1, page_text(&page)))
            .collect())
    }

    /// Cheap signals (image coverage + text) for every page, in one load.
    pub fn page_signals(&self, pdf: &[u8]) -> Result<Vec<PageSignals>> {
        let doc = self.load(pdf)?;
        let mut out = Vec::new();
        for (i, page) in doc.pages().iter().enumerate() {
            out.push(PageSignals {
                index: i,
                coverage: image_coverage(&page),
                text: page_text(&page),
            });
        }
        Ok(out)
    }

    /// Render one page to a PNG at `dpi`. `sharpen` applies OCR enhancement.
    pub fn render_page_png(
        &self,
        pdf: &[u8],
        page_index: usize,
        dpi: u32,
        sharpen: bool,
    ) -> Result<ExtractedImage> {
        let doc = self.load(pdf)?;
        let page = doc
            .pages()
            .get(page_index as u16)
            .map_err(|e| err(format!("get page {page_index}: {e}")))?;

        let scale = dpi as f32 / 72.0;
        let width = (page.width().value * scale) as i32;
        let height = (page.height().value * scale) as i32;
        let config = PdfRenderConfig::new()
            .set_target_width(width)
            .set_target_height(height);

        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| err(format!("render page {page_index}: {e}")))?;
        let mut img = bitmap.as_image();
        if sharpen {
            img = enhance(img);
        }
        let (width, height) = (img.width(), img.height());
        Ok(ExtractedImage {
            png_bytes: encode_png(&img)?,
            width,
            height,
            index: 0,
        })
    }

    /// Extract embedded raster images from one page, skipping tiny ones
    /// (smaller than `min_size` px on either axis).
    pub fn embedded_images(
        &self,
        pdf: &[u8],
        page_index: usize,
        min_size: u32,
        sharpen: bool,
    ) -> Result<Vec<ExtractedImage>> {
        let doc = self.load(pdf)?;
        let page = doc
            .pages()
            .get(page_index as u16)
            .map_err(|e| err(format!("get page {page_index}: {e}")))?;

        let mut images = Vec::new();
        let mut idx = 0u32;
        for object in page.objects().iter() {
            if object.object_type() != PdfPageObjectType::Image {
                continue;
            }
            let Some(image_object) = object.as_image_object() else {
                continue;
            };
            let mut raw = match image_object.get_raw_image() {
                Ok(img) => img,
                Err(_) => continue,
            };
            let (w, h) = (raw.width(), raw.height());
            if w < min_size || h < min_size {
                continue;
            }
            idx += 1;
            if sharpen {
                raw = enhance(raw);
            }
            let Ok(png) = encode_png(&raw) else {
                continue;
            };
            images.push(ExtractedImage {
                png_bytes: png,
                width: w,
                height: h,
                index: idx,
            });
        }
        Ok(images)
    }

    /// Extract deterministic geometry for one page: every glyph with its
    /// bounding box (from the text layer — exact, never OCR'd) plus axis-aligned
    /// ruling-line segments (table borders) from path objects. This is the raw
    /// input to lattice table reconstruction. Diagonal/curved segments are
    /// ignored; near-horizontal and near-vertical edges are kept.
    pub fn page_geometry(&self, pdf: &[u8], page_index: usize) -> Result<PageGeometry> {
        let doc = self.load(pdf)?;
        let page = doc
            .pages()
            .get(page_index as u16)
            .map_err(|e| err(format!("get page {page_index}: {e}")))?;
        let width = page.width().value;
        let height = page.height().value;

        // Glyph positions from the text layer (the exact characters; no OCR).
        let mut chars = Vec::new();
        if let Ok(text) = page.text() {
            for c in text.chars().iter() {
                let Some(ch) = c.unicode_char() else { continue };
                if ch.is_control() {
                    continue;
                }
                if let Ok(b) = c.tight_bounds() {
                    chars.push(PositionedChar {
                        ch,
                        x0: b.left().value,
                        y0: b.bottom().value,
                        x1: b.right().value,
                        y1: b.top().value,
                    });
                }
            }
        }

        // Ruling lines: walk each path object's segments and emit only the
        // axis-aligned edges (horizontal or vertical). A segment within
        // `AXIS_TOL` points of horizontal/vertical is treated as a rule line;
        // a closing segment draws the edge back to the subpath start (captures
        // the 4th side of rectangle borders).
        const AXIS_TOL: f32 = 1.5;
        let mut lines = Vec::new();
        let mut push_axis = |x0: f32, y0: f32, x1: f32, y1: f32| {
            let dx = (x1 - x0).abs();
            let dy = (y1 - y0).abs();
            if (dy <= AXIS_TOL && dx > AXIS_TOL) || (dx <= AXIS_TOL && dy > AXIS_TOL) {
                lines.push(RuleLine { x0, y0, x1, y1 });
            }
        };
        for object in page.objects().iter() {
            if object.object_type() != PdfPageObjectType::Path {
                continue;
            }
            let Some(path) = object.as_path_object() else {
                continue;
            };
            let mut cur: Option<(f32, f32)> = None;
            let mut start: Option<(f32, f32)> = None;
            for seg in path.segments().iter() {
                let (x, y) = (seg.x().value, seg.y().value);
                match seg.segment_type() {
                    PdfPathSegmentType::MoveTo => {
                        cur = Some((x, y));
                        start = Some((x, y));
                    }
                    PdfPathSegmentType::LineTo => {
                        if let Some((px, py)) = cur {
                            push_axis(px, py, x, y);
                        }
                        cur = Some((x, y));
                    }
                    _ => cur = Some((x, y)),
                }
                if seg.is_close()
                    && let (Some((px, py)), Some((sx, sy))) = (cur, start)
                {
                    push_axis(px, py, sx, sy);
                }
            }
        }

        Ok(PageGeometry {
            width,
            height,
            chars,
            lines,
        })
    }
}

/// Fraction of a page's area covered by image objects (0.0..=1.0).
fn image_coverage(page: &PdfPage) -> f64 {
    let area = page.width().value as f64 * page.height().value as f64;
    if area == 0.0 {
        return 0.0;
    }
    let mut image_area = 0.0;
    for object in page.objects().iter() {
        if object.object_type() == PdfPageObjectType::Image
            && let Ok(b) = object.bounds()
        {
            let w = (b.right().value - b.left().value).abs() as f64;
            let h = (b.top().value - b.bottom().value).abs() as f64;
            image_area += w * h;
        }
    }
    (image_area / area).min(1.0)
}

fn page_text(page: &PdfPage) -> String {
    page.text()
        .map(|t| t.all())
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_available_does_not_panic() {
        // Result depends on whether libpdfium was provisioned; either is fine.
        let _ = is_available();
    }

    #[test]
    fn garbage_bytes_error_cleanly_when_available() {
        if !is_available() {
            return; // libpdfium not provisioned in this environment — skip.
        }
        let engine = PdfEngine::new().expect("bind pdfium");
        let err = engine.page_count(b"not a pdf at all").unwrap_err();
        assert!(format!("{err}").contains("open PDF"), "unexpected: {err}");
    }
}
