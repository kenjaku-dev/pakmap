use anyhow::Context;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::config::Config;

/// Cached `which` lookups — one spawn per tool per process instead of three
/// spawns on every AUR install.
static TOOL_CACHE: OnceLock<Mutex<HashMap<&'static str, bool>>> = OnceLock::new();

fn has_tool(name: &'static str) -> bool {
    let cache = TOOL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock()
        && let Some(&found) = guard.get(name)
    {
        return found;
    }
    let found = crate::process::has_tool(name);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(name, found);
    }
    found
}

/// Install AUR package: git clone + makepkg -si (aur-guides:aur-makepkg, aur-pkgbuild)
/// Follows AUR best practices: never run makepkg as root, show PKGBUILD, use --syncdeps
pub fn install_aur_package(pkg: &crate::model::Package) -> anyhow::Result<()> {
    // Safety: refuse if running as root — makepkg must not be root per Arch Wiki + aur-package-guidelines
    if nix::unistd::geteuid().is_root() {
        anyhow::bail!(
            "AUR install must not run as root — run pakmap as normal user, sudo will be used for pacman -U"
        );
    }

    let name = pkg.name.trim();
    if name.is_empty() {
        anyhow::bail!("empty package name");
    }
    // Central allowlist (v0.9.3): previously only traversal shapes were
    // refused; option-shaped names (`-Syu`) passed into later arg lists as
    // first-class targets. One shared policy with the GUI now.
    crate::safety::names::validate_package_name(name)
        .map_err(|e| anyhow::anyhow!("invalid package name: {name:?} ({e})"))?;

    // Loaded once and reused (cache policy, diff, noconfirm flags below).
    let cfg = Config::load();

    // Cache dir: previously created-then-quietly-chmod (CWE-377) — an
    // attacker-bootstrapped /tmp/pakmap was adopted silently (their dir, our
    // PKGBUILD review + makepkg). The shared helper verifies ownership,
    // permissions, symlink and parent up front and fails loudly; the
    // HOME-less default is now a per-UID private dir instead of the shared
    // /tmp/pakmap. Nothing is deleted on rejection — the error says how to fix.
    let cache_base = crate::safety::fs::ensure_cache_base(&cfg)
        .map_err(|e| anyhow::anyhow!("cache dir rejected: {e}"))?;
    let clone_dir = cache_base.join(name);

    // CWE-367 (part 1): the clone path itself must never sit under attacker
    // control — a planted symlink would route `git pull` through it.
    crate::safety::fs::ensure_private_dir_if_exists(&clone_dir)
        .map_err(|e| anyhow::anyhow!("clone dir rejected: {e}"))?;

    // Clone or pull. Bounded (perf P2): bulk transfer gets a long budget,
    // but a dead network still fails instead of hanging forever.
    // User watches progress via inherited stdio; Ctrl-C still works.
    if clone_dir.exists() {
        eprintln!("Updating existing clone {} ...", clone_dir.display());
        let pulled = crate::process::status_inherit_bounded(
            "git",
            &["-C", &clone_dir.to_string_lossy(), "pull", "--ff-only"],
            crate::process::CLONE_TIMEOUT,
        )
        .map(|s| s.success())
        .unwrap_or(false);
        if !pulled {
            eprintln!("git pull failed, continuing with existing clone...");
        }
    } else {
        let url = format!("https://aur.archlinux.org/{}.git", name);
        eprintln!("Cloning {} ...", url);
        let cloned = crate::process::status_inherit_bounded(
            "git",
            // `--` removes the URL from git's option space (defense in depth;
            // `name` is already allowlisted).
            &["clone", "--", &url, &clone_dir.to_string_lossy()],
            crate::process::CLONE_TIMEOUT,
        )
        .map(|s| s.success())
        .unwrap_or(false);
        if !cloned {
            anyhow::bail!("git clone failed for {}", url);
        }
    }

    // 0.5.0 safety: show uncommitted/local diff first (paru/yay review pattern).
    // `git diff` inside the clone surfaces pulled-but-unbuilt changes; first
    // clone has no diff -> falls through to full PKGBUILD preview below.
    if let Some(diff) = crate::diff::git_diff_cached(name, &cfg) {
        eprintln!("\n--- PKGBUILD diff (cached vs pulled) ---");
        let (pager, args) = crate::diff::pager_cmd(&cfg);
        if pager == "cat" {
            eprintln!("{diff}");
        } else {
            use std::io::Write;
            let mut child = Command::new(pager)
                .args(&args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn();
            if let Ok(ref mut c) = child
                && let Some(stdin) = c.stdin.as_mut()
            {
                let _ = stdin.write_all(diff.as_bytes());
            }
            if let Ok(mut c) = child {
                let _ = c.wait();
            }
        }
        eprintln!("--- end diff ---\n");
    }

    // Show PKGBUILD for review (aur-audit) — use bat if available, else cat.
    // v0.9.3: identity captured here, and re-verified with dev/ino right
    // before makepkg. A PKGBUILD swapped between "user reads it" and "user
    // runs it" (CWE-367 part 2) now aborts instead of building.
    let pkgbuild = clone_dir.join("PKGBUILD");
    let review_identity = crate::safety::fs::try_regular_file_identity(&pkgbuild)
        .map_err(|e| anyhow::anyhow!("PKGBUILD rejected for review: {e}"))?;
    if review_identity.is_some() {
        eprintln!("\n--- PKGBUILD: {} ---", pkgbuild.display());
        let mut cmd = if has_tool("bat") {
            let mut c = Command::new("bat");
            c.args([
                "--style=plain",
                "--color=always",
                pkgbuild.to_string_lossy().as_ref(),
            ]);
            c
        } else {
            let mut c = Command::new("cat");
            c.arg(&pkgbuild);
            c
        };
        let _ = cmd.status();
        eprintln!("--- end PKGBUILD ---\n");

        // Optional namcap audit if installed (aur-audit)
        if has_tool("namcap") {
            eprintln!("Running namcap audit...");
            let _ = Command::new("namcap")
                .arg(&pkgbuild)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();
        }

        // Optional shellcheck for PKGBUILD (aur-audit, aur-package-guidelines)
        if has_tool("shellcheck") {
            eprintln!("Running shellcheck...");
            let _ = Command::new("shellcheck")
                .args([
                    "--shell=bash",
                    "--exclude=SC2034,SC2154,SC2164",
                    &pkgbuild.to_string_lossy(),
                ])
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();
        }
    } else {
        eprintln!("Warning: PKGBUILD not found in {}", clone_dir.display());
    }

    // Confirm already done in TUI, but double-check if run from CLI
    // Ask once more if not in TUI? We assume TUI already confirmed, so proceed.

    // CWE-367 (part 2): the user reviewed THIS file — refuse anything that
    // changed or vanished between review and build.
    if let Some(id) = review_identity {
        crate::safety::fs::assert_unchanged(&pkgbuild, id)
            .map_err(|e| anyhow::anyhow!("PKGBUILD changed since review: {e}"))?;
    }

    // Build & install via makepkg -si (aur-makepkg) — default asks, --noconfirm only if explicit
    // Consistency with repo.rs: repo uses `pacman -S --needed` without --noconfirm; AUR should also ask by default
    // Enable non-interactive with PAKMAP_NOCONFIRM=1 or config behavior.makepkg_noconfirm
    let noconfirm = std::env::var("PAKMAP_NOCONFIRM").is_ok() || cfg.behavior.makepkg_noconfirm;
    eprintln!(
        "\nBuilding and installing {} with makepkg -si{} ...",
        name,
        if noconfirm { " --noconfirm" } else { "" }
    );
    let mut cmd = Command::new("makepkg");
    if noconfirm {
        cmd.args(["-si", "--noconfirm"]);
    } else {
        cmd.args(["-si"]);
    }
    cmd.current_dir(&clone_dir)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("makepkg -si")?;
    if !status.success() {
        anyhow::bail!("makepkg failed with exit {}", status.code().unwrap_or(-1));
    }

    eprintln!("\n✓ AUR package {} installed", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Package;

    fn pkg(name: &str) -> Package {
        Package::minimal(name.to_string(), "1.0".to_string(), "aur".to_string())
    }

    // Validation bails before any spawn/git/network touch — safe to call directly.
    #[test]
    fn option_shaped_and_traversal_names_rejected_before_clone() {
        for bad in [
            "-Syu",
            "--needed",
            "-s",
            ".hidden",
            "../evil",
            "a/b",
            "two..dots",
            "",
        ] {
            assert!(install_aur_package(&pkg(bad)).is_err(), "{bad:?} rejected");
        }
    }

    #[test]
    fn real_names_pass_the_gate() {
        for ok in ["libsigc++", "0ad", "yay-bin", "python-orjson"] {
            assert_eq!(
                crate::safety::names::validate_package_name(ok),
                Ok(()),
                "{ok} must stay valid"
            );
        }
    }
}
