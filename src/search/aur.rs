use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::model::Package;

#[allow(dead_code)]
const AUR_RPC_DEFAULT: &str = "https://aur.archlinux.org/rpc/v5";

fn aur_rpc_with_config(cfg: &Config) -> String {
    cfg.aur_rpc_url()
}

// ---------------------------------------------------------------------------
// P3 perf: shared HTTP tuning + connection warm-up.
//
// All AUR-speaking HTTP clients share one tuning profile so knobs never
// drift: gzip response decoding (Cargo feature), a bounded *connect* phase
// (a dead network fails in seconds instead of burning the whole budget
// while the TUI spins), explicit `tcp_nodelay` (no small-write Nagle
// stalls), and the UA. The overall timeout still comes from the caller's
// config. Plus a one-shot per-process connection warm-up: a cheap HEAD at
// the RPC origin, so the TCP+TLS handshake is already in the pool when the
// first real request runs (TUI idle time; CLI overlaps it with the repo
// fan-out). HEAD is the cheapest warm — no body, ~200ms handshake total.
// ---------------------------------------------------------------------------

/// Bound on the *connect* phase only (dns+tcp+tls); the request timeout
/// keeps using the caller's `search.timeout_secs`.
pub const AUR_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Tuned async client builder (CLI fan-outs: search/updates/stats).
pub fn client_builder(secs: u64) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(format!("pakmap/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(secs.max(1)))
        .connect_timeout(Duration::from_secs(AUR_CONNECT_TIMEOUT_SECS))
        .tcp_nodelay(true)
}

/// Tuned blocking client builder (TUI worker pools).
pub fn blocking_client_builder(secs: u64) -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .user_agent(format!("pakmap/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(secs.max(1)))
        .connect_timeout(Duration::from_secs(AUR_CONNECT_TIMEOUT_SECS))
        .tcp_nodelay(true)
}

/// One-shot-per-process guard shared by the async and blocking warm-ups
/// (`true` = already attempted); the pooled connection lives on either way.
static AUR_WARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// RPC origin from a configured RPC URL — `https://aur.archlinux.org/rpc/v5`
/// becomes `https://aur.archlinux.org/`. Pure; None for unparsable or
/// non-http(s) URLs (warm-up silently skipped, never fatal).
pub fn aur_warm_origin(rpc_url: &str) -> Option<String> {
    let parsed = url::Url::parse(rpc_url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.trim();
    if host.is_empty() {
        return None;
    }
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some(format!("{}://{}{}/", parsed.scheme(), host, port))
}

/// Blocking warm-up (TUI workers): one HEAD per process, fire-and-forget.
/// Never panics, never blocks the caller beyond one `send()` — call it from
/// a spawned thread.
pub fn warm_blocking_aur(cfg: &Config) {
    warm_blocking_aur_url(&cfg.aur_rpc_url(), cfg.search.timeout_secs.max(1));
}

fn warm_blocking_aur_url(rpc_url: &str, timeout_secs: u64) {
    if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
        return;
    }
    // Already attempted (or racing): the pool is warm-or-in-flight either way.
    if AUR_WARMED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(origin) = aur_warm_origin(rpc_url) else {
        return;
    };
    let Ok(client) = blocking_client_for_timeout(timeout_secs) else {
        return;
    };
    let timer = crate::timing::start("aur-warm");
    let outcome = client.head(&origin).send();
    timer.finish(usize::from(outcome.is_ok()));
    match outcome {
        Ok(_) => tracing::debug!(%origin, "aur connection warmed"),
        Err(e) => tracing::debug!(err=?e, %origin, "aur warmup failed (non-fatal)"),
    }
}

/// Async warm-up (CLI fan-outs): same one-shot semantics; call detached via
/// `tokio::spawn` with the *same* client the search uses, so the handshake
/// lands in the shared connection pool.
pub async fn warm_async(client: &reqwest::Client, rpc_url: &str) {
    if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
        return;
    }
    if AUR_WARMED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(origin) = aur_warm_origin(rpc_url) else {
        return;
    };
    let timer = crate::timing::start("aur-warm");
    let outcome = client.head(&origin).send().await;
    timer.finish(usize::from(outcome.is_ok()));
    match outcome {
        Ok(_) => tracing::debug!(%origin, "aur connection warmed"),
        Err(e) => tracing::debug!(err=?e, %origin, "aur warmup failed (non-fatal)"),
    }
}

/// Per-timeout client cache — building a blocking Client does TLS init, so
/// reuse one per timeout value instead of constructing per search.
/// Returns `Result` (no `expect`): a TLS/client build failure must degrade to
/// "no AUR results", never panic a TUI worker thread.
static BLOCKING_CLIENTS: OnceLock<Mutex<HashMap<u64, reqwest::blocking::Client>>> = OnceLock::new();

fn blocking_client_for_timeout(secs: u64) -> anyhow::Result<reqwest::blocking::Client> {
    let secs = secs.max(1);
    let cache = BLOCKING_CLIENTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock()
        && let Some(c) = guard.get(&secs)
    {
        return Ok(c.clone());
    }
    let client = blocking_client_builder(secs)
        .build()
        .context("failed to build blocking reqwest client")?;
    if let Ok(mut guard) = cache.lock() {
        guard.insert(secs, client.clone());
    }
    Ok(client)
}

/// Sort by popularity/votes desc (paru-style, NaN-safe), keeping only the top
/// `limit` via partial selection instead of a full sort when truncating.
fn sort_and_truncate(results: &mut Vec<Package>, limit: usize) {
    if results.len() <= 1 {
        return;
    }
    let cmp = |a: &Package, b: &Package| {
        let ap = a.popularity.filter(|v| v.is_finite()).unwrap_or(0.0);
        let bp = b.popularity.filter(|v| v.is_finite()).unwrap_or(0.0);
        bp.partial_cmp(&ap)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.votes.unwrap_or(0).cmp(&a.votes.unwrap_or(0)))
    };
    if limit != 0 && results.len() > limit {
        results.select_nth_unstable_by(limit, cmp);
        results.truncate(limit);
        results.sort_by(cmp);
    } else {
        results.sort_by(cmp);
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct AurResponse {
    version: u32,
    #[serde(rename = "type")]
    type_: String,
    resultcount: u32,
    results: Vec<AurPackage>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AurPackage {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "PackageBase")]
    pub package_base: Option<String>,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    #[serde(rename = "NumVotes")]
    pub num_votes: Option<i32>,
    #[serde(rename = "Popularity")]
    pub popularity: Option<f64>,
    #[serde(rename = "OutOfDate")]
    pub out_of_date: Option<i64>,
    #[serde(rename = "Maintainer")]
    pub maintainer: Option<String>,
    #[serde(rename = "FirstSubmitted")]
    pub first_submitted: Option<i64>,
    #[serde(rename = "LastModified")]
    pub last_modified: Option<i64>,
    #[serde(rename = "Depends")]
    pub depends: Option<Vec<String>>,
}

impl From<AurPackage> for Package {
    fn from(p: AurPackage) -> Self {
        Package {
            name: p.name,
            version: p.version,
            description: p.description,
            repo: "aur".to_string(),
            arch: None,
            url: p.url,
            installed: false,
            votes: p.num_votes,
            // Normalize non-finite values: serde_json renders NaN as null,
            // which breaks numeric consumers (jq, scripts) downstream.
            popularity: p.popularity.filter(|v| v.is_finite()),
            out_of_date: p.out_of_date,
            maintainer: p.maintainer,
            num_votes: p.num_votes,
            last_modified: p.last_modified,
            depends: None,
            optdepends: None,
            required_by: None,
            optional_for: None,
            reason: None,
            orphan: false,
        }
    }
}

pub async fn search_aur(
    client: &reqwest::Client,
    query: &str,
    by: &str,
    limit: usize,
    use_regex: bool,
) -> anyhow::Result<Vec<Package>> {
    let cfg = Config::load();
    search_aur_with_config(client, query, by, limit, use_regex, &cfg).await
}

/// Config-aware async search — pass the cached `Config` to avoid file IO
/// (`Config::load()`) on every search. Perf fix for CLI/TUI hot path.
pub async fn search_aur_with_config(
    client: &reqwest::Client,
    query: &str,
    by: &str,
    limit: usize,
    use_regex: bool,
    cfg: &Config,
) -> anyhow::Result<Vec<Package>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    // AUR RPC is case-insensitive, url-encode — config-aware (behavior.aur_rpc or default)
    let encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let encoded = encoded.replace('+', "%20");
    let rpc = aur_rpc_with_config(cfg);
    let url = format!("{}/search/{}?by={}", rpc, encoded, by);

    tracing::debug!(url=%url, "aur request");

    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            format!("pakmap/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .with_context(|| format!("AUR request failed {}", url))?;

    if !resp.status().is_success() {
        anyhow::bail!("AUR RPC returned HTTP {}", resp.status());
    }

    let text =
        crate::safety::net::read_body_async_capped(resp, crate::safety::net::RPC_BODY_CAP_BYTES)
            .await
            .with_context(|| format!("AUR RPC body read failed {}", url))?;
    // v0.9.3 (CWE-248): the literal byte-cap `[500]` panicked whenever it
    // landed mid multi-byte char — i.e. exactly when the RPC returned
    // something unparseable. Central char-boundary snippet instead.
    let parsed: AurResponse = serde_json::from_str(&text).with_context(|| {
        format!(
            "AUR json parse failed: {}",
            crate::safety::text::error_snippet(&text, 500)
        )
    })?;

    if parsed.type_ == "error" {
        let msg = parsed
            .error
            .unwrap_or_else(|| "unknown AUR error".to_string());
        anyhow::bail!("AUR error: {}", msg);
    }

    let mut results: Vec<Package> = parsed.results.into_iter().map(Package::from).collect();

    // client-side regex filtering if requested (AUR doesn't support regex)
    if use_regex {
        let re = regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()?;
        results.retain(|p| {
            re.is_match(&p.name)
                || p.description
                    .as_deref()
                    .map(|d| re.is_match(d))
                    .unwrap_or(false)
        });
    }

    // Sort by popularity/votes desc like paru does (more trusted first), but keep original if bottom_up false
    // NaN-safe: filter non-finite to 0.0 and use unwrap_or Equal per rust-common-pitfalls.
    // Partial selection when truncating (limit 50 of hundreds = O(n), not O(n log n)).
    sort_and_truncate(&mut results, limit);

    Ok(results)
}

pub fn search_aur_blocking(
    query: &str,
    by: &str,
    limit: usize,
    use_regex: bool,
) -> anyhow::Result<Vec<Package>> {
    let cfg = Config::load();
    search_aur_blocking_with_config(query, by, limit, use_regex, &cfg)
}

/// Config-aware blocking search — pass the App's cached `Config` to avoid
/// file IO (`Config::load()`) on every keystroke. Perf fix for TUI debounce path.
pub fn search_aur_blocking_with_config(
    query: &str,
    by: &str,
    limit: usize,
    use_regex: bool,
    cfg: &Config,
) -> anyhow::Result<Vec<Package>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let encoded = encoded.replace('+', "%20");
    let rpc = aur_rpc_with_config(cfg);
    let url = format!("{}/search/{}?by={}", rpc, encoded, by);
    tracing::debug!(url=%url, "aur blocking request");
    let cfg_timeout = cfg.search.timeout_secs.max(1);
    // Shared cached client per timeout — honors the caller's config, no extra
    // `Config::load()` file IO on the search hot path.
    let client = blocking_client_for_timeout(cfg_timeout)?;
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            format!("pakmap/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .with_context(|| format!("AUR request failed {}", url))?;
    if !resp.status().is_success() {
        anyhow::bail!("AUR RPC returned HTTP {}", resp.status());
    }
    let text = crate::safety::net::read_body_capped(resp, crate::safety::net::RPC_BODY_CAP_BYTES)
        .with_context(|| format!("AUR RPC body read failed {}", url))?;
    // Same CWE-248 fix on the blocking path (TUI).
    let parsed: AurResponse = serde_json::from_str(&text).with_context(|| {
        format!(
            "AUR json parse failed: {}",
            crate::safety::text::error_snippet(&text, 500)
        )
    })?;
    if parsed.type_ == "error" {
        let msg = parsed
            .error
            .unwrap_or_else(|| "unknown AUR error".to_string());
        anyhow::bail!("AUR error: {}", msg);
    }
    let mut results: Vec<Package> = parsed.results.into_iter().map(Package::from).collect();
    if use_regex {
        let re = regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()?;
        results.retain(|p| {
            re.is_match(&p.name)
                || p.description
                    .as_deref()
                    .map(|d| re.is_match(d))
                    .unwrap_or(false)
        });
    }
    sort_and_truncate(&mut results, limit);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_client_builds_without_panic() {
        // Builder-only: no network. Must be Ok, never panic (old expect()).
        let cfg = Config::default();
        let timeout = cfg.search.timeout_secs.max(1);
        assert!(blocking_client_for_timeout(timeout).is_ok());
        // Second call serves the per-timeout cache.
        assert!(blocking_client_for_timeout(timeout).is_ok());
    }

    #[test]
    fn tuned_client_builders_build_offline() {
        // Building does TLS init but no network — must be Ok both sides with
        // the shared tuning (gzip feature, connect bound, nodelay).
        assert!(client_builder(15).build().is_ok());
        assert!(blocking_client_builder(15).build().is_ok());
        // Degenerate timeouts still bound to >= 1s inside the builders.
        assert!(client_builder(0).build().is_ok());
        assert!(blocking_client_builder(0).build().is_ok());
    }

    #[test]
    fn warm_origin_derived_from_rpc_url() {
        assert_eq!(
            aur_warm_origin("https://aur.archlinux.org/rpc/v5").as_deref(),
            Some("https://aur.archlinux.org/")
        );
        // Custom origin keeps scheme+port (local mirror cases).
        assert_eq!(
            aur_warm_origin("http://127.0.0.1:9090/rpc/v5").as_deref(),
            Some("http://127.0.0.1:9090/")
        );
        assert_eq!(
            aur_warm_origin("https://aur.debian.mirror.example/rpc/v5/").as_deref(),
            Some("https://aur.debian.mirror.example/")
        );
        // Unparsable / non-http / empty-host: no warm, never a panic.
        assert!(aur_warm_origin("not a url").is_none());
        assert!(aur_warm_origin("ftp://example.org/rpc/v5").is_none());
        assert!(aur_warm_origin("https://").is_none());
    }

    #[test]
    fn nan_popularity_normalizes_to_none() {
        let aur = AurPackage {
            name: "x".into(),
            package_base: None,
            version: "1".into(),
            description: None,
            url: None,
            num_votes: Some(3),
            popularity: Some(f64::NAN),
            out_of_date: None,
            maintainer: None,
            first_submitted: None,
            last_modified: None,
            depends: None,
        };
        let pkg = Package::from(aur);
        assert_eq!(pkg.popularity, None);
        assert_eq!(pkg.votes, Some(3));
    }
}
