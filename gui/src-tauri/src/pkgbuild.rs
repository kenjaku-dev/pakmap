//! v0.9 PKGBUILD review surface (closes `docs/gui-architecture.md` divergence
//! "live PKGBUILD review stays a TUI surface"): an in-GUI, read-only PKGBUILD
//! viewer backed by the same cache dir the TUI/install flow uses — patching
//! the AUR trust gap where the review happened out-of-band with the build.
//!
//! Contract (see the architecture doc §pkgbuild):
//! - **cache-first**: a normal view reads the existing clone only, no network;
//!   an uncached package reports `source:"none"` so the frontend can offer an
//!   explicit *"fetch" action (never auto-fetch on view).
//! - **fetch-on-demand** (`fetch:true`) refreshes the clone inside a bounded
//!   60s window (depth-1 clone or ff-only pull), reusing `clone_dir_for`
//!   (traversal-guarded, same dir as installs).
//! - **bounded everything**: 256KB PKGBUILD read cap (`truncated` flag), an
//!   8000-char diff cap matching `diff::git_diff_cached`, and timeouts everywhere
//!   through the shared bounded runner.
//! - **display-only trust boundary**: no makepkg/pacman here, no build button
//!   in the review view (build stays in OpActions with the risk copy), content
//!   is never logged and never leaves the process except as the IPC payload
//!
use serde::Serialize;

/// Safety cap: PKGBUILDs are <50KB in practice; 256KB keeps a pathological
/// clone from ballooning the webview, and anything past the cap sets
/// `truncated` so the UI points the user to aur.archlinux.org.
const READ_CAP_BYTES: usize = 256 * 1024;

/// Bounded review-fetch. A fresh depth-1 clone of an AUR repo is tiny, but the
/// review surface must never hang on a slow network — the TUI's install flow
/// has its own (larger) budget; this one stays light.
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Same diff cap as `diff::git_diff_cached` — the wire shape stays comparable.
const DIFF_CAP_CHARS: usize = 8000;

/// Wire shape of `get_pkgbuild` (docs/gui-architecture.md §pkgbuild).
/// `source: "cache" | "fetched" | "none"` drives the frontend states:
/// cached content / freshly fetched (green freshness dot) / needs a fetch.
#[derive(Serialize, Clone)]
pub struct PkgbuildResult {
    pub name: String,
    pub source: &'static str,
    pub content: Option<String>,
    pub truncated: bool,
    pub diff: Option<String>,
    pub fetch_ok: bool,
}

/// Read the clone's PKGBUILD under `cap` bytes via the shared bounded reader:
/// cap enforced DURING the read (`take`), symlink + non-regular rejection
/// (the PKGBUILD→/dev/zero clone attack), and dev/ino swap detection. The
/// v0.9.3 replacement for read-whole-then-cut (CWE-400; 300KB files no longer
/// allocate past the cap). `truncated` stays a char-boundary cut.
fn read_pkgbuild_capped(dir: &std::path::Path, cap: usize) -> Option<(String, bool)> {
    match pakmap::safety::fs::read_text_bounded(&dir.join("PKGBUILD"), cap) {
        Ok(b) => Some((b.text, b.truncated)),
        Err(_) => None,
    }
}

/// Bounded, network-free diff of the cached clone's working tree vs its HEAD
/// (like `diff::git_diff_cached` minus the 15s upstream fetch — the GUI's
/// cache path stays strictly offline; freshness is an explicit fetch=true).
fn local_git_diff(dir: &std::path::Path) -> Option<String> {
    if !dir.join(".git").exists() {
        return None;
    }
    let output = pakmap::process::output_bounded(
        "git",
        &[
            "-C",
            &dir.to_string_lossy(),
            "diff",
            "--",
            "PKGBUILD",
            ".SRCINFO",
        ],
        pakmap::process::QUICK_TIMEOUT,
    )?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s.chars().take(DIFF_CAP_CHARS).collect())
    }
}

/// Bounded on-demand refresh of the AUR clone: existing clone → ff-only pull,
/// missing → depth-1 clone from the official AUR git (same URL/dir policy as
/// `ops::install_aur_flow`). Never escalates, never builds.
fn fetch_latest(dir: &std::path::Path, name: &str) -> bool {
    if dir.join(".git").exists() {
        pakmap::process::status_null_bounded(
            "git",
            &["-C", &dir.to_string_lossy(), "pull", "--ff-only"],
            FETCH_TIMEOUT,
        )
        .map(|s| s.success())
        .unwrap_or(false)
    } else {
        let url = format!("https://aur.archlinux.org/{name}.git");
        // `--` keeps the URL/dst from being parsed as options (parity with
        // the install flow's clone guard).
        pakmap::process::status_null_bounded(
            "git",
            &[
                "clone",
                "--depth",
                "1",
                "--",
                url.as_str(),
                &dir.to_string_lossy(),
            ],
            FETCH_TIMEOUT,
        )
        .map(|s| s.success())
        .unwrap_or(false)
    }
}

/// Core of the `get_pkgbuild` command, `cfg`-seamed for unit tests. Sync
/// function doing I/O and possibly one bounded git subprocess — the command
/// drives it through `spawn_blocking` so a runtime worker never sits on
/// git/network waits.
pub fn build_pkgbuild_result(
    cfg: &pakmap::config::Config,
    raw_name: &str,
    want_fetch: bool,
) -> Result<PkgbuildResult, crate::GuiError> {
    crate::ops::validate_name(raw_name)?;
    let name = raw_name.trim().to_string();
    let Some(dir) = pakmap::diff::clone_dir_for(cfg, &name) else {
        return Err(crate::gui_error(
            "invalid_name",
            format!("could not resolve the cache dir for {name:?}"),
            Some("set behavior.cache_dir in ~/.config/pakmap/config.toml".to_string()),
        ));
    };

    let mut out = PkgbuildResult {
        name: name.clone(),
        source: "none",
        content: None,
        truncated: false,
        diff: None,
        fetch_ok: false,
    };

    let cached = read_pkgbuild_capped(&dir, READ_CAP_BYTES);
    if cached.is_none() && !want_fetch {
        // Uncached and no fetch requested: the frontend shows "not cached"
        // with an explicit fetch action. No network happened.
        return Ok(out);
    }
    if want_fetch && fetch_latest(&dir, &name) {
        // Fresh content stands alone: diffing it against the clone it came
        // from would always be empty, and the freshness label is the signal.
        if let Some((content, truncated)) = read_pkgbuild_capped(&dir, READ_CAP_BYTES) {
            tracing::info!(
                target: "pakmap_gui::pkgbuild",
                name = %name,
                bytes = content.len(),
                "pkgbuild refreshed from the AUR for review"
            );
            return Ok(PkgbuildResult {
                source: "fetched",
                fetch_ok: true,
                content: Some(content),
                truncated,
                ..out
            });
        }
    }
    // Fail-open: a failed/absent fetch still serves the cached copy.
    if let Some((content, truncated)) = cached {
        out.source = "cache";
        out.content = Some(content);
        out.truncated = truncated;
        out.diff = local_git_diff(&dir);
    }
    Ok(out)
}

/// Cache-first PKGBUILD review. `fetch: Some(true)` performs the bounded
/// refresh; the default is offline-only. Heavy work runs on a blocking worker
/// so the async command never pins a runtime thread.
///
/// Returns a `Result` only for policy-level failures (invalid/traversal names,
/// unresolvable cache dir); a failed network fetch degrades to `fetch_ok:false`
/// with whatever cached content exists — never a fatal error.
#[tauri::command]
pub(crate) async fn get_pkgbuild(
    name: String,
    fetch: Option<bool>,
) -> Result<PkgbuildResult, crate::GuiError> {
    let want_fetch = fetch.unwrap_or(false);
    let handle = tauri::async_runtime::spawn_blocking(move || {
        build_pkgbuild_result(&pakmap::config::Config::load(), &name, want_fetch)
    });
    match handle.await {
        Ok(result) => result,
        Err(e) => Err(crate::gui_error(
            "pkgbuild_worker_failed",
            format!("pkgbuild worker failed: {e}"),
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_cache(cache_dir: &std::path::Path) -> pakmap::config::Config {
        pakmap::config::Config {
            behavior: pakmap::config::BehaviorConfig {
                cache_dir: Some(cache_dir.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn invalid_names_are_rejected() {
        let cfg = pakmap::config::Config::default();
        for bad in [
            "",
            "  ",
            "../evil",
            "a/b",
            "back\\slash",
            "two..dots",
            "nul\0name",
        ] {
            let res = build_pkgbuild_result(&cfg, bad, false);
            assert!(res.is_err(), "expected reject for {bad:?}");
            assert_eq!(res.err().map(|e| e.code), Some("invalid_name".to_string()));
        }
    }

    #[test]
    fn uncached_without_fetch_reports_none() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_with_cache(tmp.path());
        let res = build_pkgbuild_result(&cfg, "tst-uncached", false).unwrap();
        assert_eq!(res.source, "none");
        assert!(res.content.is_none());
        assert!(!res.fetch_ok);
        assert!(!res.truncated);
        assert!(res.diff.is_none());
    }

    #[test]
    fn cached_pkgbuild_reads_from_clone_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tst-cached");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("PKGBUILD"), "pkgver=1\npkgrel=1").unwrap();
        let cfg = cfg_with_cache(tmp.path());

        let res = build_pkgbuild_result(&cfg, "tst-cached", false).unwrap();
        assert_eq!(res.source, "cache");
        assert_eq!(res.content.as_deref(), Some("pkgver=1\npkgrel=1"));
        assert!(!res.truncated);
        assert!(!res.fetch_ok);
        // No .git dir → no diff, offline read stays silent.
        assert!(res.diff.is_none());
    }

    #[test]
    fn oversized_pkgbuild_is_truncated_on_char_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tst-trunc");
        std::fs::create_dir_all(&dir).unwrap();
        // ASCII bulk + a multibyte tail straddling the cap.
        let big = format!("{}é", "x".repeat(READ_CAP_BYTES - 1));
        std::fs::write(dir.join("PKGBUILD"), &big).unwrap();
        let cfg = cfg_with_cache(tmp.path());

        let res = build_pkgbuild_result(&cfg, "tst-trunc", false).unwrap();
        assert_eq!(res.source, "cache");
        assert!(res.truncated);
        let content = res.content.unwrap();
        assert!(content.len() <= READ_CAP_BYTES);
        assert!(content.is_char_boundary(content.len()));
    }

    #[test]
    fn oversized_kernel_style_pkgbuild_ascii_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tst-ascii-trunc");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("PKGBUILD"), "x".repeat(READ_CAP_BYTES + 1024)).unwrap();
        let cfg = cfg_with_cache(tmp.path());

        let res = build_pkgbuild_result(&cfg, "tst-ascii-trunc", false).unwrap();
        assert!(res.truncated);
        assert_eq!(res.content.unwrap().len(), READ_CAP_BYTES);
    }

    #[test]
    fn fetch_fail_open_keeps_cached_content() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tst-failopen");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("PKGBUILD"), "pkgver=1").unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .current_dir(&dir)
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        assert!(git(&["add", "-A"]).status.success());
        assert!(git(&["commit", "-qm", "init"]).status.success());
        // No remote configured → the bounded `git pull` fails fast and offline.
        let cfg = cfg_with_cache(tmp.path());
        let res = build_pkgbuild_result(&cfg, "tst-failopen", true).unwrap();
        assert_eq!(res.source, "cache");
        assert!(
            !res.fetch_ok,
            "pull with no remote must report fetch_ok=false"
        );
        assert_eq!(res.content.as_deref(), Some("pkgver=1"));
    }

    #[test]
    fn local_git_diff_covers_worktree_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tst-diff");
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .current_dir(&dir)
                .output()
                .unwrap()
        };
        std::fs::write(dir.join("PKGBUILD"), "pkgver=1\n").unwrap();
        assert!(git(&["init", "-q"]).status.success());
        assert!(git(&["add", "-A"]).status.success());
        assert!(git(&["commit", "-qm", "init"]).status.success());
        assert!(local_git_diff(&dir).is_none(), "clean tree → no diff");
        std::fs::write(dir.join("PKGBUILD"), "pkgver=2\n").unwrap();
        let d = local_git_diff(&dir).unwrap();
        assert!(d.contains("pkgver=2"));
        assert!(d.chars().count() <= DIFF_CAP_CHARS);
    }

    #[test]
    fn local_git_diff_absent_without_repo() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
        std::fs::write(tmp.path().join("pkg").join("PKGBUILD"), "pkgver=1").unwrap();
        assert!(local_git_diff(&tmp.path().join("pkg")).is_none());
    }
}
