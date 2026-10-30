//! Shared HTTP-body caps (CWE-409 memory exhaustion).
//!
//! Every HTTP body pakmap reads goes through here: the bytes are bounded
//! DURING the read (`take` / chunk loop), so a hostile or misconfigured
//! endpoint — the AUR RPC is configurable (`behavior.aur_rpc`) and the news
//! feed is remote — can neither balloon the process nor stream forever. A
//! body past its class's cap is an explicit error; callers degrade the way
//! they already degrade other failures (search error / report err / empty
//! feed), never by allocating first and hoping.
//!
//! Caps are per dependency class (bulkhead sizing, `resilience-failure`):
//! - [`RPC_BODY_CAP_BYTES`] covers AUR RPC JSON (search/info): real-world
//!   payloads top out at a few MB even for wide `info` chunks.
//! - [`FEED_BODY_CAP_BYTES`] covers the Arch news RSS.

use thiserror::Error;

/// AUR RPC JSON body cap (16 MiB).
pub const RPC_BODY_CAP_BYTES: usize = 16 * 1024 * 1024;
/// Arch news RSS feed body cap (4 MiB).
pub const FEED_BODY_CAP_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BodyError {
    #[error("response body exceeded the {0} byte cap")]
    TooLarge(usize),
    #[error("reading the response body failed: {0}")]
    Io(String),
}

/// Read a blocking response body under `cap`. Bytes stay bounded even when
/// the body advertises a length we don't trust — the cap governs. Consumes
/// the response (its `Read` impl + `take` is the bounding mechanism).
pub fn read_body_capped(
    resp: reqwest::blocking::Response,
    cap: usize,
) -> Result<String, BodyError> {
    use std::io::Read;
    // Read cap+1: one byte more than allowed, so "too large" is detectable
    // without ever buffering past cap+1.
    let mut limited = resp.take(cap as u64 + 1);
    let mut bytes = Vec::with_capacity(64 * 1024);
    if let Err(e) = limited.read_to_end(&mut bytes) {
        return Err(BodyError::Io(e.to_string()));
    }
    drop(limited);
    if bytes.len() > cap {
        return Err(BodyError::TooLarge(cap));
    }
    // Bodies we parse next are JSON/XML — invalid UTF-8 stays an error (same
    // shape the old `resp.text()` path produced via its own error).
    String::from_utf8(bytes).map_err(|_| BodyError::Io("body is not valid UTF-8".to_string()))
}

/// Async mirror of [`read_body_capped`] for the tokio search path.
pub async fn read_body_async_capped(
    mut resp: reqwest::Response,
    cap: usize,
) -> Result<String, BodyError> {
    let mut bytes = Vec::with_capacity(64 * 1024);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| BodyError::Io(e.to_string()))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > cap {
            // Stop reading: the response is dropped, its connection closed.
            return Err(BodyError::TooLarge(cap));
        }
    }
    String::from_utf8(bytes).map_err(|_| BodyError::Io("body is not valid UTF-8".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// Minimal loopback HTTP responder: exact bytes, then close.
    /// `std`-only — no test-server dependency.
    fn start_server(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            if let Ok((mut conn, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = std::io::Read::read(&mut conn, &mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = conn.write_all(head.as_bytes());
                let _ = conn.write_all(&body);
                let _ = conn.flush();
            }
        });
        (format!("http://{addr}/"), handle)
    }

    fn blocking_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap()
    }

    #[test]
    fn small_body_returns_whole_text() {
        let (url, server) = start_server(b"safe payload under the cap".to_vec());
        let resp = blocking_client().get(&url).send().unwrap();
        let text = read_body_capped(resp, 1024).unwrap();
        assert_eq!(text, "safe payload under the cap");
        server.join().unwrap();
    }

    #[test]
    fn oversized_body_is_refused_before_parse() {
        // 64 KiB body against a 1 KiB cap.
        let (url, server) = start_server(vec![b'x'; 64 * 1024]);
        let resp = blocking_client().get(&url).send().unwrap();
        assert_eq!(read_body_capped(resp, 1024), Err(BodyError::TooLarge(1024)));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn async_mirror_reads_and_caps() {
        let (url, server) = start_server(b"hello-over-async".to_vec());
        let resp = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap();
        assert_eq!(
            read_body_async_capped(resp, 1024).await.unwrap(),
            "hello-over-async"
        );
        server.join().unwrap();

        // Same server shape, oversized this time.
        let (url, server) = start_server(vec![b'y'; 4096]);
        let resp = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap();
        assert_eq!(
            read_body_async_capped(resp, 1024).await,
            Err(BodyError::TooLarge(1024))
        );
        server.join().unwrap();
    }

    #[test]
    fn cap_boundaries_match_one_above() {
        // Exactly the cap passes, cap+1 bytes errors — the off-by-one routing
        // both a friendly endpoint and a hostile one hit.
        let (url, server) = start_server(vec![b'a'; 512]);
        let resp = blocking_client().get(&url).send().unwrap();
        assert!(read_body_capped(resp, 512).is_ok());
        server.join().unwrap();

        let (url, server2) = start_server(vec![b'a'; 512]);
        let resp = blocking_client().get(&url).send().unwrap();
        assert_eq!(read_body_capped(resp, 511), Err(BodyError::TooLarge(511)));
        server2.join().unwrap();
    }
}
