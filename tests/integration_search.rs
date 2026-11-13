#![allow(clippy::unwrap_used, clippy::expect_used)]
//! CLI integration tests (test-only crate: unwraps are idiomatic here).

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_works() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("pakmap"));
}

#[test]
fn help_shows_refresh_flag() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--refresh"));
}

#[test]
fn version_works() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("pakmap"));
}

#[test]
fn repo_search_smoke() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    cmd.args(["firefox", "--source", "repo", "--limit", "2", "--no-color"])
        .assert()
        .success();
}

#[test]
fn json_output_is_valid() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd
        .args(["neovim", "--limit", "1", "--json", "--no-color"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json should be valid");
    assert!(v.is_array());
}

#[test]
fn json_stdout_stays_pure() {
    // Logs (incl. backend timing warns) must go to stderr: stdout is the
    // data pipe. Regression: fmt subscriber once wrote to stdout.
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd
        .args(["neovim", "--limit", "1", "--json", "--no-color"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim_start().starts_with('['),
        "stdout must start with JSON, got: {}",
        stdout.chars().take(120).collect::<String>()
    );
    assert!(!stdout.contains("backend "));
}

#[test]
fn regex_recall_matches_substring() {
    // Regression: --regex must find what substring search finds. The repo
    // regex path scans all DB packages (never feeds the raw pattern to
    // libalpm's literal search), so `firef.x` must match `firefox`.
    for args in [
        vec!["firefox", "--source", "repo", "--limit", "5", "--no-color"],
        vec![
            "firef.x",
            "--source",
            "repo",
            "--limit",
            "5",
            "--no-color",
            "--regex",
        ],
    ] {
        let mut cmd = Command::cargo_bin("pakmap").unwrap();
        let output = cmd.args(&args).output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        assert!(
            stdout.contains("firefox"),
            "args {args:?} should match firefox"
        );
    }
}

#[test]
fn help_shows_new_maintenance_flags() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--updates"))
        .stdout(predicate::str::contains("--orphans"))
        .stdout(predicate::str::contains("--stats"))
        .stdout(predicate::str::contains("--upgrade"));
}

#[test]
fn updates_lists_valid_json() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd.args(["--updates", "--json"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("updates json valid");
    assert!(v.is_array());
}

#[test]
fn orphans_lists_valid_json() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd.args(["--orphans", "--json"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("orphans json valid");
    assert!(v.is_array());
}

#[test]
fn stats_valid_json() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd.args(["--stats", "--json"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stats json valid");
    assert!(v.get("explicit").is_some());
    assert!(v.get("orphans").is_some());
    assert!(v.get("updates_flatpak").is_some());
    assert!(v.get("flatpak_installed").is_some());
}

#[test]
fn readonly_refuses_mutating_ops() {
    for args in [
        vec!["--readonly", "--remove", "vim"],
        vec!["--readonly", "--remove", "org.mozilla.firefox"],
        vec!["--readonly", "--refresh"],
        vec!["--readonly", "--upgrade"],
    ] {
        let mut cmd = Command::cargo_bin("pakmap").unwrap();
        let output = cmd
            .args(&args)
            .env("PAKMAP_NOCONFIRM", "1")
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "args {args:?} must be refused in readonly"
        );
    }
}

fn paccache_present() -> bool {
    pakmap::install::clean::has_paccache()
}

#[test]
fn help_shows_clean_flags() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--clean"))
        .stdout(predicate::str::contains("--keep"))
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn clean_dry_run_json_is_valid() {
    // Needs paccache (pacman-contrib); skip gracefully off-Arch.
    if !paccache_present() {
        return;
    }
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd
        .args(["--clean", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("clean json valid");
    // v0.10.3 phase 3 contract: the report shape — preview (with the clone
    // scan) plus the planned actions — replaced v0.10.2's bare CleanPreview.
    // Action presence varies with the machine (flatpak, days>0); the shape
    // keys do not.
    assert_eq!(v["dry_run"], true);
    assert!(v["preview"]["files"].is_u64());
    assert!(v["preview"]["bytes"].is_u64());
    assert!(v["preview"]["keep"].is_u64());
    assert!(v["preview"]["clones"]["days"].is_u64());
    assert!(v["actions"].is_array());
}

#[test]
fn readonly_refuses_clean() {
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd
        .args(["--readonly", "--clean", "--dry-run"])
        .env("PAKMAP_NOCONFIRM", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
}

fn flatpak_present() -> bool {
    pakmap::sources::has_flatpak()
}

#[test]
fn flatpak_source_json_is_valid() {
    // Needs flatpak binary; skip gracefully off-machine.
    if !flatpak_present() {
        return;
    }
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd
        .args(["firefox", "--source", "flatpak", "--limit", "3", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("flatpak json valid");
    let arr = v.as_array().expect("flatpak json is array");
    assert!(
        arr.iter()
            .all(|p| p.get("repo").and_then(|r| r.as_str()) == Some("flatpak"))
    );
}

#[test]
fn flatpak_source_respects_limit() {
    if !flatpak_present() {
        return;
    }
    let mut cmd = Command::cargo_bin("pakmap").unwrap();
    let output = cmd
        .args(["firefox", "--source", "flatpak", "--limit", "1", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("flatpak json valid");
    assert!(v.as_array().map(|a| a.len() <= 1).unwrap_or(false));
}
