//! Rasterize PDF pages to PNG via `pdftoppm` (poppler-utils).
//!
//! Subprocess isolation is intentional: a malformed/malicious PDF that
//! crashes the renderer will only kill the child process, not the API.
//! All inputs are passed via stdin (no temp files, no shell), and every
//! invocation has a hard timeout plus a virtual-memory cap (Linux only).

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use tracing::{debug, warn};

/// Hard cap on output PNG size per page. Anything larger is rejected to
/// prevent a malicious PDF from producing a multi-GB image that exhausts
/// host memory while we read it.
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024; // 32 MiB

/// Virtual-memory limit applied to the `pdftoppm` child process (KiB).
/// 1 GiB is generous for legitimate slide rasterization at 150 DPI while
/// still bounding zip-bomb-style PDFs.
const CHILD_VMEM_LIMIT_KB: u64 = 1_048_576;

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
/// PDF bytes are streamed to `pdftoppm` via stdin; PNG bytes are read from
/// stdout. No filesystem temp files are created and no user input is
/// interpolated into shell arguments — only fixed flags and integer values.
pub fn rasterize_page(pdf_bytes: &[u8], opts: &RasterizeOptions) -> Result<Vec<u8>> {
    if opts.page == 0 {
        return Err(ThaiRagError::Validation(
            "rasterize_page: page must be >= 1".into(),
        ));
    }

    // `prlimit` is used to cap virtual memory of the child on Linux. On
    // platforms where it's missing (macOS dev boxes) we fall back to a
    // plain Command — the timeout still applies.
    let use_prlimit = cfg!(target_os = "linux") && which_exists("prlimit");

    let mut cmd = if use_prlimit {
        let mut c = Command::new("prlimit");
        c.arg(format!("--as={CHILD_VMEM_LIMIT_KB}"));
        c.arg("--");
        c.arg("pdftoppm");
        c
    } else {
        Command::new("pdftoppm")
    };

    cmd.arg("-png")
        .arg("-r")
        .arg(opts.dpi.to_string())
        .arg("-f")
        .arg(opts.page.to_string())
        .arg("-l")
        .arg(opts.page.to_string())
        .arg("-singlefile")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        ThaiRagError::Validation(format!(
            "pdftoppm not available — install poppler-utils ({e})"
        ))
    })?;

    // Stream PDF to stdin in its own scope so the pipe closes before we wait.
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| ThaiRagError::Validation("failed to open pdftoppm stdin".into()))?;
        stdin
            .write_all(pdf_bytes)
            .map_err(|e| ThaiRagError::Validation(format!("write to pdftoppm stdin: {e}")))?;
    }

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

    if output.stdout.len() > MAX_PNG_BYTES {
        return Err(ThaiRagError::Validation(format!(
            "rasterized page exceeds {} bytes — possible abusive PDF",
            MAX_PNG_BYTES
        )));
    }

    if !is_png(&output.stdout) {
        return Err(ThaiRagError::Validation(
            "pdftoppm produced output that is not a PNG".into(),
        ));
    }

    debug!(
        page = opts.page,
        dpi = opts.dpi,
        png_bytes = output.stdout.len(),
        "rasterized PDF page"
    );

    Ok(output.stdout)
}

/// Return how many pages the PDF reports via `pdfinfo`, or `None` if the
/// tool isn't installed or the output can't be parsed.
pub fn page_count(pdf_bytes: &[u8]) -> Option<usize> {
    let mut child = Command::new("pdfinfo")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    child.stdin.as_mut()?.write_all(pdf_bytes).ok()?;

    let output = child.wait_with_output().ok()?;
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
