//! Flatpak backend (P1: detect + list installed).
//!
//! Uses `flatpak --json` (stable machine format) instead of TSV columns:
//! TSV breaks on empty versions (double-tab) and wrapped descriptions, JSON
//! does not. TSV parser is kept as fallback for old flatpak without `--json`.
//!
//! No sudo: flatpak is user/system scoped, never needs `sudo`. Missing binary
//! or missing remote is fail-open (empty / Err with hint), never a panic.
//! No `expect()`/`unwrap()` in production paths.

use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::Package;

/// JSON row from `flatpak list --app --json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct FlatpakListRow {
    #[serde(default, rename = "application_id")]
    application_id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    arch: Option<String>,
}

/// `flatpak` on PATH probe (pure-Rust, no `which` binary needed).
pub fn has_flatpak() -> bool {
    crate::process::has_tool("flatpak")
}

/// v0.10.2 phase 2 (E1 dedupe): the single owner of the missing-binary
/// hint — five bail sites used to carry this string verbatim; the lib test
/// pins the byte-shape (drift-pin: the plan's literal, the shipped truth).
pub fn flatpak_missing_bail() -> anyhow::Result<()> {
    anyhow::bail!("flatpak not found — install flatpak to manage flatpak apps")
}

/// True when `remote` appears in `flatpak remote-list --columns=name`.
/// Missing binary -> false (fail-open, caller emits hint).
pub fn flatpak_remote_exists(remote: &str) -> bool {
    let remote = remote.trim();
    if remote.is_empty() || !has_flatpak() {
        return false;
    }
    match run_flatpak(&["remote-list", "--columns=name"], Duration::from_secs(10)) {
        Some(out) => parse_remote_list(&out).iter().any(|r| r == remote),
        None => false,
    }
}

/// Live list: `flatpak list --app --json`, 15s bound. Missing binary is an Err
/// with install hint (caller maps to exit 1 / TUI message, not a panic).
/// Empty install set is Ok(empty).
pub fn list_installed() -> anyhow::Result<Vec<Package>> {
    if !has_flatpak() {
        flatpak_missing_bail()?;
    }
    match run_flatpak(&["list", "--app", "--json"], Duration::from_secs(15)) {
        Some(out) => Ok(parse_flatpak_list_json(&out)),
        None => Ok(vec![]),
    }
}

/// Cached inventory (perf P4): the installed set changes only via
/// install/remove/update, all of which call `invalidate_flatpak_list_memo()`.
/// 60s TTL covers rapid Tab-switching and `--stats` re-entry without
/// re-spawning. Same content as [`list_installed`], maybe 60s stale.
pub fn list_installed_cached() -> anyhow::Result<Vec<Package>> {
    if let Some(hit) = list_memo_get() {
        return Ok(hit);
    }
    let fresh = list_installed()?;
    list_memo_store(fresh.clone());
    Ok(fresh)
}

/// Forget the cached inventory (install/remove/upgrade/clean paths — always
/// next to `invalidate_flatpak_memo()`).
pub fn invalidate_flatpak_list_memo() {
    let memo = FLATPAK_LIST_MEMO.get_or_init(|| std::sync::Mutex::new((None, Vec::new())));
    if let Ok(mut guard) = memo.lock() {
        *guard = (None, Vec::new());
    }
}

fn list_memo_get() -> Option<Vec<Package>> {
    const TTL_SECS: u64 = 60;
    let now = std::time::Instant::now();
    let memo = FLATPAK_LIST_MEMO.get_or_init(|| std::sync::Mutex::new((None, Vec::new())));
    if let Ok(guard) = memo.lock()
        && let (Some(at), ref cached) = *guard
        && now.duration_since(at).as_secs() < TTL_SECS
    {
        return Some(cached.clone());
    }
    None
}

fn list_memo_store(fresh: Vec<Package>) {
    let memo = FLATPAK_LIST_MEMO.get_or_init(|| std::sync::Mutex::new((None, Vec::new())));
    if let Ok(mut guard) = memo.lock() {
        *guard = (Some(std::time::Instant::now()), fresh);
    }
}

static FLATPAK_LIST_MEMO: std::sync::OnceLock<
    std::sync::Mutex<(Option<std::time::Instant>, Vec<Package>)>,
> = std::sync::OnceLock::new();

/// Run flatpak with a wall-clock bound via the shared runner (perf P2).
/// None covers timeout/spawn failure (fail-open); only successful stdout
/// is returned.
fn run_flatpak(args: &[&str], timeout: Duration) -> Option<String> {
    let out = crate::process::output_bounded("flatpak", args, timeout)?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `flatpak list --app --json`. Defensive: garbage -> empty, rows without
/// `application_id` skipped, missing version -> `"?"` (flatpak leaves version
/// empty for runtimes/drivers, e.g. VAAPI).
pub fn parse_flatpak_list_json(text: &str) -> Vec<Package> {
    let rows: Vec<FlatpakListRow> = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(app_id) = r
            .application_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let version = r
            .version
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "?".to_string());
        let mut pkg = Package::minimal(app_id, version, "flatpak".to_string());
        pkg.installed = true;
        pkg.description = r.name.or(r.description);
        pkg.arch = r.arch;
        pkg.url = r.origin.map(|o| format!("flatpak:{o}"));
        if let Some(b) = r.branch {
            // Branch is not part of Package; stash in version suffix when
            // non-stable so rows stay distinguishable in the list.
            if b != "stable" && !b.trim().is_empty() {
                pkg.version = format!("{} ({})", pkg.version, b.trim());
            }
        }
        out.push(pkg);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Fallback TSV parser for `flatpak list --columns=application,version,branch,origin`
/// (tab-separated, no header). Empty version field preserved via `\t` split —
/// never split on whitespace here.
pub fn parse_flatpak_list_columns(output: &str) -> Vec<Package> {
    let mut out = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Application") {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.is_empty() {
            continue;
        }
        let app_id = cols.first().map(|s| s.trim()).unwrap_or("");
        if app_id.is_empty() {
            continue;
        }
        let version = cols
            .get(1)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("?");
        let branch = cols.get(2).map(|s| s.trim()).unwrap_or("");
        let origin = cols.get(3).map(|s| s.trim()).unwrap_or("");
        let mut pkg = Package::minimal(
            app_id.to_string(),
            version.to_string(),
            "flatpak".to_string(),
        );
        pkg.installed = true;
        if !origin.is_empty() {
            pkg.url = Some(format!("flatpak:{origin}"));
        }
        if !branch.is_empty() && branch != "stable" {
            pkg.version = format!("{version} ({branch})");
        }
        out.push(pkg);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Parse `flatpak remote-list --columns=name` (one name per line, TSV or plain).
pub fn parse_remote_list(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|l| l.split('\t').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty() && s != "Name")
        .collect()
}

/// Validate a flatpak application ID (reverse-DNS, e.g. `org.mozilla.firefox`).
/// Same traversal/NUL policy as the pacman paths; dots required (at least
/// one) since bare words are never valid flatpak IDs. Returns trimmed id.
pub fn validate_app_id(id: &str) -> anyhow::Result<String> {
    let id = id.trim();
    if id.is_empty() {
        anyhow::bail!("empty application id");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        anyhow::bail!("invalid application id: {:?}", id);
    }
    if !id.contains('.') {
        anyhow::bail!(
            "invalid application id (need reverse-DNS like org.mozilla.firefox): {:?}",
            id
        );
    }
    if id.starts_with('.') || id.starts_with('-') || id.ends_with('.') || id.ends_with('-') {
        anyhow::bail!("invalid application id: {:?}", id);
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        anyhow::bail!("invalid application id: {:?}", id);
    }
    Ok(id.to_string())
}

/// Args for `flatpak install` after the binary name, extracted for unit
/// testing without spawning. No sudo — flatpak never needs it. `-y` only with
/// noconfirm (scripts / PAKMAP_NOCONFIRM=1); otherwise flatpak asks.
pub fn install_command_args(remote: &str, id: &str, noconfirm: bool) -> Vec<String> {
    let mut args = vec!["install".to_string(), remote.to_string(), id.to_string()];
    if noconfirm {
        args.push("-y".to_string());
    }
    args
}

/// Args for `flatpak uninstall`. Same `-y` policy as install.
pub fn remove_command_args(id: &str, noconfirm: bool) -> Vec<String> {
    let mut args = vec!["uninstall".to_string(), id.to_string()];
    if noconfirm {
        args.push("-y".to_string());
    }
    args
}

/// Args for unused-runtime cleanup (`flatpak uninstall --unused`).
pub fn unused_command_args(noconfirm: bool) -> Vec<String> {
    let mut args = vec!["uninstall".to_string(), "--unused".to_string()];
    if noconfirm {
        args.push("-y".to_string());
    }
    args
}

/// True when `id` is installed (`flatpak info` probe, 10s bound).
/// Missing binary -> false (fail-open).
pub fn is_flatpak_installed(id: &str) -> bool {
    if !has_flatpak() || validate_app_id(id).is_err() {
        return false;
    }
    crate::process::output_bounded("flatpak", &["info", id.trim()], Duration::from_secs(10))
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Install a flatpak app with inherited stdio so the user sees progress.
/// No sudo. Caller owns confirmation; readonly refused here (defense in depth).
pub fn install_flatpak_package(id: &str, cfg: &crate::config::Config) -> anyhow::Result<()> {
    let id = validate_app_id(id)?;
    if cfg.is_readonly() {
        anyhow::bail!("refusing to install: readonly mode is enabled");
    }
    if !has_flatpak() {
        flatpak_missing_bail()?;
    }
    let remote = cfg.flatpak_remote_name();
    if !flatpak_remote_exists(&remote) {
        anyhow::bail!(
            "flatpak remote '{remote}' not found — add it or set [behavior] flatpak_remote"
        );
    }
    let args = install_command_args(&remote, &id, cfg.flatpak_noconfirm());
    let refs = arg_refs(&args);
    let status = Command::new("flatpak")
        .args(&refs)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run flatpak install: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "flatpak install failed with exit {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Uninstall a flatpak app. Same safety/stdio contract as install.
pub fn remove_flatpak_package(id: &str, cfg: &crate::config::Config) -> anyhow::Result<()> {
    let id = validate_app_id(id)?;
    if cfg.is_readonly() {
        anyhow::bail!("refusing to remove: readonly mode is enabled");
    }
    if !has_flatpak() {
        flatpak_missing_bail()?;
    }
    if !is_flatpak_installed(&id) {
        anyhow::bail!("flatpak app '{id}' is not installed");
    }
    let args = remove_command_args(&id, cfg.flatpak_noconfirm());
    let refs = arg_refs(&args);
    let status = Command::new("flatpak")
        .args(&refs)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run flatpak uninstall: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "flatpak uninstall failed with exit {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// E3 dedupe (v0.10.2 phase 2): borrow the owned arg strings for
/// `Command::args` — four *_command_args sites carried the same map inland.
fn arg_refs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// Remove unused runtimes (`flatpak uninstall --unused`). No preview exists
/// server-side, so callers confirm first (same TTY/exit-2 contract as clean).
pub fn clean_unused_flatpak(cfg: &crate::config::Config) -> anyhow::Result<()> {
    if cfg.is_readonly() {
        anyhow::bail!("refusing to clean: readonly mode is enabled");
    }
    if !has_flatpak() {
        flatpak_missing_bail()?;
    }
    let args = unused_command_args(cfg.flatpak_noconfirm());
    let refs = arg_refs(&args);
    let status = Command::new("flatpak")
        .args(&refs)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run flatpak uninstall --unused: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "flatpak uninstall --unused failed with exit {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// JSON row from `flatpak search --json`.
#[derive(Debug, Clone, Deserialize)]
struct FlatpakSearchRow {
    #[serde(default, rename = "application_id")]
    application_id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    remotes: Option<String>,
}

/// Parse `flatpak search --json`. Pure, never panics: garbage -> empty, rows
/// without `application_id` skipped, empty version -> `"?"`. Remote relevance
/// order preserved (no re-sort); `limit` truncates (0 = no limit).
pub fn parse_flatpak_search_json(text: &str, limit: usize) -> Vec<Package> {
    let rows: Vec<FlatpakSearchRow> = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut out = Vec::new();
    for r in rows {
        let Some(app_id) = r
            .application_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let version = r
            .version
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "?".to_string());
        let mut pkg = Package::minimal(app_id, version, "flatpak".to_string());
        pkg.installed = false;
        pkg.description = r.description.or(r.name);
        if let Some(rem) = r
            .remotes
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            pkg.url = Some(format!("flatpak:{rem}"));
        }
        if let Some(b) = r
            .branch
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            && b != "stable"
        {
            pkg.version = format!("{} ({})", pkg.version, b);
        }
        out.push(pkg);
        if limit != 0 && out.len() >= limit {
            break;
        }
    }
    out
}

/// Live search: `flatpak search <query> --json`, bounded by
/// `cfg.search.timeout_secs`. Fail-open `Ok(empty)` when disabled, binary
/// missing, query empty, or timeout — mirrors AUR graceful degradation.
/// `use_regex` filters client-side on app-id + description (flatpak has no
/// server-side regex); invalid regex is an Err like the AUR path.
pub fn search_flatpak_with_config(
    query: &str,
    limit: usize,
    use_regex: bool,
    cfg: &crate::config::Config,
) -> anyhow::Result<Vec<Package>> {
    if !cfg.flatpak_enabled() || !has_flatpak() || query.trim().is_empty() {
        return Ok(vec![]);
    }
    let trimmed = query.trim();
    // Perf W (fast path): the searched index is a local file —
    // `<install>/appstream/<remote>/<arch>/active/appstream.xml` — so read it
    // directly instead of paying flatpak's full index load on every search
    // (~3.4s per call measured; direct parse ≈ 150-400ms budget).
    // `None` = no local cache file for the configured remote → spawn path.
    let mut pkgs = match appstream_search(trimmed, limit, &cfg.flatpak_remote_name()) {
        Some(pkgs) => pkgs,
        None => {
            let timeout = Duration::from_secs(cfg.search.timeout_secs.max(1));
            let Some(out) = run_flatpak(&["search", trimmed, "--json"], timeout) else {
                return Ok(vec![]);
            };
            parse_flatpak_search_json(&out, limit)
        }
    };
    if use_regex {
        let re = regex::RegexBuilder::new(query.trim())
            .case_insensitive(true)
            .build()?;
        pkgs.retain(|p| {
            re.is_match(&p.name)
                || p.description
                    .as_deref()
                    .map(|d| re.is_match(d))
                    .unwrap_or(false)
        });
        if limit != 0 && pkgs.len() > limit {
            pkgs.truncate(limit);
        }
    }
    Ok(pkgs)
}

/// Args for `flatpak update <id>`. `-y` only with noconfirm.
pub fn update_command_args(id: &str, noconfirm: bool) -> Vec<String> {
    let mut args = vec!["update".to_string(), id.to_string()];
    if noconfirm {
        args.push("-y".to_string());
    }
    args
}

// ---------------------------------------------------------------------------
// Perf W: direct appstream cache search — no `flatpak` spawn.
//
// `flatpak search` reloads the full Flathub index on every invocation
// (measured 3.4s warm on Flathub's ~49MB XML). The same index is a plain
// local file — `<install>/appstream/<remote>/<arch>/active/appstream.xml` —
// so search reads+scans it directly and stops at the limit. Fallback
// (resilience): files count as absent when the remote is not cached, the
// layout changed, or the file exceeds `MAX_APPSTREAM_BYTES`; then the old
// bounded `flatpak search --json` spawn runs (fail-open chain). Flatpak
// documents the appstream auto-refresh as "only when at least a day old",
// so serving our own copy is never fresher than flatpak itself would.
// `.gz`-only layouts keep working via the spawn fallback (no gzip dep).
// ---------------------------------------------------------------------------

/// Hard cap on appstream XML bytes we are willing to scan before treating a
/// cache file as absent (bound resource use; oversized caches go to the
/// spawn fallback).
fn within_appstream_cap(bytes: u64) -> bool {
    const MAX_APPSTREAM_BYTES: u64 = 256 * 1024 * 1024;
    bytes <= MAX_APPSTREAM_BYTES
}

/// Roots that can host `appstream/<remote>/<arch>/active/appstream.xml`:
/// XDG data dir (user install) plus the system install. Tests may override
/// all roots with `PAKMAP_FLATPAK_APPSTREAM_ROOT` (chaos shims); an empty
/// value is ignored.
fn appstream_cache_roots() -> Vec<std::path::PathBuf> {
    if let Ok(root) = std::env::var("PAKMAP_FLATPAK_APPSTREAM_ROOT")
        && !root.trim().is_empty()
    {
        return vec![std::path::PathBuf::from(root)];
    }
    let mut roots = Vec::new();
    if let Some(xdg) = dirs::data_dir() {
        roots.push(xdg);
    }
    roots.push(std::path::PathBuf::from("/var/lib/flatpak"));
    roots
}

/// Every `active/appstream.xml` for `remote` under `root` (arch-agnostic:
/// flatpak lays out one subdir per arch under `appstream/<remote>/`).
fn appstream_cache_files(root: &std::path::Path, remote: &str) -> Vec<std::path::PathBuf> {
    let remote_dir = root.join("appstream").join(remote);
    let Ok(entries) = std::fs::read_dir(&remote_dir) else {
        return vec![];
    };
    let mut out: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path().join("active").join("appstream.xml"))
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out
}

/// Fast-path entry. `Some` = local cache consulted (caller must not spawn);
/// `None` = no usable cache file anywhere (caller falls back to the bounded
/// `flatpak search` spawn). Timing is reported via the shared timer log.
pub fn appstream_search(query: &str, limit: usize, remote: &str) -> Option<Vec<Package>> {
    let roots = appstream_cache_roots();
    let started = crate::timing::start("flatpak-appstream");
    let found = appstream_search_roots(query, limit, remote, &roots);
    started.finish(found.as_ref().map_or(0, |v| v.len()));
    found
}

/// Same over explicit roots (testable; no env reads, no timing log).
/// Dedupes by app-id across roots, first hit wins; `limit` caps the merge
/// (0 = no limit).
pub fn appstream_search_roots(
    query: &str,
    limit: usize,
    remote: &str,
    roots: &[std::path::PathBuf],
) -> Option<Vec<Package>> {
    let remote = remote.trim();
    if remote.is_empty() {
        return None;
    }
    let mut merged: Vec<Package> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut consulted_any = false;
    for root in roots {
        for file in appstream_cache_files(root, remote) {
            let size = std::fs::metadata(&file).map_or(0, |m| m.len());
            if !within_appstream_cap(size) {
                continue;
            }
            consulted_any = true;
            if let Ok(text) = std::fs::read_to_string(&file) {
                for pkg in parse_appstream_search(&text, query, limit, remote) {
                    if seen.insert(pkg.name.clone()) {
                        merged.push(pkg);
                        if limit != 0 && merged.len() >= limit {
                            return Some(merged);
                        }
                    }
                }
            }
        }
    }
    if consulted_any { Some(merged) } else { None }
}

/// Search one appstream XML. Pure, never panics: garbage/unclosed tails
/// return what was collected so far. Matches (ASCII-case-folded) on id +
/// name + summary + keywords; preserves file order (`flatpak search` is
/// relevance-ranked in the JSON path, ours is deterministic — parity of
/// *fields* is the contract, ordering stays file order). `limit` truncates
/// (0 = no limit). Version comes from the first `<release version="...">`.
pub fn parse_appstream_search(xml: &str, query: &str, limit: usize, remote: &str) -> Vec<Package> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return vec![];
    }
    let mut out: Vec<Package> = Vec::new();
    let mut cursor = 0usize;
    while limit == 0 || out.len() < limit {
        let Some(open_rel) = xml[cursor..].find("<component") else {
            break;
        };
        let body_start = cursor + open_rel + "<component".len();
        // Unclosed trailing component keeps what was collected, like the
        // JSON parsers do on truncated input.
        let Some(close_rel) = xml[body_start..].find("</component>") else {
            break;
        };
        let block = &xml[body_start..body_start + close_rel];
        cursor = body_start + close_rel + "</component>".len();
        let Some(id) = tag_first_text(block, "id") else {
            continue;
        };
        // Display parity with `flatpak search`: `<id>org.x.App.desktop` rows
        // address the same app-id as plain `org.x.App`.
        let id = if let Some(stripped) = id.strip_suffix(".desktop") {
            stripped
        } else {
            id
        };
        if id.trim().is_empty() {
            continue;
        }
        let id = id.trim();
        let name = tag_first_text(block, "name").unwrap_or("");
        let summary = tag_first_text(block, "summary").unwrap_or("");
        let keywords = collect_tag_texts(block, "keyword");
        let mut haystack = String::with_capacity(id.len() + name.len() + summary.len() + 64);
        haystack.push_str(id);
        haystack.push('\n');
        haystack.push_str(name.trim());
        haystack.push('\n');
        haystack.push_str(summary.trim());
        haystack.push('\n');
        haystack.push_str(&keywords.join("\n"));
        if !haystack.to_ascii_lowercase().contains(&needle) {
            continue;
        }
        let version = release_version(block).unwrap_or_else(|| "?".to_string());
        // `flatpak search --json` maps description = description.or(name);
        // appstream has no separate long description here, so summary is the
        // description with the display name as fallback (entity-unescaped).
        let description = if summary.trim().is_empty() {
            if name.trim().is_empty() {
                None
            } else {
                Some(unescape_xml(name.trim()))
            }
        } else {
            Some(unescape_xml(summary.trim()))
        };
        let mut pkg = Package::minimal(id.to_string(), version, "flatpak".to_string());
        pkg.installed = false;
        pkg.description = description;
        pkg.url = Some(format!("flatpak:{remote}"));
        out.push(pkg);
    }
    out
}

/// First `<tag ...>text</tag>` text in `block` (attribute-tolerant). Text
/// fragments that start with more markup (e.g. `<summary><em>...`) are
/// ignored — display-only degradation, never a search miss, since the
/// haystack keeps the plain-text fields (id/name + keywords still match).
fn tag_first_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open_at = block.find(&format!("<{tag}"))?;
    let gt_rel = block[open_at..].find('>')?;
    let rest = &block[open_at + gt_rel + 1..];
    let close_rel = rest.find(&format!("</{tag}>"))?;
    let text = &rest[..close_rel];
    if text.trim_start().starts_with('<') {
        return None;
    }
    Some(text)
}

/// All `<tag ...>text</tag>` texts in `block`, in order, trimmed.
fn collect_tag_texts(block: &str, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(open_rel) = block[cursor..].find(&format!("<{tag}")) {
        let content_at = cursor + open_rel;
        let Some(gt_rel) = block[content_at..].find('>') else {
            break;
        };
        let text_start = content_at + gt_rel + 1;
        if text_start > block.len() {
            break;
        }
        let rest = &block[text_start..];
        let Some(close_rel) = rest.find(&format!("</{tag}>")) else {
            break;
        };
        cursor = text_start + close_rel + tag.len() + 3;
        let text = rest[..close_rel].trim();
        if !text.is_empty() {
            out.push(unescape_xml(text));
        }
    }
    out
}

/// Version attribute of the first `<release version="...">` in `block`.
/// None when absent/empty (becomes `"?"` upstream, mirroring flatpak).
fn release_version(block: &str) -> Option<String> {
    let rel_at = block.find("<release")?;
    let after = &block[rel_at..];
    let v_at = after.find("version=")?;
    let post = &after[v_at + "version=".len()..];
    let quote_at = post.find(['"', '\''])?;
    let rest = &post[quote_at + 1..];
    let end_at = rest.find(['"', '\''])?;
    let v = rest[..end_at].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Minimal XML entity unescape for display text (the 5 predefined entities
/// plus the numeric `&#39;`). Unknown or numeric entities keep their bytes —
/// display-only, so partial decoding never corrupts search matching.
/// Always advances past at least one byte: an entity at end-of-string (`…&amp;`)
/// must terminate, not spin.
fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if let Some(r) = tail.strip_prefix("&lt;") {
            out.push('<');
            rest = r;
        } else if let Some(r) = tail.strip_prefix("&gt;") {
            out.push('>');
            rest = r;
        } else if let Some(r) = tail.strip_prefix("&amp;") {
            out.push('&');
            rest = r;
        } else if let Some(r) = tail.strip_prefix("&quot;") {
            out.push('"');
            rest = r;
        } else if let Some(r) = tail.strip_prefix("&apos;") {
            out.push('\'');
            rest = r;
        } else if let Some(r) = tail.strip_prefix("&#39;") {
            out.push('\'');
            rest = r;
        } else {
            // Unknown/numeric entity: keep the '&', consume exactly one byte.
            out.push('&');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Args for `flatpak update` (all refs).
pub fn update_all_command_args(noconfirm: bool) -> Vec<String> {
    let mut args = vec!["update".to_string()];
    if noconfirm {
        args.push("-y".to_string());
    }
    args
}

fn run_flatpak_inherit(args: &[String], what: &str) -> anyhow::Result<()> {
    let refs = arg_refs(args);
    let status = Command::new("flatpak")
        .args(&refs)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run {what}: {e}"))?;
    if !status.success() {
        anyhow::bail!("{what} failed with exit {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Update one installed app (`flatpak update <id>`). No sudo; readonly refused.
pub fn update_flatpak_package(id: &str, cfg: &crate::config::Config) -> anyhow::Result<()> {
    let id = validate_app_id(id)?;
    if cfg.is_readonly() {
        anyhow::bail!("refusing to update: readonly mode is enabled");
    }
    if !has_flatpak() {
        flatpak_missing_bail()?;
    }
    if !is_flatpak_installed(&id) {
        anyhow::bail!("flatpak app '{id}' is not installed");
    }
    run_flatpak_inherit(
        &update_command_args(&id, cfg.flatpak_noconfirm()),
        "flatpak update",
    )
}

/// Update all flatpak refs (`flatpak update`). Fail-open Ok when disabled or
/// binary missing so `--upgrade` degrades to pacman-only — callers needing
/// strictness check `has_flatpak()` first.
pub fn update_all_flatpak(cfg: &crate::config::Config) -> anyhow::Result<()> {
    if cfg.is_readonly() {
        anyhow::bail!("refusing to update: readonly mode is enabled");
    }
    if !cfg.flatpak_enabled() || !has_flatpak() {
        return Ok(());
    }
    run_flatpak_inherit(
        &update_all_command_args(cfg.flatpak_noconfirm()),
        "flatpak update",
    )
}

/// Parse the `Version:` line from `flatpak remote-info` text output.
/// None when absent/empty (fail-open: app skipped, never an error).
pub fn parse_remote_info_version(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("Version:") {
            let v = rest.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Live remote version for one installed app via
/// `flatpak remote-info --<scope> <origin> <id>`. None covers disabled
/// remotes, timeouts, missing refs — all fail-open skips.
fn remote_version(app_id: &str, origin: &str, scope: &str, timeout: Duration) -> Option<String> {
    let out = crate::process::output_bounded(
        "flatpak",
        &["remote-info", scope, origin, app_id],
        timeout,
    )?;
    if !out.status.success() {
        return None;
    }
    parse_remote_info_version(&String::from_utf8_lossy(&out.stdout))
}

/// Available flatpak updates: installed version vs remote `Version:`.
/// String inequality (flatpak versions are freeform, not alpm-comparable);
/// `?`/empty versions and failed lookups are skipped, never errors.
/// Both installations probed (`--system` then `--user`) since one id can live
/// in either.
pub fn check_flatpak_updates_with_config(
    cfg: &crate::config::Config,
) -> Vec<crate::updates::PkgUpdate> {
    if !cfg.flatpak_enabled() || !has_flatpak() {
        return vec![];
    }
    // Perf P4: cached inventory (60s) — one scan serves list + updates.
    let installed = list_installed_cached().unwrap_or_default();
    check_flatpak_updates_for(&installed, cfg)
}

/// Same check over a pre-fetched installed list (perf P3: `--stats` lists
/// once and reuses it here instead of spawning `flatpak list` twice).
/// Takes the list as granted; enablement/binary still gated.
pub fn check_flatpak_updates_for(
    installed: &[Package],
    cfg: &crate::config::Config,
) -> Vec<crate::updates::PkgUpdate> {
    if !cfg.flatpak_enabled() || !has_flatpak() || installed.is_empty() {
        return vec![];
    }
    let timeout = Duration::from_secs(cfg.search.timeout_secs.max(1));
    // Perf P3: fixed 4-way split over scoped threads (each thread blocks on
    // its own remote-info spawns; 6 apps ≈ 3s serial → <1.5s here).
    let per_chunk = installed.len().div_ceil(4).max(1);
    let out: Vec<crate::updates::PkgUpdate> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for chunk in installed.chunks(per_chunk) {
            handles.push(s.spawn(move || {
                let mut out = Vec::new();
                for pkg in chunk {
                    if let Some(u) = check_one_app(pkg, timeout) {
                        out.push(u);
                    }
                }
                out
            }));
        }
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut out = out;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// One app's update, if any. Pure-ish probe: None covers every skip/failure.
fn check_one_app(pkg: &Package, timeout: Duration) -> Option<crate::updates::PkgUpdate> {
    // Strip the non-stable branch suffix the list parser appends
    // ("1.0 (beta)") — remote-info versions carry no branch.
    let local = pkg.version.split(" (").next().unwrap_or("").trim();
    if local.is_empty() || local == "?" {
        return None;
    }
    let origin = pkg
        .url
        .as_deref()
        .and_then(|u| u.strip_prefix("flatpak:"))
        .unwrap_or("flathub");
    let remote = remote_version(&pkg.name, origin, "--system", timeout)
        .or_else(|| remote_version(&pkg.name, origin, "--user", timeout))?;
    let rv = remote.trim();
    if rv.is_empty() || rv == local {
        return None;
    }
    Some(crate::updates::PkgUpdate {
        name: pkg.name.clone(),
        old_version: local.to_string(),
        new_version: rv.to_string(),
        repo: "flatpak".to_string(),
        out_of_date: false,
    })
}

/// Memoized updates (`caching`: TTL + explicit invalidate). Remote-info is one
/// spawn per app, so Tab-switching within the TTL reuses the last check;
/// F5/upgrade paths call `invalidate_flatpak_memo()` for a fresh check.
pub fn check_flatpak_updates_cached(cfg: &crate::config::Config) -> Vec<crate::updates::PkgUpdate> {
    if let Some(hit) = memo_get() {
        return hit;
    }
    let fresh = check_flatpak_updates_with_config(cfg);
    memo_store(fresh.clone());
    fresh
}

/// Memoized check over a pre-fetched installed list (perf P3: `--stats`
/// lists once, then reuses the list here — one `flatpak list` total).
pub fn check_flatpak_updates_cached_for(
    installed: &[Package],
    cfg: &crate::config::Config,
) -> Vec<crate::updates::PkgUpdate> {
    if let Some(hit) = memo_get() {
        return hit;
    }
    let fresh = check_flatpak_updates_for(installed, cfg);
    memo_store(fresh.clone());
    fresh
}

fn memo_get() -> Option<Vec<crate::updates::PkgUpdate>> {
    const TTL_SECS: u64 = 300;
    let now = std::time::Instant::now();
    let memo = FLATPAK_UPDATES_MEMO.get_or_init(|| std::sync::Mutex::new((None, Vec::new())));
    if let Ok(guard) = memo.lock()
        && let (Some(at), ref cached) = *guard
        && now.duration_since(at).as_secs() < TTL_SECS
    {
        return Some(cached.clone());
    }
    None
}

fn memo_store(fresh: Vec<crate::updates::PkgUpdate>) {
    let memo = FLATPAK_UPDATES_MEMO.get_or_init(|| std::sync::Mutex::new((None, Vec::new())));
    if let Ok(mut guard) = memo.lock() {
        *guard = (Some(std::time::Instant::now()), fresh);
    }
}

/// Forget the memoized flatpak update list (refresh/upgrade/clean paths).
pub fn invalidate_flatpak_memo() {
    let memo = FLATPAK_UPDATES_MEMO.get_or_init(|| std::sync::Mutex::new((None, Vec::new())));
    if let Ok(mut guard) = memo.lock() {
        *guard = (None, Vec::new());
    }
}

static FLATPAK_UPDATES_MEMO: std::sync::OnceLock<
    std::sync::Mutex<(Option<std::time::Instant>, Vec<crate::updates::PkgUpdate>)>,
> = std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    const LIST_JSON: &str = r#"[
      {"name":"Firefox","application_id":"org.mozilla.firefox","version":"158.0","branch":"stable","arch":"x86_64","origin":"flathub","installation":"system"},
      {"name":"Nvidia VAAPI driver","application_id":"org.freedesktop.Platform.VAAPI.nvidia","version":"","branch":"26.08","arch":"x86_64","origin":"flathub","installation":"system"},
      {"name":"","version":"1.0"}
    ]"#;

    #[test]
    fn parse_list_json_counts_and_fields() {
        let pkgs = parse_flatpak_list_json(LIST_JSON);
        // Third row has no application_id -> skipped.
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].repo, "flatpak");
        assert!(pkgs.iter().all(|p| p.installed));
        let ff = pkgs
            .iter()
            .find(|p| p.name == "org.mozilla.firefox")
            .unwrap();
        assert_eq!(ff.version, "158.0");
        assert_eq!(ff.arch.as_deref(), Some("x86_64"));
    }

    #[test]
    fn parse_list_json_empty_version_becomes_qmark_with_branch() {
        let pkgs = parse_flatpak_list_json(LIST_JSON);
        let vaapi = pkgs
            .iter()
            .find(|p| p.name == "org.freedesktop.Platform.VAAPI.nvidia")
            .unwrap();
        assert_eq!(vaapi.version, "? (26.08)");
    }

    #[test]
    fn parse_list_json_garbage_is_empty_never_panics() {
        assert!(parse_flatpak_list_json("").is_empty());
        assert!(parse_flatpak_list_json("not json").is_empty());
        assert!(parse_flatpak_list_json("{\"a\":1}").is_empty());
        assert!(parse_flatpak_list_json("null").is_empty());
    }

    #[test]
    fn parse_columns_preserves_empty_version() {
        let out = "org.mozilla.firefox\t158.0\tstable\tflathub\norg.freedesktop.Platform.VAAPI.nvidia\t\t26.08\tflathub\n";
        let pkgs = parse_flatpak_list_columns(out);
        assert_eq!(pkgs.len(), 2);
        let vaapi = pkgs.iter().find(|p| p.name.contains("VAAPI")).unwrap();
        assert_eq!(vaapi.version, "? (26.08)");
    }

    #[test]
    fn parse_remote_list_names() {
        let out = "Name\nflathub\nfilmcraft-origin\n";
        assert_eq!(parse_remote_list(out), vec!["flathub", "filmcraft-origin"]);
        assert!(parse_remote_list("").is_empty());
    }

    #[test]
    fn disabled_config_never_searches() {
        // v0.9.4: the trait wrapper is gone — the free functions ARE the
        // interface, and the enabled gate fronts them. Probes stay live
        // (boolean only, hermetic on both outcomes), and a disabled config
        // returns empty without touching anything.
        let _ = has_flatpak();
        let mut cfg = crate::config::Config::default();
        cfg.behavior.flatpak_enabled = false;
        assert!(
            search_flatpak_with_config("", 10, false, &cfg)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn live_list_smoke() {
        if !has_flatpak() {
            return;
        }
        let pkgs = list_installed().unwrap_or_default();
        // Machine has flatpaks; off-Arch CI skips via early return above.
        assert!(pkgs.iter().all(|p| p.repo == "flatpak" && p.installed));
    }

    #[test]
    fn list_cache_matches_live_and_invalidates() {
        invalidate_flatpak_list_memo();
        if !has_flatpak() {
            // Absent binary: both error with the install hint, no spawn.
            assert!(list_installed().is_err());
            assert!(list_installed_cached().is_err());
            return;
        }
        let live = list_installed().unwrap_or_default();
        let cached = list_installed_cached().unwrap_or_default();
        assert_eq!(live, cached);
        // Second call serves the memo (same content, no spawn needed).
        let cached2 = list_installed_cached().unwrap_or_default();
        assert_eq!(live, cached2);
        invalidate_flatpak_list_memo();
    }

    const SEARCH_JSON: &str = r#"[
      {"name":"Firefox","description":"Fast, Private & Safe Web Browser","application_id":"org.mozilla.firefox","version":"158.0","branch":"stable","remotes":"flathub"},
      {"name":"Nvidia VAAPI driver","description":"VA-API","application_id":"org.freedesktop.Platform.VAAPI.nvidia","version":"","branch":"26.08","remotes":"flathub"},
      {"name":"NoId","version":"1.0"}
    ]"#;

    #[test]
    fn parse_search_json_fields_limit_order() {
        let pkgs = parse_flatpak_search_json(SEARCH_JSON, 0);
        assert_eq!(pkgs.len(), 2);
        // Remote relevance order preserved (firefox first).
        assert_eq!(pkgs[0].name, "org.mozilla.firefox");
        assert_eq!(pkgs[0].version, "158.0");
        assert!(!pkgs[0].installed);
        let vaapi = &pkgs[1];
        assert_eq!(vaapi.version, "? (26.08)");
        let one = parse_flatpak_search_json(SEARCH_JSON, 1);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn parse_search_json_garbage_is_empty() {
        assert!(parse_flatpak_search_json("", 0).is_empty());
        assert!(parse_flatpak_search_json("nope", 0).is_empty());
        assert!(parse_flatpak_search_json("null", 0).is_empty());
    }

    #[test]
    fn search_empty_query_is_empty_without_spawning() {
        let cfg = crate::config::Config::default();
        let pkgs = search_flatpak_with_config("   ", 50, false, &cfg).unwrap();
        assert!(pkgs.is_empty());
    }

    #[test]
    fn search_disabled_is_empty() {
        let mut cfg = crate::config::Config::default();
        cfg.behavior.flatpak_enabled = false;
        let pkgs = search_flatpak_with_config("firefox", 50, false, &cfg).unwrap();
        assert!(pkgs.is_empty());
    }

    #[test]
    fn search_invalid_regex_is_err() {
        if !has_flatpak() {
            return;
        }
        let cfg = crate::config::Config::default();
        assert!(search_flatpak_with_config("([", 50, true, &cfg).is_err());
    }

    #[test]
    fn validate_app_id_shapes() {
        assert!(validate_app_id("org.mozilla.firefox").is_ok());
        assert!(validate_app_id("  org.cutwire.Drift  ").is_ok());
        assert_eq!(
            validate_app_id("  org.cutwire.Drift  ").unwrap(),
            "org.cutwire.Drift"
        );
        for bad in [
            "",
            "   ",
            "firefox",
            "../evil",
            "a/b",
            "a\\b",
            ".leading",
            "trailing.",
            "has space",
            "semi;colon",
            "foo\0bar",
            "a..b",
        ] {
            assert!(validate_app_id(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn install_args_no_sudo_confirm_gated() {
        let plain = install_command_args("flathub", "org.mozilla.firefox", false);
        assert_eq!(plain, vec!["install", "flathub", "org.mozilla.firefox"]);
        assert!(!plain.iter().any(|a| a == "sudo"));
        let nc = install_command_args("flathub", "org.mozilla.firefox", true);
        assert_eq!(nc.last().map(|s| s.as_str()), Some("-y"));
    }

    #[test]
    fn remove_args_no_sudo_confirm_gated() {
        let plain = remove_command_args("org.mozilla.firefox", false);
        assert_eq!(plain, vec!["uninstall", "org.mozilla.firefox"]);
        assert!(!plain.iter().any(|a| a == "sudo"));
        let nc = remove_command_args("org.mozilla.firefox", true);
        assert!(nc.contains(&"-y".to_string()));
        let unused = unused_command_args(true);
        assert_eq!(unused, vec!["uninstall", "--unused", "-y"]);
        assert!(!unused_command_args(false).contains(&"-y".to_string()));
    }

    #[test]
    fn readonly_refuses_mutation_without_spawning() {
        let mut cfg = crate::config::Config::default();
        cfg.behavior.readonly = true;
        assert!(install_flatpak_package("org.mozilla.firefox", &cfg).is_err());
        assert!(remove_flatpak_package("org.mozilla.firefox", &cfg).is_err());
        assert!(clean_unused_flatpak(&cfg).is_err());
    }

    #[test]
    fn invalid_id_never_spawns() {
        let cfg = crate::config::Config::default();
        assert!(install_flatpak_package("not-an-app-id", &cfg).is_err());
        assert!(remove_flatpak_package("", &cfg).is_err());
        assert!(!is_flatpak_installed("not-an-app-id"));
        assert!(!is_flatpak_installed("definitely.not.installed.xyz"));
    }

    #[test]
    fn invalid_app_id_errors_through_free_runners() {
        // Invalid id errors surface through the free functions (no silent success).
        let cfg = crate::config::Config::default();
        assert!(install_flatpak_package("bad", &cfg).is_err());
        assert!(remove_flatpak_package("bad", &cfg).is_err());
        // Disabled backend short-circuits to empty without spawning (the
        // updates helper degrades to a plain Vec — no Result at this seam).
        let mut off_cfg = crate::config::Config::default();
        off_cfg.behavior.flatpak_enabled = false;
        assert!(check_flatpak_updates_with_config(&off_cfg).is_empty());
        // Empty query short-circuits (enabled config, no spawn either way).
        assert!(
            search_flatpak_with_config("", 10, false, &cfg)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn parse_remote_info_version_vectors() {
        let out = "Drift - Edit videos\n\n            ID: org.cutwire.Drift\n       Version: 0.8.0\n       License: GPL\n";
        assert_eq!(parse_remote_info_version(out), Some("0.8.0".to_string()));
        assert_eq!(parse_remote_info_version("no version here\n"), None);
        assert_eq!(parse_remote_info_version("Version:   \n"), None);
        assert_eq!(parse_remote_info_version(""), None);
    }

    #[test]
    fn update_args_no_sudo_confirm_gated() {
        assert_eq!(
            update_command_args("org.x.App", false),
            vec!["update", "org.x.App"]
        );
        assert_eq!(
            update_command_args("org.x.App", true),
            vec!["update", "org.x.App", "-y"]
        );
        assert_eq!(update_all_command_args(false), vec!["update"]);
        assert_eq!(update_all_command_args(true), vec!["update", "-y"]);
    }

    #[test]
    fn update_readonly_and_invalid_refuse_without_spawning() {
        let mut cfg = crate::config::Config::default();
        cfg.behavior.readonly = true;
        assert!(update_flatpak_package("org.mozilla.firefox", &cfg).is_err());
        assert!(update_all_flatpak(&cfg).is_err());
        let cfg = crate::config::Config::default();
        assert!(update_flatpak_package("bad", &cfg).is_err());
    }

    #[test]
    fn flatpak_memo_invalidate_is_safe() {
        invalidate_flatpak_memo();
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let cfg = crate::config::Config::default();
        let a = check_flatpak_updates_cached(&cfg);
        let b = check_flatpak_updates_cached(&cfg);
        assert_eq!(a, b);
        // Same memo serves the pre-fetched-list variant (P3 single-scan):
        // recompute from an explicit list must match the internal-list result.
        let listed = list_installed().unwrap_or_default();
        invalidate_flatpak_memo();
        let c = check_flatpak_updates_cached_for(&listed, &cfg);
        assert_eq!(a, c);
        invalidate_flatpak_memo();
    }

    #[test]
    fn for_list_variants_short_circuit_without_spawning() {
        // Disabled or empty list returns empty with no subprocess.
        // (Uncached `for` only — the cached variant shares the memo with the
        // test above, so its lifecycle is asserted there.)
        let mut cfg = crate::config::Config::default();
        cfg.behavior.flatpak_enabled = false;
        let pkgs = vec![Package::minimal(
            "org.x.App".into(),
            "1.0".into(),
            "flatpak".into(),
        )];
        assert!(check_flatpak_updates_for(&pkgs, &cfg).is_empty());
        let cfg = crate::config::Config::default();
        assert!(check_flatpak_updates_for(&[], &cfg).is_empty());
    }

    // -------------------------------------------------------------------
    // Appstream direct search (perf W) — pure parser + path plumbing.
    // -------------------------------------------------------------------

    const APPSTREAM_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<components version="0.15" origin="flathub">
  <component type="desktop">
    <id>org.mozilla.firefox.desktop</id>
    <name>Firefox</name>
    <summary>Fast, Private &amp; Safe Web Browser</summary>
    <keywords><keyword>browser</keyword><keyword>internet</keyword></keywords>
    <releases>
      <release version="158.0" date="2026-01-01"/>
      <release version="157.0" date="2025-12-01"/>
    </releases>
  </component>
  <component type="runtime">
    <id>org.freedesktop.Platform</id>
    <name>Freedesktop Platform</name>
    <summary>Shared runtime</summary>
    <releases><release version="26.08" date="2026-02-01"/></releases>
  </component>
  <component type="desktop">
    <id>org.x.Termui</id>
    <name>TermUI</name>
    <summary>A terminal ui demo</summary>
  </component>
</components>"#;

    #[test]
    fn appstream_fields_extracted_like_flatpak_search_json() {
        let pkgs = parse_appstream_search(APPSTREAM_XML, "firefox", 0, "flathub");
        assert_eq!(pkgs.len(), 1);
        let ff = &pkgs[0];
        // `.desktop` suffix stripped, version from first <release>, entities decode.
        assert_eq!(ff.name, "org.mozilla.firefox");
        assert_eq!(ff.version, "158.0");
        assert_eq!(
            ff.description.as_deref(),
            Some("Fast, Private & Safe Web Browser")
        );
        assert_eq!(ff.url.as_deref(), Some("flatpak:flathub"));
        assert_eq!(ff.repo, "flatpak");
        assert!(!ff.installed);
    }

    #[test]
    fn appstream_matches_keywords_and_folds_case() {
        // Keyword hit ("browser") and an uppercase query both match.
        assert_eq!(
            parse_appstream_search(APPSTREAM_XML, "browser", 0, "r").len(),
            1
        );
        assert_eq!(
            parse_appstream_search(APPSTREAM_XML, "FIREFOX", 0, "r").len(),
            1
        );
        // Runtime components are searchable, search results are apps or runtimes.
        let rt = parse_appstream_search(APPSTREAM_XML, "platform", 0, "r");
        assert!(rt.iter().all(|p| p.name == "org.freedesktop.Platform"));
        assert_eq!(rt.len(), 1);
        // "ui" only hits the TermUI summary.
        assert_eq!(parse_appstream_search(APPSTREAM_XML, "ui", 0, "r").len(), 1);
    }

    #[test]
    fn appstream_limit_truncates_and_zero_is_unbounded() {
        assert_eq!(parse_appstream_search(APPSTREAM_XML, "a", 1, "r").len(), 1);
        assert_eq!(parse_appstream_search(APPSTREAM_XML, "e", 2, "r").len(), 2);
        // "e" matches all three summaries/ids.
        assert_eq!(parse_appstream_search(APPSTREAM_XML, "e", 0, "r").len(), 3);
    }

    #[test]
    fn appstream_garbage_and_empty_query_are_empty() {
        assert!(parse_appstream_search("", "firefox", 0, "r").is_empty());
        assert!(parse_appstream_search("plain text, not xml", "firefox", 0, "r").is_empty());
        assert!(parse_appstream_search(APPSTREAM_XML, "", 0, "r").is_empty());
        assert!(parse_appstream_search(APPSTREAM_XML, "   ", 0, "r").is_empty());
        assert!(parse_appstream_search(APPSTREAM_XML, "not-in-here", 0, "r").is_empty());
    }

    #[test]
    fn appstream_missing_releases_becomes_qmark() {
        // Third fixture component has no <releases> — version falls to "?".
        let pkgs = parse_appstream_search(APPSTREAM_XML, "termui", 0, "r");
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].version, "?");
    }

    #[test]
    fn appstream_trailing_entity_terminates_and_unescapes() {
        // Regression: an entity at end-of-text used to spin `unescape_xml`
        // forever (cursor never advanced past it).
        assert_eq!(unescape_xml("&amp;"), "&");
        assert_eq!(unescape_xml("A&amp;"), "A&");
        assert_eq!(unescape_xml("&amp;&amp;"), "&&");
        assert_eq!(unescape_xml("B&lt;"), "B<");
        assert_eq!(unescape_xml("&x;"), "&x;");
        assert_eq!(unescape_xml("&"), "&");
        let xml = r#"<components><component type="desktop"><id>org.q.Q</id><name>Q</name><summary>Salt &amp;</summary></component></components>"#;
        let pkgs = parse_appstream_search(xml, "q", 0, "r");
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].description.as_deref(), Some("Salt &"));
    }

    #[test]
    fn appstream_unclosed_tail_keeps_collected_results() {
        let truncated =
            format!("{APPSTREAM_XML}\n  <component type=\"desktop\">\n    <id>org.b.Clip</id>");
        let pkgs = parse_appstream_search(&truncated, "firefox", 0, "r");
        assert_eq!(pkgs.len(), 1);
    }

    #[test]
    fn appstream_search_roots_merge_dedupe_and_detect_cache() {
        let root_a = tempfile::tempdir().unwrap();
        let root_b = tempfile::tempdir().unwrap();
        // Same app-id in both roots: first root wins, second is deduped.
        let dir = |root: &tempfile::TempDir| {
            let p = root
                .path()
                .join("appstream")
                .join("flathub")
                .join("x86_64")
                .join("active");
            std::fs::create_dir_all(&p).unwrap();
            p
        };
        std::fs::write(
            dir(&root_a).join("appstream.xml"),
            r#"<components><component type="desktop"><id>org.d.App</id><name>App</name><summary>one</summary><releases><release version="1.0"/></releases></component></components>"#,
        )
        .unwrap();
        std::fs::write(
            dir(&root_b).join("appstream.xml"),
            r#"<components><component type="desktop"><id>org.d.App</id><name>App</name><summary>one</summary><releases><release version="2.0"/></releases></component><component type="desktop"><id>org.b.Other</id><name>Other</name><summary>an app too</summary></component></components>"#,
        )
        .unwrap();
        let merged = appstream_search_roots(
            "app",
            0,
            "flathub",
            &[root_a.path().to_path_buf(), root_b.path().to_path_buf()],
        );
        let pkgs = merged.unwrap_or_default();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "org.d.App");
        assert_eq!(pkgs[0].version, "1.0"); // first root wins the dedupe
        assert_eq!(pkgs[1].name, "org.b.Other");
        assert_eq!(pkgs[1].version, "?"); // no <releases> block
        // No cache anywhere -> None (caller falls back to the spawn path).
        let empty_root = tempfile::tempdir().unwrap();
        assert!(appstream_search_roots("app", 0, "flathub", &[]).is_none());
        assert!(
            appstream_search_roots("app", 0, "flathub", &[empty_root.path().to_path_buf()])
                .is_none()
        );
    }

    #[test]
    fn appstream_cap_boundary_and_empty_remote() {
        assert!(within_appstream_cap(0));
        assert!(within_appstream_cap(256 * 1024 * 1024));
        assert!(!within_appstream_cap(256 * 1024 * 1024 + 1));
        assert!(!within_appstream_cap(u64::MAX));
        // Empty remote name never consults files.
        assert!(appstream_search_roots("a", 0, "  ", &[]).is_none());
    }

    // v0.10.2 phase 2 (E1): the missing-flatpak hint was cloned at five
    // bail sites; the single helper is the one owner of the string and a
    // drift pin (the independent truth = v0.10.2 plan §E1's literal, which
    // itself recorded today's shipped behavior).
    #[test]
    fn flatpak_missing_hint_is_byte_pinned_to_the_single_owner() {
        let err = flatpak_missing_bail().unwrap_err();
        assert_eq!(
            err.to_string(),
            "flatpak not found — install flatpak to manage flatpak apps"
        );
    }
}
