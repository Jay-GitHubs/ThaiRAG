//! Provision the native `libpdfium` library at build time.
//!
//! `pdfium-render` is a binding to Chromium's PDF engine; the actual native
//! library is not on crates.io. This script downloads the right prebuilt
//! `libpdfium` for the target from `bblanchon/pdfium-binaries` and records its
//! path via `cargo:rustc-env=PDFIUM_DYLIB_PATH`, which the engine reads with
//! `option_env!` to bind at runtime.
//!
//! Failure is non-fatal: we emit a `cargo:warning` and continue. The engine
//! then falls back to the system library / CWD and reports unavailable if
//! nothing is found, so the build never breaks on a missing download (offline
//! CI, restricted networks, unsupported targets). In Docker we instead bake
//! `libpdfium.so` onto the system library path.
//!
//! Overrides:
//! - `PDFIUM_DYLIB_PATH` — point at a pre-installed binary; skip downloading.
//! - `PDFIUM_RELEASE` — pin a release tag (default: `latest`).

use std::path::PathBuf;
use std::{env, io};

fn main() {
    println!("cargo:rerun-if-env-changed=PDFIUM_DYLIB_PATH");
    println!("cargo:rerun-if-env-changed=PDFIUM_RELEASE");

    // Operator-provided binary path wins — no download.
    if let Ok(path) = env::var("PDFIUM_DYLIB_PATH") {
        if PathBuf::from(&path).exists() {
            println!("cargo:rustc-env=PDFIUM_DYLIB_PATH={path}");
            return;
        }
        println!("cargo:warning=pdfium: PDFIUM_DYLIB_PATH={path} does not exist; ignoring.");
    }

    match provision() {
        Ok(path) => println!("cargo:rustc-env=PDFIUM_DYLIB_PATH={}", path.display()),
        Err(e) => println!(
            "cargo:warning=pdfium: could not provision libpdfium ({e}). The smart-PDF \
             engine will be disabled unless libpdfium is on the system library path \
             (Docker bakes it in). Set PDFIUM_DYLIB_PATH to override."
        ),
    }
}

fn provision() -> io::Result<PathBuf> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(other)?);
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let (asset, libname) = match (os.as_str(), arch.as_str()) {
        ("linux", "x86_64") => ("pdfium-linux-x64.tgz", "libpdfium.so"),
        ("linux", "aarch64") => ("pdfium-linux-arm64.tgz", "libpdfium.so"),
        ("macos", "x86_64") => ("pdfium-mac-x64.tgz", "libpdfium.dylib"),
        ("macos", "aarch64") => ("pdfium-mac-arm64.tgz", "libpdfium.dylib"),
        _ => return Err(other(format!("unsupported target {os}/{arch}"))),
    };

    let dest = out_dir.join(libname);
    if dest.exists() {
        return Ok(dest);
    }

    let release = env::var("PDFIUM_RELEASE").unwrap_or_else(|_| "latest".to_string());
    let url = if release == "latest" {
        format!("https://github.com/bblanchon/pdfium-binaries/releases/latest/download/{asset}")
    } else {
        format!("https://github.com/bblanchon/pdfium-binaries/releases/download/{release}/{asset}")
    };

    // Download the .tgz (ureq follows GitHub's redirect to the CDN).
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| other(format!("download {url}: {e}")))?;
    let mut buf = Vec::new();
    io::copy(&mut resp.into_reader(), &mut buf)?;

    // Extract just lib/<libname> from the gzipped tarball.
    let gz = flate2::read::GzDecoder::new(&buf[..]);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path.file_name().and_then(|n| n.to_str()) == Some(libname) {
            entry.unpack(&dest)?;
            return Ok(dest);
        }
    }
    Err(other(format!("{libname} not found inside {asset}")))
}

fn other(msg: impl ToString) -> io::Error {
    io::Error::other(msg.to_string())
}
