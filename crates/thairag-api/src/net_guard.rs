//! Outbound-request and upload-content guards.
//!
//! `validate_external_url` is the SSRF gate for any user-supplied URL the
//! server will fetch (document refresh): scheme allowlist, no embedded
//! credentials, and no host that resolves to a private / loopback /
//! link-local / metadata address. `redact_url` keeps credentials and query
//! strings out of logs and job descriptions. `mime_matches_magic` verifies
//! that uploaded bytes actually look like the declared MIME type.

use std::net::IpAddr;

use thairag_core::error::{Result, ThaiRagError};

fn err(msg: String) -> ThaiRagError {
    ThaiRagError::Validation(msg)
}

/// True for any address an external-content fetch must never reach:
/// loopback, RFC1918, link-local (incl. 169.254.169.254 cloud metadata),
/// CGNAT, unspecified, broadcast, documentation/benchmark ranges, and their
/// IPv6 equivalents (with v4-mapped addresses re-checked as v4).
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || o[0] == 100 && (o[1] & 0xC0) == 64 // 100.64.0.0/10 CGNAT
                || o[0] == 192 && o[1] == 0 && o[2] == 0 // 192.0.0.0/24
                || o[0] == 198 && (o[1] & 0xFE) == 18 // 198.18.0.0/15
                || o[0] >= 224 // multicast + reserved
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(&IpAddr::V4(v4));
            }
            let seg = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || (seg[0] & 0xFE00) == 0xFC00 // fc00::/7 unique-local
                || (seg[0] & 0xFFC0) == 0xFE80 // fe80::/10 link-local
                || (seg[0] & 0xFF00) == 0xFF00 // multicast
        }
    }
}

/// SSRF gate for a user-supplied URL the server will fetch.
///
/// Rejects non-http(s) schemes, embedded credentials, hosts that are private
/// literal IPs, and hostnames where ANY resolved address is private. The
/// caller must also fetch with redirects DISABLED — a redirect hop is a fresh
/// URL this function never saw.
pub async fn validate_external_url(url_str: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(url_str).map_err(|e| err(format!("Invalid source URL: {e}")))?;

    match url.scheme() {
        "http" | "https" => {}
        s => return Err(err(format!("Source URL scheme '{s}' is not allowed"))),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(err(
            "Source URL must not contain embedded credentials".into()
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| err("Source URL has no host".into()))?;

    // Literal IP host: check directly. Hostname: resolve and check every
    // address (a name with mixed public/private records is rejected).
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(err(
                "Source URL resolves to a private or internal address".into()
            ));
        }
    } else {
        let port = url.port_or_known_default().unwrap_or(443);
        let addrs: Vec<_> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| err(format!("Source URL host did not resolve: {e}")))?
            .collect();
        if addrs.is_empty() {
            return Err(err("Source URL host did not resolve".into()));
        }
        if addrs.iter().any(|a| is_private_ip(&a.ip())) {
            return Err(err(
                "Source URL resolves to a private or internal address".into()
            ));
        }
    }
    Ok(url)
}

/// URL safe for logs and job descriptions: scheme + host + path only —
/// credentials and query strings (tokens, signatures) are dropped.
pub fn redact_url(url_str: &str) -> String {
    match reqwest::Url::parse(url_str) {
        Ok(u) => format!(
            "{}://{}{}{}",
            u.scheme(),
            u.host_str().unwrap_or("<no-host>"),
            u.port().map(|p| format!(":{p}")).unwrap_or_default(),
            u.path()
        ),
        Err(_) => "<unparseable-url>".to_string(),
    }
}

/// Verify uploaded bytes look like the declared MIME type, for the formats
/// with unambiguous magic numbers. Types without reliable magic (plain text,
/// markdown, csv, html, json) return `true` — they cannot be sniffed.
pub fn mime_matches_magic(mime: &str, bytes: &[u8]) -> bool {
    let mime = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        // Spec says offset 0; real-world PDFs occasionally carry a small
        // preamble, so search the first 1 KB like pdf readers do.
        "application/pdf" => bytes.windows(5).take(1024).any(|w| w == b"%PDF-"),
        // OOXML containers are zip archives.
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            bytes.starts_with(b"PK\x03\x04")
        }
        "image/png" => bytes.starts_with(&[0x89, b'P', b'N', b'G']),
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => bytes.starts_with(b"GIF8"),
        "image/webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_private_and_metadata_targets() {
        for url in [
            "http://127.0.0.1:6379/",
            "http://10.0.0.5/x",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://100.64.0.1/",
            "ftp://example.com/file",
            "file:///etc/passwd",
            "http://user:pass@example.com/doc",
        ] {
            assert!(
                validate_external_url(url).await.is_err(),
                "must reject {url}"
            );
        }
    }

    #[test]
    fn private_ranges_classified() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.169.254",
            "::1",
            "fd00::1",
            "fe80::1",
        ] {
            assert!(is_private_ip(&ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700::1111"] {
            assert!(!is_private_ip(&ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn redaction_strips_credentials_and_query() {
        assert_eq!(
            redact_url("https://user:secret@example.com:8443/docs/a.pdf?sig=abc&token=x"),
            "https://example.com:8443/docs/a.pdf"
        );
        assert_eq!(redact_url("not a url"), "<unparseable-url>");
    }

    #[test]
    fn magic_sniffing() {
        assert!(mime_matches_magic("application/pdf", b"%PDF-1.7 ..."));
        assert!(!mime_matches_magic(
            "application/pdf",
            b"MZ\x90\x00 exe bytes"
        ));
        assert!(mime_matches_magic(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            b"PK\x03\x04rest"
        ));
        assert!(!mime_matches_magic("image/png", b"GIF89a"));
        // Unsniffable types pass through.
        assert!(mime_matches_magic("text/plain", b"anything"));
        assert!(mime_matches_magic("text/markdown; charset=utf-8", b"# hi"));
    }
}
