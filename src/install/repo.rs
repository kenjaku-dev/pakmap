use anyhow::{Context, anyhow};
use std::process::Command;

use crate::safety::names::validate_package_name;

/// Install official repo package via sudo pacman -S (aur-guides:aur-pacman, aur-helpers)
/// Delegates to pacman binary — safer than reimplementing alpm transaction per plan.
pub fn install_repo_package(name: &str) -> anyhow::Result<()> {
    // v0.9.3: central allowlist shared with the GUI. Adds the leading-dash
    // rejection the old traversal-only guard missed — "-Syu" would land in
    // pacman's argv and read as a whole-system upgrade (CWE-88).
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("empty package name");
    }
    validate_package_name(name).map_err(|e| anyhow!("invalid package name: {name:?} ({e})"))?;

    // Check if already installed via pacman -Q (optional, just info).
    // Bounded (perf P2): None (stuck/missing) reads as "not installed".
    // `--` terminates pacman's option space before the target (defense in
    // depth; the allowlist above is the first half).
    let already = crate::process::output_bounded(
        "pacman",
        &["-Q", "--", name],
        crate::process::QUICK_TIMEOUT,
    )
    .map(|o| o.status.success())
    .unwrap_or(false);
    if already {
        eprintln!("Package {} already installed, reinstalling...", name);
    }

    // Build sudo pacman -S command inheriting tty
    // Do NOT use --noconfirm by default — let pacman ask. Caller can add --noconfirm via env
    let mut cmd = Command::new("sudo");
    cmd.args(["pacman", "-S", "--needed", "--", name]);

    // Inherit stdio so user sees pacman progress + can enter password
    cmd.stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("failed to run sudo pacman -S")?;
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        anyhow::bail!("pacman -S failed with exit {}", code);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Validation refuses before ANY spawn — these never touch pacman/sudo.
    #[test]
    fn option_shaped_and_traversal_names_rejected_before_spawn() {
        for bad in [
            "-Syu",
            "--needed",
            "-s",
            ".hidden",
            "../evil",
            "a/b",
            "two..dots",
            "foo bar",
            "",
            "   ",
        ] {
            assert!(
                install_repo_package(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn real_names_pass_validation() {
        // Probes the allowlist directly (the full install flow spawns sudo on
        // a live system — not a unit-testable path).
        for ok in ["htop", "libsigc++", "0ad", "python-orjson"] {
            assert_eq!(validate_package_name(ok), Ok(()), "{ok}");
        }
    }
}
