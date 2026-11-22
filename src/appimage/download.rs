//! Bounded HTTPS download and SHA-256 verification for AppImages.
//!
//! The body is streamed straight to a caller-provided file under a hard cap —
//! never buffered whole — and hashed while it is written. A checksum is
//! mandatory unless the caller explicitly opts out. Every redirect hop is
//! re-validated, including a private/loopback address check, so a public URL
//! cannot bounce the request into the user's LAN.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr};

use sha2::{Digest, Sha256};

/// 1 GiB: generous for real AppImages, still a hard bound.
pub const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;
const MAX_REDIRECTS: usize = 5;

pub fn valid_hex_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Stream `reader` into `writer`, enforcing `cap` and hashing on the fly.
/// Returns the hex digest and the number of bytes written.
pub(crate) fn hash_reader<R: Read, W: Write>(
    mut reader: R,
    writer: &mut W,
    cap: u64,
) -> Result<(String, u64), String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|e| format!("read failed: {e}"))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or("download size overflow")?;
        // One byte past the cap is enough to detect a too-large body without
        // buffering it: stop as soon as we cross the boundary.
        if total > cap {
            return Err(format!("body exceeded the {cap} byte cap"));
        }
        hasher.update(&buf[..read]);
        writer
            .write_all(&buf[..read])
            .map_err(|e| format!("write failed: {e}"))?;
    }
    Ok((format!("{:x}", hasher.finalize()), total))
}

fn v4_is_private(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1])) // CGNAT 100.64/10
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking 198.18/15
        || o[0] >= 224 // multicast + reserved
}

fn v6_is_private(v6: Ipv6Addr) -> bool {
    let first = v6.segments()[0];
    v6.is_loopback()
        || v6.is_unspecified()
        || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (first & 0xffc0) == 0xfe80 // link local fe80::/10
        || (first & 0xffc0) == 0xfec0 // deprecated site local
        || v6.to_ipv4_mapped().is_some_and(v4_is_private)
}

/// True for names that resolve (by convention or configuration) to the local
/// machine or a private network. DNS rebinding is out of scope; this stops the
/// obvious literal targets.
fn is_private_host(host: &str) -> bool {
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "metadata.google.internal") {
        return true;
    }
    if host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
    {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => v4_is_private(v4),
        Ok(std::net::IpAddr::V6(v6)) => v6_is_private(v6),
        Err(_) => false,
    }
}

/// One redirect hop must stay on https and must not be aimed at a private
/// address. Loopback targets are allowed only when the original URL was
/// loopback too (a developer deliberately pointing at a local server).
fn redirect_target_allowed(url: &url::Url, allow_loopback: bool) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("refusing a plaintext redirect".to_string());
    }
    crate::safety::url::validate_external_url(url.as_str())
        .map_err(|e| format!("rejected redirect URL: {e}"))?;
    let host = url.host_str().unwrap_or_default();
    if is_private_host(host) {
        let loopback = crate::safety::url::is_loopback_host(host);
        if !(allow_loopback && loopback) {
            return Err("refusing a redirect to a private or loopback address".to_string());
        }
    }
    Ok(())
}

/// Download one HTTPS URL into an already-open file, verifying SHA-256 before
/// returning. The caller owns the file and is responsible for deleting it on
/// error (a `NamedTempFile`/`TempFile` guard does this automatically).
pub fn download_into(
    url: &str,
    dest: &mut std::fs::File,
    expected: Option<&str>,
    allow_unverified: bool,
) -> Result<(String, u64), String> {
    match expected {
        Some(hash) if !valid_hex_sha256(hash) => {
            return Err("--sha256 must be exactly 64 hex characters".to_string());
        }
        None if !allow_unverified => {
            return Err(
                "sha256 is required — pass --sha256 <hex> or --allow-unverified".to_string(),
            );
        }
        _ => {}
    }
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("only https URLs are accepted".to_string());
    }
    // Same gate the opener/news use: no embedded credentials, control chars,
    // hosts without a name, or over-long URLs.
    let original_host =
        crate::safety::url::validate_external_url(url).map_err(|e| format!("rejected URL: {e}"))?;
    let allow_loopback = crate::safety::url::is_loopback_host(&original_host);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
        .connect_timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            match redirect_target_allowed(attempt.url(), allow_loopback) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        }))
        .build()
        .map_err(|e| format!("HTTP client failed: {e}"))?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| format!("download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("download failed: HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|len| len > MAX_BYTES) {
        return Err(format!(
            "download declares {} bytes, above the {MAX_BYTES} byte cap",
            response.content_length().unwrap_or_default()
        ));
    }
    let (digest, bytes) = hash_reader(&mut response, dest, MAX_BYTES)?;
    dest.flush().map_err(|e| e.to_string())?;
    if let Some(expected) = expected
        && !digest.eq_ignore_ascii_case(expected)
    {
        return Err(format!(
            "sha256 MISMATCH — expected {expected}, downloaded {digest}; file discarded"
        ));
    }
    Ok((digest, bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{hash_reader, is_private_host, redirect_target_allowed, valid_hex_sha256};
    use std::io::Cursor;

    fn url(raw: &str) -> url::Url {
        url::Url::parse(raw).unwrap()
    }

    #[test]
    fn checksum_shape_is_strict() {
        assert!(valid_hex_sha256(&"ab".repeat(32)));
        assert!(!valid_hex_sha256("abc"));
        assert!(!valid_hex_sha256(&format!("{}z", "a".repeat(63))));
        assert!(!valid_hex_sha256(&"AB".repeat(32)[..63]));
    }

    #[test]
    fn hashes_bytes_while_streaming() {
        let mut out = Vec::new();
        let (digest, bytes) = hash_reader(Cursor::new(b"abc".to_vec()), &mut out, 16).unwrap();
        assert_eq!(bytes, 3);
        assert_eq!(out, b"abc");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn enforces_the_body_cap_while_reading() {
        let mut out = Vec::new();
        let err = hash_reader(Cursor::new(vec![7u8; 100]), &mut out, 16).unwrap_err();
        assert!(err.contains("cap"), "{err}");
        assert!(out.len() <= 17, "must stop at cap+1, not buffer the rest");
    }

    #[test]
    fn private_and_loopback_hosts_are_recognized() {
        for host in [
            "localhost",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "foo.local",
            "metadata.google.internal",
        ] {
            assert!(is_private_host(host), "{host} must be private");
        }
        for host in ["example.org", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(!is_private_host(host), "{host} must be public");
        }
    }

    #[test]
    fn redirects_are_https_only_and_never_reach_a_private_target() {
        assert!(redirect_target_allowed(&url("https://example.org/a"), false).is_ok());
        assert!(redirect_target_allowed(&url("https://127.0.0.1/a"), false).is_err());
        assert!(redirect_target_allowed(&url("https://192.168.1.1/a"), false).is_err());
        assert!(redirect_target_allowed(&url("https://[::1]/a"), false).is_err());
        assert!(redirect_target_allowed(&url("http://example.org/a"), false).is_err());
        assert!(redirect_target_allowed(&url("https://u:p@example.org/a"), false).is_err());
    }

    #[test]
    fn loopback_redirects_need_a_loopback_origin() {
        assert!(redirect_target_allowed(&url("https://127.0.0.1:8443/a"), true).is_ok());
        assert!(redirect_target_allowed(&url("https://localhost/a"), true).is_ok());
        assert!(
            redirect_target_allowed(&url("https://192.168.1.1/a"), true).is_err(),
            "loopback origin does not unlock the LAN"
        );
    }
}
