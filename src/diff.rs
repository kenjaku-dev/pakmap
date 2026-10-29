use std::path::PathBuf;

/// Where AUR git clones live (shared with install/aur.rs cache policy).
/// The HOME/XDG-less fallback is a per-UID private dir under $TMPDIR —
/// v0.9.3 replaces the old shared `/tmp` default (CWE-377).
pub fn cache_base(cfg: &crate::config::Config) -> PathBuf {
    if let Some(p) = cfg.cache_dir_path() {
        return p;
    }
    dirs::cache_dir()
        .map(|p| p.join("pakmap"))
        .unwrap_or_else(crate::safety::fs::per_user_cache_fallback)
}

pub fn clone_dir_for(cfg: &crate::config::Config, name: &str) -> Option<PathBuf> {
    let safe: PathBuf = PathBuf::from(name.trim());
    let file = safe.file_name()?.to_str()?;
    if file != name.trim() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(cache_base(cfg).join(file))
}

/// Read the cached PKGBUILD for a previous install/clone, if any.
/// Bounded via the shared reader (v0.9.3): byte cap during the read, symlink
/// and non-regular rejection — the 8KB preview never depends on a full read.
pub fn cached_pkgbuild(cfg: &crate::config::Config, name: &str) -> Option<String> {
    let dir = clone_dir_for(cfg, name)?;
    crate::safety::fs::read_text_bounded(
        &dir.join("PKGBUILD"),
        crate::safety::fs::PKGBUILD_READ_CAP,
    )
    .ok()
    .map(|b| b.text)
}

/// Pure line diff for PKGBUILD preview: `-old` / `+new` / ` unchanged`.
/// No external `diff` dependency; deterministic and unit-tested.
/// `ui-design`: minimal signal — capped to `max_lines` with trailer count.
pub fn render_diff(old: &str, new: &str, max_lines: usize) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // LCS DP on lines; PKGBUILDs are small (<500 lines) so O(n*m) is fine.
    let n = a.len().min(600);
    let m = b.len().min(600);
    let a = &a[..n];
    let b = &b[..m];
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out: Vec<String> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            if out.len() < max_lines {
                out.push(format!("  {}", a[i]));
            }
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(format!("- {}", a[i]));
            i += 1;
        } else {
            out.push(format!("+ {}", b[j]));
            j += 1;
        }
        if out.len() >= max_lines {
            break;
        }
    }
    while i < n && out.len() < max_lines {
        out.push(format!("- {}", a[i]));
        i += 1;
    }
    while j < m && out.len() < max_lines {
        out.push(format!("+ {}", b[j]));
        j += 1;
    }
    let total_changes = out
        .iter()
        .filter(|l| l.starts_with('-') || l.starts_with('+'))
        .count();
    if out.len() >= max_lines {
        out.push(format!("… truncated (showing {max_lines} lines)"));
    }
    if total_changes == 0 {
        return "No PKGBUILD changes.".to_string();
    }
    out.join("\n")
}

/// Try `git diff` inside an existing clone.
/// Returns None when no clone/diff (first install) so callers show full PKGBUILD.
pub fn git_diff_cached(name: &str, cfg: &crate::config::Config) -> Option<String> {
    let dir = clone_dir_for(cfg, name)?;
    if !dir.join(".git").exists() {
        return None;
    }
    // 5.1: fetch upstream first so the diff reflects pulled-but-unbuilt
    // changes, not just local edits. Best-effort with a timeout: offline or
    // slow networks fall back to the cached diff (fail-open).
    fetch_quiet(&dir);
    // Bounded (perf P2): local diff, fast — timeout is pure hang insurance.
    let output = crate::process::output_bounded(
        "git",
        &[
            "-C",
            &dir.to_string_lossy(),
            "diff",
            "--",
            "PKGBUILD",
            ".SRCINFO",
        ],
        crate::process::QUICK_TIMEOUT,
    )?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s.chars().take(8000).collect())
    }
}

/// `git fetch --quiet origin` bounded by `timeout` (default 15s).
/// Returns true on success; false covers offline/slow/missing-remote —
/// callers must treat false as "use cached state", never as fatal.
/// Perf P2: runs through the shared bounded runner (child killed + reaped on
/// expiry, no orphaned fetch lingering in the background).
pub fn fetch_upstream(dir: &std::path::Path, timeout: std::time::Duration) -> bool {
    let dir_str = dir.to_string_lossy().into_owned();
    crate::process::status_null_bounded(
        "git",
        &["-C", dir_str.as_str(), "fetch", "--quiet", "origin"],
        timeout,
    )
    .map(|s| s.success())
    .unwrap_or(false)
}

fn fetch_quiet(dir: &std::path::Path) -> bool {
    fetch_upstream(dir, std::time::Duration::from_secs(15))
}

/// Pager choice: config diff_pager > bat > less > cat. Never fails.
pub fn pager_cmd(cfg: &crate::config::Config) -> (&'static str, Vec<String>) {
    let pref = cfg.behavior.diff_pager.as_deref().unwrap_or("auto");
    match pref {
        "cat" => ("cat", vec![]),
        "less" => ("less", vec!["-R".into()]),
        "bat" => ("bat", vec!["--style=plain".into(), "--color=always".into()]),
        _ => {
            if which("bat") {
                ("bat", vec!["--style=plain".into(), "--color=always".into()])
            } else if which("less") {
                ("less", vec!["-R".into()])
            } else {
                ("cat", vec![])
            }
        }
    }
}

fn which(name: &str) -> bool {
    crate::process::has_tool(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_shows_changed_lines() {
        let old = "pkgver=1\npkgrel=1\ndepends=(foo)";
        let new = "pkgver=2\npkgrel=1\ndepends=(foo)";
        let d = render_diff(old, new, 50);
        assert!(d.contains("- pkgver=1"));
        assert!(d.contains("+ pkgver=2"));
    }

    #[test]
    fn diff_identical_reports_none() {
        assert_eq!(render_diff("a\nb", "a\nb", 50), "No PKGBUILD changes.");
    }

    #[test]
    fn diff_truncates() {
        let old: String = (0..100).map(|i| format!("line{i}\n")).collect();
        let new: String = (0..100).map(|i| format!("changed{i}\n")).collect();
        let d = render_diff(&old, &new, 10);
        assert!(d.contains("truncated"));
    }

    #[test]
    fn clone_dir_rejects_traversal() {
        let cfg = crate::config::Config::default();
        assert!(clone_dir_for(&cfg, "../evil").is_none());
        assert!(clone_dir_for(&cfg, "a/b").is_none());
        assert!(clone_dir_for(&cfg, "yay").is_some());
    }

    #[test]
    fn fetch_upstream_on_non_repo_is_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!fetch_upstream(
            dir.path(),
            std::time::Duration::from_secs(10)
        ));
    }
}
