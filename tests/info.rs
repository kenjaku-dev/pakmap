#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `--info PKG` (v0.10.3 Phase B): read-only single-package resolution.
//! The not-found path is offline-safe: every source fails open (missing
//! repo DBs, absent AUR, no flatpak → skipped), so the deterministic result
//! is exit 1 + the stderr hint regardless of environment.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn info_unknown_package_exits_1_with_stderr_hint() {
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--info", "zzz-no-such-package-zzz", "--no-tui"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no package named"));
}

#[test]
fn info_requires_a_value() {
    Command::cargo_bin("pakmap")
        .unwrap()
        .arg("--info")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn info_json_not_found_keeps_stdout_pure() {
    // Contract: with --json, stdout carries a JSON object or nothing — the
    // error note only ever goes to stderr (scripts pipe stdout unfiltered).
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--info", "zzz-no-such-package-zzz", "--json"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[test]
fn info_resolves_repo_package() {
    // `pacman` is always installed on the dev box and always in the sync
    // DBs; resolution order puts the installed hit first (repo=local).
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--info", "pacman", "--no-tui"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pacman"));
}
