#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Hermetic `install.sh` contract tests (v0.11.0 phase 3).
//!
//! The install script is exercised as a real bash process against a fake
//! GitHub release: a stub fetcher serves fixtures from a local directory, a
//! stub `pacman` satisfies the Arch-only guard, and `--user` keeps the whole
//! run inside a temp HOME (no sudo, no network, no writes to the real system).
//! No fixture contents are trusted — every case is independent.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const VERSION_LINE_PREFIX: &str = "PAKMAP_VERSION=";

fn write_exec(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
    version: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["bin", "fixtures", "home", "tmp"] {
            fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        // Stub fetcher: resolves the requested basename against our fixtures.
        write_exec(
            &root.path().join("bin/curl"),
            r#"#!/usr/bin/env bash
set -euo pipefail
out=""; url=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -*) shift ;;
    *) [[ $1 == https://* ]] && url="$1"; shift ;;
  esac
done
[[ -n $url && -n $out ]] || exit 2
src="$PAKMAP_FIXTURES/${url##*/}"
[[ -f $src ]] || { printf 'no fixture for %s\n' "${url##*/}" >&2; exit 22; }
cp "$src" "$out"
"#,
        );
        write_exec(
            &root.path().join("bin/pacman"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let script =
            fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh")).unwrap();
        let version = script
            .lines()
            .find(|line| line.starts_with(VERSION_LINE_PREFIX))
            .and_then(|line| line.split('"').nth(1))
            .expect("install.sh must declare PAKMAP_VERSION")
            .to_string();
        fs::write(root.path().join("install.sh"), &script).unwrap();
        Self { root, version }
    }

    /// Build a synthetic CLI release tarball. `assets` adds the four files the
    /// v0.11.0 release must ship (name → contents).
    fn add_cli_tarball(&self, assets: &[(&str, &str)]) -> String {
        let stage = self.root.path().join("cli-stage");
        fs::create_dir_all(&stage).unwrap();
        write_exec(
            &stage.join("pakmap"),
            &format!(
                "#!/usr/bin/env bash\ncase \"${{1:-}}\" in\n  --version) echo \"pakmap {}\" ;;\n  --show-config) echo \"$HOME/.config/pakmap/config.toml\" ;;\n  *) echo \"smoke ok\" ;;\nesac\n",
                self.version
            ),
        );
        for (name, body) in assets {
            fs::write(stage.join(name), body).unwrap();
        }
        let tarball = format!("pakmap-{}-x86_64-linux.tar.gz", self.version);
        let out = Command::new("tar")
            .args([
                "-czf",
                self.root
                    .path()
                    .join("fixtures")
                    .join(&tarball)
                    .to_str()
                    .unwrap(),
            ])
            .args(["-C", stage.to_str().unwrap(), "."])
            .status()
            .unwrap();
        assert!(out.success(), "tar failed");
        tarball
    }

    fn add_manifest(&self, tarball: &str, contents: String) {
        fs::write(
            self.root.path().join("fixtures").join("SHASUMS256.txt"),
            contents,
        )
        .unwrap();
        let _ = tarball;
    }

    fn valid_manifest(&self, tarball: &str) -> String {
        let out = Command::new("sha256sum")
            .arg(self.root.path().join("fixtures").join(tarball))
            .output()
            .unwrap();
        assert!(out.status.success());
        let line = String::from_utf8(out.stdout).unwrap();
        let sum = line.split_whitespace().next().unwrap();
        format!("{sum}  {tarball}\n")
    }

    fn run(&self) -> Output {
        Command::new("bash")
            .arg(self.root.path().join("install.sh"))
            .args(["--user", "--no-gui", "--no-sync"])
            .env("HOME", self.root.path().join("home"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root.path().join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("PAKMAP_FIXTURES", self.root.path().join("fixtures"))
            .env("TMPDIR", self.root.path().join("tmp"))
            .output()
            .unwrap()
    }

    fn cli_dest(&self) -> std::path::PathBuf {
        self.root.path().join("home/.local/bin/pakmap")
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn valid_checksum_installs_the_cli() {
    let fx = Fixture::new();
    let tarball = fx.add_cli_tarball(&[]);
    let manifest = fx.valid_manifest(&tarball);
    fx.add_manifest(&tarball, manifest);
    let out = fx.run();
    assert!(out.status.success(), "install failed: {}", text(&out));
    assert!(fx.cli_dest().is_file(), "the CLI binary was not installed");
    assert!(text(&out).contains("sha256 verified"));
}

#[test]
fn missing_manifest_is_refused() {
    let fx = Fixture::new();
    fx.add_cli_tarball(&[]);
    // No SHASUMS256.txt fixture at all: the fetch fails.
    let out = fx.run();
    assert!(!out.status.success(), "missing manifest must fail closed");
    let msg = text(&out);
    assert!(msg.contains("SHASUMS256.txt"), "unexpected message: {msg}");
    assert!(msg.contains("refus"), "unexpected message: {msg}");
    assert!(
        !fx.cli_dest().exists(),
        "nothing must be installed on refusal"
    );
}

#[test]
fn checksum_mismatch_is_refused() {
    let fx = Fixture::new();
    let tarball = fx.add_cli_tarball(&[]);
    fx.add_manifest(&tarball, format!("{:064}  {tarball}\n", 0));
    let out = fx.run();
    assert!(!out.status.success(), "a wrong checksum must fail");
    assert!(text(&out).contains("sha256 MISMATCH"));
    assert!(
        !fx.cli_dest().exists(),
        "nothing must be installed on mismatch"
    );
}

/// The four integration artifacts are generated from the same clap parser by
/// the release job and must land in the distro-standard share paths — both in
/// system mode and (here) in `--user` mode.
#[test]
fn release_assets_are_installed_from_the_tarball() {
    let fx = Fixture::new();
    let tarball = fx.add_cli_tarball(&[
        ("pakmap.bash", "# bash completion for pakmap\n"),
        ("_pakmap", "#compdef pakmap\n"),
        ("pakmap.fish", "complete -c pakmap\n"),
        ("pakmap.1", ".TH pakmap 1\n"),
    ]);
    let manifest = fx.valid_manifest(&tarball);
    fx.add_manifest(&tarball, manifest);
    let out = fx.run();
    assert!(out.status.success(), "install failed: {}", text(&out));

    let share = fx.root.path().join("home/.local/share");
    for (rel, needle) in [
        ("bash-completion/completions/pakmap", "# bash completion"),
        ("zsh/site-functions/_pakmap", "#compdef"),
        ("fish/vendor_completions.d/pakmap.fish", "complete -c"),
        ("man/man1/pakmap.1", ".TH pakmap"),
    ] {
        let path = share.join(rel);
        assert!(path.is_file(), "missing installed asset {}", path.display());
        assert!(
            fs::read_to_string(&path).unwrap().contains(needle),
            "wrong contents in {}",
            path.display()
        );
    }
}

/// A tarball from an older release (before the assets were added) must still
/// install — only the binary is required, artifacts stay optional.
#[test]
fn legacy_tarball_without_assets_still_installs() {
    let fx = Fixture::new();
    let tarball = fx.add_cli_tarball(&[]);
    let manifest = fx.valid_manifest(&tarball);
    fx.add_manifest(&tarball, manifest);
    let out = fx.run();
    assert!(
        out.status.success(),
        "legacy install failed: {}",
        text(&out)
    );
    assert!(fx.cli_dest().is_file());
    assert!(
        !fx.root
            .path()
            .join("home/.local/share/man/man1/pakmap.1")
            .exists()
    );
}

/// Release-gate checker: called by `release.yml` on the real tarball, and
/// exercised here against synthetic ones so its red/green behavior is pinned.
fn check_assets(tarball: &Path) -> Output {
    Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/check-release-assets.sh"
        ))
        .arg(tarball)
        .output()
        .unwrap()
}

#[test]
fn release_asset_checker_accepts_complete_tarball() {
    let fx = Fixture::new();
    let name = fx.add_cli_tarball(&[
        ("pakmap.bash", ""),
        ("_pakmap", ""),
        ("pakmap.fish", ""),
        ("pakmap.1", ""),
    ]);
    let out = check_assets(&fx.root.path().join("fixtures").join(name));
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn release_asset_checker_rejects_incomplete_tarball() {
    let fx = Fixture::new();
    let name = fx.add_cli_tarball(&[("pakmap.bash", "")]);
    let out = check_assets(&fx.root.path().join("fixtures").join(name));
    assert!(!out.status.success(), "incomplete tarball must be rejected");
    let msg = text(&out);
    assert!(msg.contains("_pakmap"), "unexpected message: {msg}");
    assert!(msg.contains("pakmap.1"), "unexpected message: {msg}");
}

/// The source-install path: `make install-assets` must stage into DESTDIR and
/// honor PREFIX, using the binary's own generators (stubbed here).
#[test]
fn make_install_assets_honors_destdir_and_prefix() {
    let fx = Fixture::new();
    let stub = fx.root.path().join("bin/pakmap-assets");
    write_exec(
        &stub,
        r#"#!/usr/bin/env bash
case "$1" in
  --generate-completions)
    case "${2:-}" in
      bash) echo '# bash completion for pakmap' ;;
      zsh)  echo '#compdef pakmap' ;;
      fish) echo 'complete -c pakmap' ;;
      *) exit 2 ;;
    esac ;;
  --generate-man) echo '.TH pakmap 1' ;;
  *) exit 2 ;;
esac
"#,
    );
    let destdir = fx.root.path().join("make-root");
    let out = Command::new("make")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "install-assets",
            &format!("PAKMAP_BIN={}", stub.display()),
            &format!("DESTDIR={}", destdir.display()),
            "PREFIX=/usr",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "make failed: {}", text(&out));
    for (rel, needle) in [
        ("bash-completion/completions/pakmap", "# bash completion"),
        ("zsh/site-functions/_pakmap", "#compdef"),
        ("fish/vendor_completions.d/pakmap.fish", "complete -c"),
        ("man/man1/pakmap.1", ".TH pakmap"),
    ] {
        let path = destdir.join("usr/share").join(rel);
        assert!(path.is_file(), "missing staged asset {}", path.display());
        assert!(fs::read_to_string(&path).unwrap().contains(needle));
    }
}
