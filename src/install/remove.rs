use anyhow::{Context, anyhow};
use std::process::Command;

use crate::config::Config;
use crate::safety::names::validate_package_name;

/// Remove an installed package via `sudo pacman -R<flags>` (remover mode).
/// Safety: validates name, refuses empty/traversal, previews with `pacman -Qi`,
/// then suspends via caller (TUI `suspend_and_run`) with inherited stdio.
pub fn remove_package(name: &str) -> anyhow::Result<()> {
    let cfg = Config::load();
    remove_package_with_config(name, &cfg)
}

pub fn remove_package_with_config(name: &str, cfg: &Config) -> anyhow::Result<()> {
    // v0.9.3: central allowlist. Same first-token guard as repo.rs — the name
    // can never parse as a pacman option, even after `--` (which is added
    // below as depth two).
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("empty package name");
    }
    validate_package_name(name).map_err(|e| anyhow!("invalid package name: {name:?} ({e})"))?;

    // Must be installed, otherwise `pacman -Rs` errors confusingly.
    // Bounded (perf P2): a stuck pacman fails closed here, never hangs TUI.
    let q = crate::process::output_bounded(
        "pacman",
        &["-Q", "--", name],
        crate::process::QUICK_TIMEOUT,
    )
    .map(|o| o.status.success())
    .unwrap_or(false);
    if !q {
        anyhow::bail!("package '{}' is not installed (pacman -Q failed)", name);
    }

    // Preview: show `pacman -Qi` so user sees size, deps, required-by.
    // Best-effort within a bound; a timeout skips the preview, not the flow.
    let _ = crate::process::status_inherit_bounded(
        "pacman",
        &["-Qi", "--", name],
        crate::process::QUICK_TIMEOUT,
    );

    let flag = cfg.remove_flag_arg();
    let mut cmd = Command::new("sudo");
    cmd.arg("pacman").arg(&flag);
    if cfg.remove_noconfirm() {
        cmd.arg("--noconfirm");
    }
    cmd.arg("--").arg(name);
    cmd.stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("failed to run sudo pacman -R")?;
    if !status.success() {
        anyhow::bail!(
            "pacman {} failed with exit {}",
            flag,
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_name_bails() {
        let cfg = Config::default();
        assert!(remove_package_with_config("", &cfg).is_err());
        assert!(remove_package_with_config("   ", &cfg).is_err());
    }

    #[test]
    fn traversal_names_rejected() {
        let cfg = Config::default();
        // ".." anywhere is rejected per policy (also covers "a..b..c.."),
        // plus path separators and NUL.
        for bad in ["../evil", "a/b", "a\\b", "a..b..c..", "..", "foo\0bar", ""] {
            assert!(
                remove_package_with_config(bad, &cfg).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    // v0.9.3 (CWE-88): option-shaped names refuse before any pacman spawn —
    // safe to exercise directly in tests.
    #[test]
    fn option_shaped_names_rejected_before_spawn() {
        let cfg = Config::default();
        for bad in ["-Syu", "--needed", "-Rs", ".hidden", "foo bar", "çok"] {
            assert!(
                remove_package_with_config(bad, &cfg).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
