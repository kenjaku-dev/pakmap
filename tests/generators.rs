#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Generator flags (v0.10.3 Phase A): `--generate-completions` +
//! `--generate-man` run the real binary off the same clap parser. No
//! libalpm/network needed — CI-safe in the bare container.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn completions_bash() {
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--generate-completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_pakmap"))
        .stdout(predicate::str::contains("--no-tui"));
}

#[test]
fn completions_zsh() {
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--generate-completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("#compdef pakmap"));
}

#[test]
fn completions_fish() {
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--generate-completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -c pakmap"));
}

#[test]
fn man_page_renders() {
    Command::cargo_bin("pakmap")
        .unwrap()
        .arg("--generate-man")
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH pakmap 1"))
        // roff escapes hyphens: the synopsis lists `[\fB\-\-tui\fR]`
        .stdout(predicate::str::contains(r"\-\-tui"));
}

#[test]
fn unknown_shell_is_a_clap_error() {
    // clap_complete's Shell enum has no tcsh → the value_enum rejects it
    // before any generator code runs (exit 2, standard clap failure).
    Command::cargo_bin("pakmap")
        .unwrap()
        .args(["--generate-completions", "tcsh"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}
