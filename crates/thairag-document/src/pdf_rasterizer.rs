//! Rasterize PDF pages to PNG via `pdftoppm` (poppler-utils).
//!
//! Subprocess isolation is intentional: a malformed/malicious PDF that
//! crashes the renderer will only kill the child process, not the API.
//! The PDF is staged in a private temp file (poppler needs a *seekable*
//! input — it cannot parse a PDF from a pipe) and no input is interpolated
//! into shell arguments. Every invocation has a hard timeout plus a
//! virtual-memory cap (Linux only).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use tracing::{debug, warn};

/// Hard cap on output PNG size per page. Anything larger is rejected to
/// prevent a malicious PDF from producing a multi-GB image that exhausts
/// host memory while we read it.
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024; // 32 MiB

/// Default virtual-memory limit applied to the `pdftoppm` child process (KiB).
///
/// Note this is **virtual address space**, not RSS. Modern poppler links
/// libcairo + libfontconfig + libfreetype + libpng + a stack of small libs;
/// even an idle pdftoppm process commonly maps 500MB-2GB of virtual address
/// space just from loading shared libraries — well before any user PDF is
/// processed. If `--as` is set too tight, pdftoppm crashes on startup and
/// the render fails (non-zero exit / no PNG produced) — captured from the
/// child's stderr rather than as a stdin broken pipe.
///
/// 4 GiB is a safe headroom for any real slide deck (typical RSS at 150 DPI
/// is 50-200 MB) while still bounding pathological inputs. Operators can
/// override via `THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB` env var or disable
/// the limit entirely with `THAIRAG__PDF_RASTERIZER__DISABLE_PRLIMIT=1`.
const DEFAULT_CHILD_VMEM_LIMIT_KB: u64 = 4 * 1024 * 1024;

/// Read the configured virtual-memory limit from env, falling back to
/// [`DEFAULT_CHILD_VMEM_LIMIT_KB`]. Parse failures are silently treated
/// as "use the default" — never block ingestion on a config typo.
fn vmem_limit_kb() -> u64 {
    std::env::var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_CHILD_VMEM_LIMIT_KB)
}

/// The virtual-memory limit as a **byte** count, for `prlimit --as`.
///
/// `prlimit --as` takes its value in bytes (the native unit of `RLIMIT_AS`),
/// unlike `ulimit -v` which is in KiB. Passing the KiB number straight to
/// `--as` was a 1024× under-cap: the intended 4 GiB became 4 MiB, far too
/// small for the dynamic linker to even `mmap` poppler's shared libraries
/// (the symptom was `pdftoppm: error while loading shared libraries:
/// liblcms2.so.2: failed to map segment from shared object`).
fn vmem_limit_bytes() -> u64 {
    vmem_limit_kb().saturating_mul(1024)
}

/// `true` when the operator has explicitly disabled the prlimit wrapper.
/// Useful when running under container memory cgroups (which provide their
/// own enforcement) or when even 4 GiB virtual address space is too tight
/// for an unusual environment.
fn prlimit_disabled() -> bool {
    matches!(
        std::env::var("THAIRAG__PDF_RASTERIZER__DISABLE_PRLIMIT").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// Configuration for one rasterization call.
#[derive(Debug, Clone)]
pub struct RasterizeOptions {
    /// 1-indexed page number to render.
    pub page: usize,
    /// Resolution in DPI. 150 is a good balance for vision LLM input.
    pub dpi: u32,
    /// Hard timeout for the subprocess.
    pub timeout: Duration,
}

impl Default for RasterizeOptions {
    fn default() -> Self {
        Self {
            page: 1,
            dpi: 150,
            timeout: Duration::from_secs(15),
        }
    }
}

/// Rasterize a single PDF page to PNG bytes.
///
/// The PDF is written to a private temp file and its path handed to
/// `pdftoppm` (poppler cannot read a PDF from a non-seekable pipe). The PNG
/// is written to a sibling temp file and read back. Both are removed when
/// the `RasterTemp` guard drops. No user input is interpolated into shell
/// arguments — only fixed flags, integer values, and the temp path.
pub fn rasterize_page(pdf_bytes: &[u8], opts: &RasterizeOptions) -> Result<Vec<u8>> {
    if opts.page == 0 {
        return Err(ThaiRagError::Validation(
            "rasterize_page: page must be >= 1".into(),
        ));
    }

    // `prlimit` is used to cap virtual memory of the child on Linux. On
    // platforms where it's missing (macOS dev boxes) we fall back to a
    // plain Command — the timeout still applies. Operators can also
    // disable it explicitly when running under cgroups.
    let use_prlimit = cfg!(target_os = "linux") && which_exists("prlimit") && !prlimit_disabled();

    let mut cmd = if use_prlimit {
        let mut c = Command::new("prlimit");
        // `--as` is a byte count, not KiB — see `vmem_limit_bytes`.
        c.arg(format!("--as={}", vmem_limit_bytes()));
        c.arg("--");
        c.arg("pdftoppm");
        c
    } else {
        Command::new("pdftoppm")
    };

    // poppler parses a PDF back-to-front (the xref table lives at the end),
    // so it needs a *seekable* input — it cannot read a PDF from a pipe.
    // Stage the bytes in a private temp file and hand pdftoppm the path; the
    // rendered PNG is written to a sibling temp file. Both are cleaned up
    // when `temp` drops.
    let temp = RasterTemp::new(pdf_bytes)?;

    cmd.arg("-png")
        .arg("-r")
        .arg(opts.dpi.to_string())
        .arg("-f")
        .arg(opts.page.to_string())
        .arg("-l")
        .arg(opts.page.to_string())
        .arg("-singlefile")
        .arg(&temp.input)
        .arg(&temp.base)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        ThaiRagError::Validation(format!(
            "pdftoppm not available — install poppler-utils ({e})"
        ))
    })?;

    // Poll for completion with a hard deadline. If exceeded, kill the child.
    let deadline = Instant::now() + opts.timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    warn!(
                        page = opts.page,
                        timeout_ms = opts.timeout.as_millis(),
                        "pdftoppm timed out — killed"
                    );
                    return Err(ThaiRagError::Validation(format!(
                        "pdftoppm timed out after {}ms on page {}",
                        opts.timeout.as_millis(),
                        opts.page
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(ThaiRagError::Validation(format!(
                    "pdftoppm wait failed: {e}"
                )));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| ThaiRagError::Validation(format!("pdftoppm wait_with_output failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ThaiRagError::Validation(format!(
            "pdftoppm failed (page {}): {}",
            opts.page,
            stderr.trim()
        )));
    }

    // pdftoppm wrote the page to `<base>.png`. Check the on-disk size before
    // reading so a pathological PDF can't make us slurp a multi-GB image
    // into memory.
    let png_path = temp.output_png();
    let size = std::fs::metadata(&png_path).map(|m| m.len()).map_err(|e| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        ThaiRagError::Validation(format!(
            "pdftoppm reported success but produced no PNG for page {} ({e}){}",
            opts.page,
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        ))
    })?;

    if size as usize > MAX_PNG_BYTES {
        return Err(ThaiRagError::Validation(format!(
            "rasterized page exceeds {} bytes — possible abusive PDF",
            MAX_PNG_BYTES
        )));
    }

    let png = std::fs::read(&png_path)
        .map_err(|e| ThaiRagError::Validation(format!("failed to read rasterized PNG: {e}")))?;

    if !is_png(&png) {
        return Err(ThaiRagError::Validation(
            "pdftoppm produced output that is not a PNG".into(),
        ));
    }

    debug!(
        page = opts.page,
        dpi = opts.dpi,
        png_bytes = png.len(),
        "rasterized PDF page"
    );

    Ok(png)
}

/// Return how many pages the PDF reports via `pdfinfo`, or `None` if the
/// tool isn't installed or the output can't be parsed.
pub fn page_count(pdf_bytes: &[u8]) -> Option<usize> {
    // Same constraint as rasterize_page: pdfinfo needs a seekable file, not a
    // pipe. Stage the PDF in a temp file and pass the path.
    let temp = RasterTemp::new(pdf_bytes).ok()?;
    let output = Command::new("pdfinfo")
        .arg(&temp.input)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            return rest.trim().parse::<usize>().ok();
        }
    }
    None
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n"
}

/// A private pair of temp files for one rasterization: `<base>.pdf` holds the
/// input PDF we feed to `pdftoppm`, and `<base>.png` is where pdftoppm writes
/// the rendered page (poppler appends `.png` to the output root we pass it).
/// Both are removed when the guard drops, even on the error paths.
struct RasterTemp {
    /// Output root handed to pdftoppm (`<tmp>/thairag-raster-<pid>-<n>`).
    base: PathBuf,
    /// Input PDF path (`<base>.pdf`).
    input: PathBuf,
}

impl RasterTemp {
    fn new(pdf_bytes: &[u8]) -> Result<Self> {
        // Unique within this process: pid disambiguates across processes, the
        // atomic counter across concurrent renders. No randomness needed.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("thairag-raster-{}-{n}", std::process::id()));
        let input = base.with_extension("pdf");

        // `create_new` fails if the path already exists, defeating symlink /
        // pre-creation races in the shared temp dir.
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&input)
            .map_err(|e| {
                ThaiRagError::Validation(format!("failed to create temp PDF for pdftoppm: {e}"))
            })?;
        f.write_all(pdf_bytes).map_err(|e| {
            ThaiRagError::Validation(format!("failed to write temp PDF for pdftoppm: {e}"))
        })?;

        Ok(Self { base, input })
    }

    /// Path pdftoppm writes for `-singlefile` given `base` as the output root.
    fn output_png(&self) -> PathBuf {
        self.base.with_extension("png")
    }
}

impl Drop for RasterTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.input);
        let _ = std::fs::remove_file(self.output_png());
    }
}

fn which_exists(binary: &str) -> bool {
    Command::new("which")
        .arg(binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Returns true if `pdftoppm` is available on PATH. Used at startup or by
/// tests to skip rasterization paths gracefully on systems without it.
pub fn is_available() -> bool {
    which_exists("pdftoppm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_page_zero() {
        let opts = RasterizeOptions {
            page: 0,
            ..Default::default()
        };
        let err = rasterize_page(b"%PDF-1.4", &opts).unwrap_err();
        assert!(format!("{err}").contains("page must be >= 1"));
    }

    #[test]
    fn is_png_signature() {
        assert!(is_png(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_png(b"not a png"));
        assert!(!is_png(b""));
    }

    #[test]
    fn missing_pdftoppm_returns_clean_error() {
        if is_available() {
            // Skip — we want to assert the not-installed branch.
            return;
        }
        let err = rasterize_page(b"%PDF-1.4\n%%EOF\n", &RasterizeOptions::default()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("pdftoppm") || msg.contains("poppler"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn vmem_limit_defaults_when_env_unset() {
        // Safety: this test mutates a process-global env var. It's small
        // and only sets/unsets one variable; no other test reads it.
        // SAFETY: env access requires unsafe in edition 2024.
        unsafe {
            std::env::remove_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB");
        }
        assert_eq!(vmem_limit_kb(), DEFAULT_CHILD_VMEM_LIMIT_KB);
    }

    #[test]
    fn vmem_limit_bytes_is_kib_times_1024() {
        // The historical bug: `--as` is bytes, not KiB. Guard the conversion
        // so the default cap is a real 4 GiB, not 4 MiB.
        // SAFETY: env access requires unsafe in edition 2024.
        unsafe {
            std::env::remove_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB");
        }
        assert_eq!(vmem_limit_bytes(), DEFAULT_CHILD_VMEM_LIMIT_KB * 1024);
        assert_eq!(vmem_limit_bytes(), 4 * 1024 * 1024 * 1024); // 4 GiB
    }

    #[test]
    fn vmem_limit_honours_env_override() {
        // SAFETY: env access requires unsafe in edition 2024.
        unsafe {
            std::env::set_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB", "8388608");
        }
        assert_eq!(vmem_limit_kb(), 8_388_608);
        unsafe {
            std::env::remove_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB");
        }
    }

    #[test]
    fn vmem_limit_ignores_invalid_env() {
        // Garbage env var falls back to default rather than blocking ingestion.
        unsafe {
            std::env::set_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB", "not-a-number");
        }
        assert_eq!(vmem_limit_kb(), DEFAULT_CHILD_VMEM_LIMIT_KB);
        unsafe {
            std::env::set_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB", "0");
        }
        assert_eq!(vmem_limit_kb(), DEFAULT_CHILD_VMEM_LIMIT_KB);
        unsafe {
            std::env::remove_var("THAIRAG__PDF_RASTERIZER__VMEM_LIMIT_KB");
        }
    }

    #[test]
    fn prlimit_disable_flag_recognised() {
        for val in ["1", "true", "yes"] {
            unsafe {
                std::env::set_var("THAIRAG__PDF_RASTERIZER__DISABLE_PRLIMIT", val);
            }
            assert!(prlimit_disabled(), "value `{val}` should disable prlimit");
        }
        unsafe {
            std::env::set_var("THAIRAG__PDF_RASTERIZER__DISABLE_PRLIMIT", "0");
        }
        assert!(!prlimit_disabled());
        unsafe {
            std::env::remove_var("THAIRAG__PDF_RASTERIZER__DISABLE_PRLIMIT");
        }
    }

    #[test]
    fn rejects_garbage_pdf() {
        if !is_available() {
            return;
        }
        let err = rasterize_page(b"this is not a pdf", &RasterizeOptions::default()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.to_lowercase().contains("pdftoppm")
                || msg.to_lowercase().contains("syntax")
                || msg.to_lowercase().contains("error"),
            "unexpected error: {msg}"
        );
    }
}
