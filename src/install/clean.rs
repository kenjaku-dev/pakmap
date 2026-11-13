use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::config::Config;

/// Preview of what `paccache` would remove. Always computed live
/// (`caching` skill: destructive previews must never be served stale).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CleanPreview {
    pub files: usize,
    pub bytes: u64,
    pub keep: u32,
}

/// Args for `paccache` after the binary name, extracted for unit testing
/// without spawning anything. Preview uses `-d` (dryrun op); real run uses
/// `-r` (remove op) — they are distinct operations in paccache 1.x.
pub fn clean_command_args(keep: u32, dry_run: bool) -> Vec<String> {
    let op = if dry_run { "-d" } else { "-r" };
    vec![
        op.to_string(),
        "-k".to_string(),
        keep.max(1).to_string(),
        "--nocolor".to_string(),
    ]
}

fn has_tool(name: &str) -> bool {
    crate::process::has_tool(name)
}

pub fn has_paccache() -> bool {
    has_tool("paccache")
}

/// Parse a `paccache` size token: `870.63 MiB`, `51.03 MiB`, `300 KiB`,
/// `2.0 GiB`, `512 B`. Returns bytes. Pure, unit-tested — paccache output
/// format drift must degrade to 0, never panic.
pub fn parse_size_token(num: f64, unit: &str) -> Option<u64> {
    if !num.is_finite() || num < 0.0 {
        return None;
    }
    let mult: f64 = match unit {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((num * mult) as u64)
}

/// Parse full `paccache -d` output: file count from the candidate list plus
/// bytes from the `disk space saved:` trailer. Defensive: unparseable output
/// yields counts of what was recognizable, never an error.
pub fn parse_dryrun(output: &str, keep: u32) -> CleanPreview {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut in_list = false;
    for line in output.lines() {
        let t = line.trim();
        if t.starts_with("==> Candidate packages:") {
            in_list = true;
            continue;
        }
        if t.starts_with("==> finished dry run:") {
            in_list = false;
            // `==> finished dry run: 16 candidates (disk space saved: 870.63 MiB)`
            if let Some(idx) = t.find("disk space saved:")
                && let (Some(n), Some(u)) = {
                    let rest = t[idx + "disk space saved:".len()..].trim();
                    let rest = rest.trim_end_matches(')');
                    let mut parts = rest.split_whitespace();
                    (parts.next(), parts.next())
                }
                && let Ok(num) = n.replace(',', ".").parse::<f64>()
                && let Some(b) = parse_size_token(num, u)
            {
                bytes = b;
            }
            continue;
        }
        if in_list && !t.is_empty() && !t.starts_with("==>") {
            files += 1;
        }
    }
    CleanPreview { files, bytes, keep }
}

/// Live preview via `paccache -d -kN`. Missing paccache or spawn failure is
/// an error with an install hint (caller maps to exit 1, not a panic).
/// Bounded (perf P2): a stuck paccache fails instead of hanging the caller.
pub fn preview_clean(keep: u32) -> anyhow::Result<CleanPreview> {
    if !has_paccache() {
        anyhow::bail!("paccache not found — install pacman-contrib to clean the cache");
    }
    let args = clean_command_args(keep, true);
    let mut full: Vec<String> = args;
    full.push("-v".to_string());
    let arg_refs: Vec<&str> = full.iter().map(|s| s.as_str()).collect();
    // Verbose lists candidates so the count is exact even if the trailer drifts.
    let output =
        crate::process::output_bounded("paccache", &arg_refs, crate::process::NETWORK_TIMEOUT)
            .context("paccache dry run timed out or failed to run")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_dryrun(&stdout, keep.max(1)))
}

/// Real clean via `sudo paccache -r -kN` with inherited stdio so the user
/// sees progress (`observability`: the tool's own output is the audit trail).
/// Caller owns confirmation; readonly is refused here too (defense in depth).
pub fn run_clean(cfg: &Config, keep: u32) -> anyhow::Result<()> {
    if cfg.is_readonly() {
        anyhow::bail!("refusing to clean: readonly mode is enabled");
    }
    if !has_paccache() {
        anyhow::bail!("paccache not found — install pacman-contrib to clean the cache");
    }
    let args = clean_command_args(keep, false);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let mut cmd = Command::new("sudo");
    if cfg.clean_noconfirm() {
        // paccache has no --noconfirm; `sudo -n` fails fast instead of hanging
        // on a password prompt in scripts.
        cmd.arg("-n");
    }
    cmd.arg("paccache").args(&arg_refs);
    cmd.stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    let status = cmd.status().context("failed to run sudo paccache -r")?;
    if !status.success() {
        anyhow::bail!(
            "paccache -r failed with exit {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

// ---- AUR clone-cache prune (v0.10.x phase 2) -------------------------------
//
// The clone cache (~/.cache/pakmap by default) accumulates one dir per AUR
// package ever cloned and nothing ever deleted it — stats only measured it.
// Pruning is settings-driven (`behavior.clean_clones_days`, 0 = off) and
// strictly user-owned: the cache-dir contract is "your own dir", so there is
// no sudo/pkexec anywhere in this path.

/// What `prune_stale_clones` would delete right now (or just deleted).
/// `days` echoes the configured threshold so every surface can render an
/// honest prompt; `days == 0` means the feature is off.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct ClonePlan {
    pub dirs: usize,
    pub bytes: u64,
    pub days: u32,
}

/// AUR package-name shape: lowercase alnum lead, then lowercase alnum and
/// `.` `_` `+` `-`. Rejects `.` `..` hidden and traversal-ish entries by
/// construction, so a cache-base join can never escape the base directory
/// (belt on top of names coming straight from `read_dir`).
pub fn is_aur_pkg_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() {
        return false;
    }
    let ok = |c: u8| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'+' | b'-')
    };
    if !ok(b[0]) || b[0] == b'.' {
        return false;
    }
    b[1..].iter().all(|&c| ok(c))
}

/// One stale clone dir found by the scan — prune's delete list.
struct StaleClone {
    name: String,
    bytes: u64,
}

/// Scan the cache base for stale clone dirs. Skip-on-error everywhere: a
/// missing base, an unreadable entry, or a hostile name contributes nothing
/// (the preview must never block cleaning). Files (e.g. the news cache) and
/// symlinks are never candidates; lstat semantics all the way down.
fn scan_detail(cfg: &Config) -> (Vec<StaleClone>, std::path::PathBuf) {
    let days = cfg.behavior.clean_clones_days;
    let base = crate::safety::fs::resolve_cache_base(cfg);
    if days == 0 {
        return (Vec::new(), base);
    }
    let Ok(rd) = std::fs::read_dir(&base) else {
        return (Vec::new(), base);
    };
    let now = std::time::SystemTime::now();
    let mut stale = Vec::new();
    for entry in rd.flatten() {
        let Ok(md) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if md.file_type().is_symlink() || !md.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_aur_pkg_name(&name) {
            continue;
        }
        // a future mtime (clock skew) yields age 0 → skip, never "old"
        let age = now
            .duration_since(md.modified().unwrap_or(now))
            .unwrap_or_default();
        if age.as_secs() < u64::from(days) * 86_400 {
            continue;
        }
        stale.push(StaleClone {
            bytes: crate::stats::dir_size_recursive(&entry.path(), 0),
            name,
        });
    }
    (stale, base)
}

/// Preview scan: how many AUR clone dirs under the cache base are older than
/// `behavior.clean_clones_days`. Cheap, local, never fatal.
pub fn scan_stale_clones(cfg: &Config) -> ClonePlan {
    let (stale, _) = scan_detail(cfg);
    ClonePlan {
        dirs: stale.len(),
        bytes: stale.iter().map(|s| s.bytes).sum(),
        days: cfg.behavior.clean_clones_days,
    }
}

/// Remove the stale clone dirs (user-owned — never root/pkexec). Re-verifies
/// each entry at delete time, so a scan→delete race (swap to symlink,
/// concurrent removal) cannot touch anything unexpected. Readonly is refused
/// here too, like `run_clean` (defense in depth).
pub fn prune_stale_clones(cfg: &Config) -> anyhow::Result<ClonePlan> {
    if cfg.is_readonly() {
        anyhow::bail!("refusing to prune clones: readonly mode is enabled");
    }
    let (stale, base) = scan_detail(cfg);
    let mut removed = ClonePlan {
        days: cfg.behavior.clean_clones_days,
        ..ClonePlan::default()
    };
    for c in stale {
        let path = base.join(&c.name);
        let Ok(md) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if md.file_type().is_symlink() || !md.is_dir() {
            continue;
        }
        std::fs::remove_dir_all(&path)
            .with_context(|| format!("failed to prune AUR clone dir {}", path.display()))?;
        removed.dirs += 1;
        removed.bytes += c.bytes;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_args_preview_vs_real() {
        assert_eq!(
            clean_command_args(2, true),
            vec!["-d", "-k", "2", "--nocolor"]
        );
        assert_eq!(
            clean_command_args(1, false),
            vec!["-r", "-k", "1", "--nocolor"]
        );
        // keep=0 clamps to 1 (keep nothing would nuke the cache).
        assert_eq!(clean_command_args(0, true)[2], "1");
    }

    #[test]
    fn parse_sizes() {
        assert_eq!(parse_size_token(512.0, "B"), Some(512));
        assert_eq!(parse_size_token(2.0, "GiB"), Some(2 * 1024 * 1024 * 1024));
        let mib = parse_size_token(870.63, "MiB").unwrap();
        assert!(mib > 870 * 1024 * 1024 && mib < 871 * 1024 * 1024);
        assert_eq!(parse_size_token(f64::NAN, "MiB"), None);
        assert_eq!(parse_size_token(-1.0, "MiB"), None);
        assert_eq!(parse_size_token(1.0, "XB"), None);
    }

    #[test]
    fn parse_dryrun_counts_and_bytes() {
        let out = "==> Candidate packages:\na-1.pkg.tar.zst\na-1.pkg.tar.zst.sig\nb-2.pkg.tar.zst\n==> finished dry run: 3 candidates (disk space saved: 51.03 MiB)\n";
        let p = parse_dryrun(out, 2);
        assert_eq!(p.files, 3);
        assert_eq!(p.keep, 2);
        assert!(p.bytes > 50 * 1024 * 1024 && p.bytes < 52 * 1024 * 1024);
    }

    #[test]
    fn parse_dryrun_empty_is_zero() {
        let p = parse_dryrun(
            "==> finished dry run: 0 candidates (disk space saved: 0 B)\n",
            2,
        );
        assert_eq!((p.files, p.bytes), (0, 0));
    }

    #[test]
    fn parse_dryrun_garbage_never_panics() {
        let p = parse_dryrun("garbage\n==> weird (disk space saved: lots)\n", 2);
        assert_eq!((p.files, p.bytes), (0, 0));
    }

    // ---- v0.10.x phase 2: AUR clone-cache prune ---------------------------

    use std::time::Duration;

    fn cfg_for(base: &std::path::Path, days: u32) -> Config {
        let mut cfg = Config::default();
        cfg.behavior.clean_clones_days = days;
        cfg.behavior.cache_dir = Some(base.display().to_string());
        cfg
    }

    /// A clone dir with one payload file; `old` backdates the DIR's own mtime
    /// (the scan ages entries by the dir's lstat mtime — writes inside keep a
    /// dir fresh, so an "old" dir must be backdated after its files exist).
    fn touch_clone(base: &std::path::Path, name: &str, old: bool) {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("payload.bin"), vec![7u8; 512]).unwrap();
        if old {
            let past = std::time::SystemTime::now() - Duration::from_secs(90 * 86_400);
            std::fs::File::open(&dir)
                .unwrap()
                .set_modified(past)
                .unwrap();
        }
    }

    #[test]
    fn aur_pkg_name_accepts_only_aur_shapes() {
        assert!(is_aur_pkg_name("yay"));
        assert!(is_aur_pkg_name("python-requests"));
        assert!(is_aur_pkg_name("0ad"));
        assert!(is_aur_pkg_name("pkg.name"));
        assert!(is_aur_pkg_name("pkg_name"));
        assert!(is_aur_pkg_name("pkg+name"));
        assert!(!is_aur_pkg_name(""));
        assert!(!is_aur_pkg_name("."));
        assert!(!is_aur_pkg_name(".."));
        assert!(!is_aur_pkg_name(".hidden"));
        assert!(!is_aur_pkg_name("Upper"));
        assert!(!is_aur_pkg_name("a/b"));
        assert!(!is_aur_pkg_name("a b"));
    }

    #[test]
    fn scan_stale_clones_filters_by_name_age_and_type() {
        let dir = tempfile::tempdir().unwrap();
        touch_clone(dir.path(), "old-clone", true);
        touch_clone(dir.path(), "fresh-clone", false);
        // non-dir and hostile names are never clone candidates
        std::fs::write(dir.path().join("arch-news.json"), b"{}").unwrap();
        std::fs::create_dir(dir.path().join(".weird")).unwrap();
        std::fs::create_dir(dir.path().join("Upper")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("old-clone"), dir.path().join("linked"))
            .unwrap();
        let cfg = cfg_for(dir.path(), 30);
        let p = scan_stale_clones(&cfg);
        assert_eq!(p.dirs, 1);
        assert_eq!(p.days, 30);
        assert!(p.bytes > 0);
    }

    #[test]
    fn scan_zero_days_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        touch_clone(dir.path(), "old-clone", true);
        assert_eq!(scan_stale_clones(&cfg_for(dir.path(), 0)).dirs, 0);
    }

    #[test]
    fn scan_missing_base_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(scan_stale_clones(&cfg_for(&missing, 30)).dirs, 0);
    }

    #[test]
    fn prune_removes_only_stale_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        touch_clone(dir.path(), "old-clone", true);
        touch_clone(dir.path(), "fresh-clone", false);
        let cfg = cfg_for(dir.path(), 30);
        let p = prune_stale_clones(&cfg).unwrap();
        assert_eq!(p.dirs, 1);
        assert!(!dir.path().join("old-clone").exists());
        assert!(dir.path().join("fresh-clone").exists());
        // second pass finds nothing (idempotent)
        assert_eq!(prune_stale_clones(&cfg).unwrap().dirs, 0);
    }

    #[test]
    fn prune_refuses_readonly() {
        let dir = tempfile::tempdir().unwrap();
        touch_clone(dir.path(), "old-clone", true);
        let mut cfg = cfg_for(dir.path(), 30);
        cfg.behavior.readonly = true;
        assert!(prune_stale_clones(&cfg).is_err());
        assert!(dir.path().join("old-clone").exists());
    }
}
