//! Bounded HTTPS download and SHA-256 verification for AppImages.
//!
//! The body is streamed straight to disk under a hard cap — never buffered
//! whole — and hashed while it is written. A checksum is mandatory unless
//! the caller explicitly opts out.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// 1 GiB: generous for real AppImages, still a hard bound.
pub const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

pub struct Downloaded {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

pub fn valid_hex_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Hash a local file without loading it whole.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let (digest, _) = hash_reader(file, &mut std::io::sink(), u64::MAX)?;
    Ok(digest)
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

/// Download one HTTPS URL to `dest`, verifying SHA-256 before returning.
pub fn download(
    url: &str,
    dest: &Path,
    expected: Option<&str>,
    allow_unverified: bool,
) -> Result<Downloaded, String> {
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
    crate::safety::url::validate_external_url(url).map_err(|e| format!("rejected URL: {e}"))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
        .connect_timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("refusing a plaintext redirect")
            } else {
                attempt.follow()
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
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    let (digest, bytes) = hash_reader(&mut response, &mut writer, MAX_BYTES)?;
    writer.flush().map_err(|e| e.to_string())?;
    if let Some(expected) = expected
        && !digest.eq_ignore_ascii_case(expected)
    {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "sha256 MISMATCH — expected {expected}, downloaded {digest}; file discarded"
        ));
    }
    Ok(Downloaded {
        path: dest.to_path_buf(),
        sha256: digest,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{hash_reader, sha256_file, valid_hex_sha256};
    use std::io::Cursor;

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
    fn hashes_a_local_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
