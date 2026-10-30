use serde::{Deserialize, Serialize};

/// One available update: installed `old_version` -> sync/AUR `new_version`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PkgUpdate {
    pub name: String,
    pub old_version: String,
    pub new_version: String,
    pub repo: String,
    pub out_of_date: bool,
}

/// Repo updates via libalpm: localdb vs syncdbs with `alpm::vercmp`.
/// Graceful: missing syncdbs (never `Sy`) yields empty, never an error.
pub fn check_repo_updates() -> anyhow::Result<Vec<PkgUpdate>> {
    let cfg = crate::search::repo::get_cached_config().or_else(|_| {
        pacmanconf::Config::new().map_err(|e| anyhow::anyhow!("pacmanconf failed: {e:?}"))
    })?;
    check_repo_updates_with_config(&cfg)
}

pub fn check_repo_updates_with_config(cfg: &pacmanconf::Config) -> anyhow::Result<Vec<PkgUpdate>> {
    let db_path = if cfg.db_path.is_empty() {
        "/var/lib/pacman".to_string()
    } else {
        cfg.db_path.clone()
    };
    let root_dir = if cfg.root_dir.is_empty() {
        "/".to_string()
    } else {
        cfg.root_dir.clone()
    };
    let handle = alpm::Alpm::new(root_dir.as_str(), db_path.as_str())
        .map_err(|e| anyhow::anyhow!("alpm init failed: {e:?}"))?;
    for repo in &cfg.repos {
        let _ = handle.register_syncdb(repo.name.as_str(), alpm::SigLevel::USE_DEFAULT);
    }
    let syncdbs = handle.syncdbs();
    if syncdbs.is_empty() {
        return Ok(vec![]);
    }
    let localdb = handle.localdb();
    let mut out = Vec::new();
    for pkg in localdb.pkgs().iter() {
        let name = pkg.name();
        let local_ver = pkg.version().to_string();
        // First syncdb hit wins (repo order = pacman priority).
        for db in syncdbs.iter() {
            let Ok(sync_pkg) = db.pkg(name) else {
                continue;
            };
            let sync_ver = sync_pkg.version().to_string();
            if alpm::vercmp(sync_ver.clone(), local_ver.clone()) == std::cmp::Ordering::Greater {
                out.push(PkgUpdate {
                    name: name.to_string(),
                    old_version: local_ver.clone(),
                    new_version: sync_ver,
                    repo: db.name().to_string(),
                    out_of_date: false,
                });
            }
            break;
        }
    }
    out.sort_by(|a, b| a.repo.cmp(&b.repo).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// AUR updates: foreign pkgs (installed, not in any syncdb) vs RPC `/info`.
/// Batched (50/request), timeout from config, graceful offline -> empty.
/// `resilience-failure`: bounded calls, fail-open to repo-only results.
pub async fn check_aur_updates(
    client: &reqwest::Client,
    cfg: &crate::config::Config,
) -> Vec<PkgUpdate> {
    let foreign = foreign_packages().unwrap_or_default();
    if foreign.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for chunk in foreign.chunks(50) {
        match fetch_aur_info(client, cfg, chunk).await {
            Ok(infos) => {
                for (name, local_ver) in &infos.requested {
                    if let Some(remote) = infos.versions.get(name)
                        && alpm::vercmp(remote.clone(), local_ver.clone())
                            == std::cmp::Ordering::Greater
                    {
                        out.push(PkgUpdate {
                            name: name.clone(),
                            old_version: local_ver.clone(),
                            new_version: remote.clone(),
                            repo: "aur".to_string(),
                            out_of_date: infos.out_of_date.get(name).copied().flatten().is_some(),
                        });
                    }
                }
            }
            Err(e) => {
                tracing::warn!(err=?e, "AUR update check failed, continuing repo-only");
                break;
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

struct AurInfoBatch {
    requested: Vec<(String, String)>,
    versions: std::collections::HashMap<String, String>,
    out_of_date: std::collections::HashMap<String, Option<i64>>,
}

async fn fetch_aur_info(
    client: &reqwest::Client,
    cfg: &crate::config::Config,
    chunk: &[(String, String)],
) -> anyhow::Result<AurInfoBatch> {
    let rpc = cfg.aur_rpc_url();
    let mut url = format!("{}/info?", rpc);
    for (i, (name, _)) in chunk.iter().enumerate() {
        if i > 0 {
            url.push('&');
        }
        url.push_str("arg[]=");
        url.push_str(&url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>());
    }
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("AUR info request failed: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("AUR RPC HTTP {}", resp.status());
    }
    let text = resp
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("AUR info read failed: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("AUR info parse: {e}"))?;
    let mut versions = std::collections::HashMap::new();
    let mut out_of_date = std::collections::HashMap::new();
    if let Some(arr) = v.get("results").and_then(|r| r.as_array()) {
        for item in arr {
            if let Some(name) = item.get("Name").and_then(|n| n.as_str()) {
                if let Some(ver) = item.get("Version").and_then(|x| x.as_str()) {
                    versions.insert(name.to_string(), ver.to_string());
                }
                let ood = item.get("OutOfDate").and_then(|x| x.as_i64());
                out_of_date.insert(name.to_string(), ood);
            }
        }
    }
    Ok(AurInfoBatch {
        requested: chunk.to_vec(),
        versions,
        out_of_date,
    })
}

/// Installed pkgs absent from every syncdb = AUR/foreign (name, version).
fn foreign_packages() -> anyhow::Result<Vec<(String, String)>> {
    let cfg = crate::search::repo::get_cached_config().or_else(|_| {
        pacmanconf::Config::new().map_err(|e| anyhow::anyhow!("pacmanconf failed: {e:?}"))
    })?;
    let db_path = if cfg.db_path.is_empty() {
        "/var/lib/pacman".to_string()
    } else {
        cfg.db_path.clone()
    };
    let root_dir = if cfg.root_dir.is_empty() {
        "/".to_string()
    } else {
        cfg.root_dir.clone()
    };
    let handle = alpm::Alpm::new(root_dir.as_str(), db_path.as_str())
        .map_err(|e| anyhow::anyhow!("alpm init failed: {e:?}"))?;
    for repo in &cfg.repos {
        let _ = handle.register_syncdb(repo.name.as_str(), alpm::SigLevel::USE_DEFAULT);
    }
    let syncdbs = handle.syncdbs();
    let localdb = handle.localdb();
    let mut out = Vec::new();
    for pkg in localdb.pkgs().iter() {
        let name = pkg.name();
        let mut in_sync = false;
        for db in syncdbs.iter() {
            if db.pkg(name).is_ok() {
                in_sync = true;
                break;
            }
        }
        if !in_sync {
            out.push((name.to_string(), pkg.version().to_string()));
        }
    }
    Ok(out)
}

/// Blocking AUR updates for TUI worker threads (no tokio runtime there).
/// Same batching/graceful logic as the async version.
pub fn check_aur_updates_blocking(cfg: &crate::config::Config) -> Vec<PkgUpdate> {
    check_aur_updates_blocking_cached(cfg)
}

/// B5/B6 (v0.9.1): the memo's richer payload. `checked_at_epoch` is the
/// wall-clock of the last executed check (the Updates screen's
/// "AUR checked Xm ago" chip); `aur_err` is set only when the check
/// referenced the AUR and failed mid-flight — fail-open stays untouched
/// (`updates` remains best-effort and partial on mid-chunk failure).
#[derive(Clone, Debug)]
pub struct AurUpdateReport {
    pub updates: Vec<PkgUpdate>,
    pub checked_at_epoch: Option<u64>,
    pub aur_err: Option<String>,
}

fn epoch_secs() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Memoized wrapper (`caching` skill: TTL + explicit invalidate).
/// Tab-switching back to Updates within the TTL reuses the last fetch
/// instead of hammering the AUR RPC; `F5`/upgrade/downgrade paths call
/// `invalidate_aur_memo()` for a fresh check. The two plain-Vec entry
/// points keep their pre-v0.9.1 contract for CLI/TUI callers; the GUI
/// takes the richer `check_aur_updates_blocking_report`.
pub fn check_aur_updates_blocking_cached(cfg: &crate::config::Config) -> Vec<PkgUpdate> {
    check_aur_updates_blocking_report(cfg).updates
}

pub fn check_aur_updates_blocking_report(cfg: &crate::config::Config) -> AurUpdateReport {
    const TTL_SECS: u64 = 300;
    let now = std::time::Instant::now();
    let memo = AUR_MEMO.get_or_init(|| std::sync::Mutex::new(AurMemoState::default()));
    if let Ok(guard) = memo.lock()
        && let Some(at) = guard.at
        // Cache hits include empty lists: a failed/offline fetch backs off
        // for the TTL instead of timing out on every Tab press. F5 forces
        // a fresh check via invalidate_aur_memo().
        && now.duration_since(at).as_secs() < TTL_SECS
    {
        return AurUpdateReport {
            updates: guard.updates.clone(),
            checked_at_epoch: guard.checked_at_epoch,
            aur_err: guard.aur_err.clone(),
        };
    }
    let fresh = check_aur_updates_blocking_live(cfg);
    if let Ok(mut guard) = memo.lock() {
        *guard = AurMemoState {
            at: Some(now),
            checked_at_epoch: fresh.checked_at_epoch,
            updates: fresh.updates.clone(),
            aur_err: fresh.aur_err.clone(),
        };
    }
    fresh
}

/// Forget the memoized AUR update list (refresh/upgrade/clean paths).
pub fn invalidate_aur_memo() {
    let memo = AUR_MEMO.get_or_init(|| std::sync::Mutex::new(AurMemoState::default()));
    if let Ok(mut guard) = memo.lock() {
        *guard = AurMemoState::default();
    }
}

#[derive(Default)]
struct AurMemoState {
    at: Option<std::time::Instant>,
    checked_at_epoch: Option<u64>,
    updates: Vec<PkgUpdate>,
    aur_err: Option<String>,
}

static AUR_MEMO: std::sync::OnceLock<std::sync::Mutex<AurMemoState>> = std::sync::OnceLock::new();

fn check_aur_updates_blocking_live(cfg: &crate::config::Config) -> AurUpdateReport {
    let mut report = AurUpdateReport {
        updates: Vec::new(),
        checked_at_epoch: epoch_secs(),
        aur_err: None,
    };
    let foreign = foreign_packages().unwrap_or_default();
    if foreign.is_empty() {
        // Nothing to check: the memo carries the fresh timestamp so the
        // "checked Xm ago" chip stays truthful (an empty result is a
        // conclusion, not a degradation).
        return report;
    }
    let client = crate::search::aur::blocking_client_builder(cfg.search.timeout_secs).build();
    let Ok(client) = client else {
        report.aur_err = Some("AUR client initialisation failed".to_string());
        return report;
    };
    let rpc = cfg.aur_rpc_url();
    for chunk in foreign.chunks(50) {
        let mut url = format!("{rpc}/info?");
        for (i, (name, _)) in chunk.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str("arg[]=");
            url.push_str(
                &url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>(),
            );
        }
        let resp = client.get(&url).send();
        let Ok(resp) = resp else {
            tracing::warn!("AUR update check offline, repo-only");
            report.aur_err = Some("AUR RPC unreachable (offline?)".to_string());
            break;
        };
        if !resp.status().is_success() {
            report.aur_err = Some(format!("AUR RPC returned HTTP {}", resp.status().as_u16()));
            break;
        }
        let Ok(text) =
            crate::safety::net::read_body_capped(resp, crate::safety::net::RPC_BODY_CAP_BYTES)
        else {
            report.aur_err = Some("AUR RPC payload too large or unreadable".to_string());
            break;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            report.aur_err = Some("AUR RPC returned a malformed payload".to_string());
            break;
        };
        let mut versions = std::collections::HashMap::new();
        let mut ood = std::collections::HashMap::new();
        if let Some(arr) = v.get("results").and_then(|r| r.as_array()) {
            for item in arr {
                if let Some(name) = item.get("Name").and_then(|n| n.as_str()) {
                    if let Some(ver) = item.get("Version").and_then(|x| x.as_str()) {
                        versions.insert(name.to_string(), ver.to_string());
                    }
                    ood.insert(
                        name.to_string(),
                        item.get("OutOfDate").and_then(|x| x.as_i64()),
                    );
                }
            }
        }
        for (name, local_ver) in chunk {
            if let Some(remote) = versions.get(name)
                && alpm::vercmp(remote.clone(), local_ver.clone()) == std::cmp::Ordering::Greater
            {
                report.updates.push(PkgUpdate {
                    name: name.clone(),
                    old_version: local_ver.clone(),
                    new_version: remote.clone(),
                    repo: "aur".to_string(),
                    out_of_date: ood.get(name).copied().flatten().is_some(),
                });
            }
        }
    }
    report.updates.sort_by(|a, b| a.name.cmp(&b.name));
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All AUR-memo tests share the process-global memo static; serialize the
    /// ones that touch it so `cargo test`'s thread harness can't interleave
    /// two memo checks between one test's invalidate and its assertion
    /// (nextest runs each test in its own process, uncontended there).
    fn aur_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn repo_updates_offline_never_errors() {
        // On Arch: returns list (maybe empty); off-Arch without pacman DB:
        // alpm init succeeds but no syncdbs -> empty. Must never panic/Err-fatal.
        let r = check_repo_updates();
        assert!(r.is_ok());
    }

    #[test]
    fn vercmp_orders_versions() {
        assert_eq!(
            alpm::vercmp("2-1".to_string(), "1-1".to_string()),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            alpm::vercmp("1-1".to_string(), "1-1".to_string()),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn aur_memo_invalidate_is_safe() {
        let _guard = aur_test_lock().lock().unwrap();
        invalidate_aur_memo();
        // Cached wrapper works and second call hits the memo (same content).
        // Live network: hermetic on content equality, skip fully offline.
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let cfg = crate::config::Config::default();
        let a = check_aur_updates_blocking_cached(&cfg);
        let b = check_aur_updates_blocking_cached(&cfg);
        assert_eq!(a, b);
        invalidate_aur_memo();
    }

    #[test]
    fn aur_report_memo_carries_epoch_across_hit() {
        let _guard = aur_test_lock().lock().unwrap();
        invalidate_aur_memo();
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let cfg = crate::config::Config::default();
        let a = check_aur_updates_blocking_report(&cfg);
        let b = check_aur_updates_blocking_report(&cfg);
        // Memo hit within the TTL: the reported "checked at" is stable, not
        // re-stamped per call (B6: the chip reflects the actual check time).
        assert_eq!(a.checked_at_epoch, b.checked_at_epoch);
        assert_eq!(a.updates, b.updates);
        // Contract: the report carries an epoch even for an empty update
        // list (an empty result is a conclusion, not a degradation).
        assert!(a.checked_at_epoch.is_some() || !a.updates.is_empty());
        invalidate_aur_memo();
    }

    #[test]
    fn aur_report_err_is_none_or_a_known_phrase() {
        let _guard = aur_test_lock().lock().unwrap();
        invalidate_aur_memo();
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let cfg = crate::config::Config::default();
        let r = check_aur_updates_blocking_report(&cfg);
        // B5 invariant: fail-open stays — never an Err to the caller, and a
        // set reason is always one of the human phrases from the live check.
        if let Some(err) = &r.aur_err {
            assert!(
                err.starts_with("AUR ")
                    && (err.contains("unreachable")
                        || err.contains("HTTP")
                        || err.contains("read failed")
                        || err.contains("malformed payload")
                        || err.contains("initialisation failed")),
                "unexpected aur_err phrasing: {err}"
            );
        }
        invalidate_aur_memo();
    }
}
