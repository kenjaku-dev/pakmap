#![allow(clippy::unwrap_used, clippy::expect_used)]
//! plan §2 — `pakmap config …` subcommand surface, driven via the real
//! binary (assert_cmd). Every test uses its own `--config` file so the
//! suite is hermetic against the dev machine's XDG config. Test-only crate:
//! unwraps are idiomatic here.

use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use serde_json::Value;

fn cmd() -> Command {
    Command::cargo_bin("pakmap").unwrap()
}

fn temp_config(dir: &tempfile::TempDir, text: &str) -> std::path::PathBuf {
    let p = dir.path().join("config.toml");
    std::fs::write(&p, text).unwrap();
    p
}

/// `config list --json`: every catalog key + the env info rows, with origins
/// and machine fields. (plan §2 list/§1.1 EnvOnly)
#[test]
fn list_json_contains_catalog_and_env_rows() {
    let out = cmd().args(["config", "list", "--json"]).output().unwrap();
    assert!(out.status.success());
    let items: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    for key in [
        "search.limit",
        "tui.border",
        "theme.text",
        "behavior.readonly",
        "gui.theme",
    ] {
        assert!(items.iter().any(|e| e["key"] == key), "missing {key}");
    }
    for env in [
        "PAKMAP_NOCONFIRM",
        "PAKMAP_TRUST_PROJECT",
        "PAKMAP_SKIP_LIVE",
    ] {
        assert!(
            items.iter().any(|e| e["key"] == env),
            "missing env row {env}"
        );
    }
    let limit = items.iter().find(|e| e["key"] == "search.limit").unwrap();
    assert_eq!(limit["apply"], "immediate");
    assert!(limit["value"].is_string());
}

/// `config list` table mode: 5-column human table, `--section` filter, and
/// `--json`-less default exit 0 even when the XDG config carries warnings.
#[test]
fn list_table_and_section_filter() {
    let out = cmd()
        .args(["config", "list", "--section", "gui"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("KEY"));
    assert!(text.contains("gui.theme"));
    assert!(
        !text.contains("search.limit"),
        "section filter isolates gui rows"
    );

    let all = cmd().args(["config", "list"]).output().unwrap();
    assert!(all.status.success());
    let alltext = String::from_utf8_lossy(&all.stdout);
    assert!(alltext.contains("behavior.readonly"));
    assert!(alltext.contains("danger"));
}

/// `config docs` — full generated example, same bytes `--init-config` writes.
#[test]
fn docs_prints_the_example_template() {
    let out = cmd().args(["config", "docs"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("# pakmap config"));
    assert!(text.contains("[search]"));
    assert!(text.contains("[tui]"));
    assert!(text.contains("[theme]"));
    assert!(text.contains("[behavior]"));
    assert!(text.contains("[gui]"));
    assert!(text.contains("PAKMAP_NOCONFIRM"), "env knobs documented");
}

/// `config path` echoes the (possibly replaced) user path.
#[test]
fn path_prints_the_user_path() {
    let out = cmd()
        .args(["config", "path", "--config", "/tmp/pakmap-some-path.toml"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("/tmp/pakmap-some-path.toml"));
}

/// `config get` — effective value; unknown key exits 2 with a suggestion.
#[test]
fn get_effective_value_and_unknown_key_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let p = temp_config(&dir, "[search]\nlimit = 21\n");
    let out = cmd()
        .args(["config", "get", "search.limit", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "21\n");

    let out = cmd()
        .args(["config", "get", "search.limt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("did you mean 'search.limit'"), "{err}");

    // The deprecated alias is readable, resolved onto its canonical key.
    let out = cmd()
        .args(["config", "get", "behavior.bottom_up", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("deprecated alias"), "{err}");
}

/// `config set` — writes per kind, preserves comments, strict door codes:
/// 2 unknown-key / invalid-value, 0 ok; file never touched on failure.
#[test]
fn set_roundtrip_and_refusal_codes() {
    let dir = tempfile::tempdir().unwrap();
    let p = temp_config(&dir, "# my comment\n[search]\nlimit = 5 # tail\n");
    let out = cmd()
        .args(["config", "set", "search.limit", "34", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "set failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&p).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("limit = 34 # tail"));
    assert!(text.contains("# my comment"));

    let out = cmd()
        .args(["config", "get", "search.limit", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "34\n");

    // Invalid value → exit 2 with the clamp, file untouched.
    let out = cmd()
        .args(["config", "set", "tui.debounce_ms", "0", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("50..=5000"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&p).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("limit = 34"));

    // Unknown key → exit 2.
    let out = cmd()
        .args(["config", "set", "search.likmit", "3", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// `config set --yes` bootstraps a missing user file from the template.
#[test]
fn set_yes_bootstraps_the_user_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("fresh.toml");
    let out = cmd()
        .args(["config", "set", "search.limit", "9", "--config"])
        .arg(&p)
        .args(["--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("Created"));
    let bytes = std::fs::read(&p).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.starts_with("# pakmap config"));
    assert!(text.contains("limit = 9"));
    let mode = std::fs::metadata(&p).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o644);
}

/// `config unset` — value falls back to default; empty table tidied up.
#[test]
fn unset_falls_back_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let p = temp_config(&dir, "[search]\nlimit = 21\n");
    let out = cmd()
        .args(["config", "unset", "search.limit", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    let bytes = std::fs::read(&p).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("[search]"), "{text}");
    let out = cmd()
        .args(["config", "get", "search.limit", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "50\n");
}

/// `config validate` — findings printed, exit 0 unless `--strict`; JSON form
/// carries kind/suggestion/line (plan §2).
#[test]
fn validate_reports_findings_with_codes() {
    let dir = tempfile::tempdir().unwrap();
    let p = temp_config(
        &dir,
        "[search]\nlimit = 5\ntypo = 1\n[tui]\ndebounce_ms = 0\n[behavior]\nbottom_up = true\n",
    );
    let out = cmd()
        .args(["config", "validate", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("'search.typo'"), "{text}");
    assert!(text.contains("'tui.debounce_ms'"), "{text}");
    assert!(text.contains("deprecated alias"), "{text}");
    assert!(text.contains("findings"), "{text}");

    let out = cmd()
        .args(["config", "validate", "--strict", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    let out = cmd()
        .args(["config", "validate", "--json", "--config"])
        .arg(&p)
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let kinds: Vec<&str> = doc["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"unknown-key"));
    assert!(kinds.contains(&"invalid-value"));
    assert!(kinds.contains(&"alias"));
    assert_eq!(doc["count"], doc["findings"].as_array().unwrap().len());

    let out = cmd()
        .args(["config", "validate", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
}
