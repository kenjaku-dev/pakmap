//! Shared external-URL gate.
//!
//! The contract mirrors the GUI's `validate_package_url` (docs/gui
//! -architecture.md §opener): https only (http accepted for loopback dev
//! targets), no embedded credentials, no control characters, 2048-byte cap.
//! GUI wiring moves onto this primitive in the URL phase of the plan; news
//! links join the same gate. This is an *open-in-browser* gate — the app never
//! fetches the URL from its own network stack.

use thiserror::Error;

/// Byte cap for external URLs (same cap the GUI enforces in Rust).
pub const URL_MAX_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UrlError {
    #[error("empty URL")]
    Empty,
    #[error("URL too long ({0} byte cap)")]
    TooLong(usize),
    #[error("URL contains control characters (the WHATWG parser would strip them for you)")]
    ControlChars,
    #[error("not a parseable URL")]
    NotParseable,
    #[error("scheme '{0}' is not allowed — only https pages are opened")]
    BadScheme(String),
    #[error("URL contains embedded credentials")]
    Credentials,
    #[error("URL has no host")]
    NoHost,
}

/// Loopback-only exception for http (dev servers viewed on package pages).
pub fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "::1" | "[::1]"
    ) || host.starts_with("127.")
}

/// Validate `raw` and return the host on success — audit logging carries the
/// host only, never path/query (they can encode per-user tracking).
pub fn validate_external_url(raw: &str) -> Result<String, UrlError> {
    if raw.is_empty() {
        return Err(UrlError::Empty);
    }
    if raw.len() > URL_MAX_BYTES {
        return Err(UrlError::TooLong(URL_MAX_BYTES));
    }
    if raw.chars().any(char::is_control) {
        return Err(UrlError::ControlChars);
    }
    let parsed = url::Url::parse(raw).map_err(|_| UrlError::NotParseable)?;
    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback_host(parsed.host_str().unwrap_or_default()) => {}
        scheme => return Err(UrlError::BadScheme(scheme.to_string())),
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(UrlError::Credentials);
    }
    parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .ok_or(UrlError::NoHost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_and_loopback_http_pass_returning_host() {
        assert_eq!(
            validate_external_url("https://mozilla.org/en-US/firefox/").unwrap(),
            "mozilla.org"
        );
        assert_eq!(
            validate_external_url("https://aur.archlinux.org/packages/pakmap").unwrap(),
            "aur.archlinux.org"
        );
        for loopback in [
            "http://localhost:8080/",
            "http://127.0.0.1:3000/x",
            "http://127.0.0.2/",
            "http://LOCALHOST/",
            "http://[::1]:4444/",
        ] {
            assert!(
                validate_external_url(loopback).is_ok(),
                "loopback dev target {loopback} must pass"
            );
        }
    }

    #[test]
    fn hostile_schemes_hosts_and_framing_fail() {
        assert!(matches!(
            validate_external_url("javascript:alert(1)"),
            Err(UrlError::BadScheme(_))
        ));
        assert!(matches!(
            validate_external_url("data:text/plain,hi"),
            Err(UrlError::BadScheme(_))
        ));
        assert!(matches!(
            validate_external_url("file:///etc/passwd"),
            Err(UrlError::BadScheme(_))
        ));
        assert!(matches!(
            validate_external_url("http://example.com/"),
            Err(UrlError::BadScheme(_))
        ));
        assert!(matches!(
            validate_external_url("https://u:p@example.com/"),
            Err(UrlError::Credentials)
        ));
        assert!(matches!(
            validate_external_url("https://user@example.com/"),
            Err(UrlError::Credentials)
        ));
        assert_eq!(validate_external_url(""), Err(UrlError::Empty), "empty");
    }

    #[test]
    fn boundaries_hold() {
        // Control characters are rejected pre-parse (the WHATWG parser would
        // silently strip TAB/CR/LF otherwise — scheme injection via that hole).
        assert!(matches!(
            validate_external_url("https://a\x01b.example.com/"),
            Err(UrlError::ControlChars)
        ));
        assert!(matches!(
            validate_external_url("https://example.com/\tx?"),
            Err(UrlError::ControlChars)
        ));
        // Not a URL at all.
        assert!(matches!(
            validate_external_url("htop"),
            Err(UrlError::NotParseable)
        ));
        // Cap: 2048 bytes exactly is fine, 2049 not ("https://"=8 + "/"=1 →
        // host 2039 lands on the boundary).
        let ok = format!("https://{}/", "a".repeat(2039));
        assert_eq!(ok.len(), URL_MAX_BYTES);
        assert!(validate_external_url(&ok).is_ok());
        let cap = format!("https://{}/", "a".repeat(2040));
        assert!(matches!(
            validate_external_url(&cap),
            Err(UrlError::TooLong(URL_MAX_BYTES))
        ));
    }

    #[test]
    fn loopback_helper_matches_gui_semantics() {
        for host in ["localhost", "::1", "[::1]", "127.0.0.9", "LocalHost"] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in ["example.com", "localhost.example.com", "192.168.0.1"] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }
}
