#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Desktop metadata contract (v0.11.0 phase 3).
//!
//! Two layers: a source-level check that runs everywhere in `cargo test`
//! (identity/launchable/screenshots must be coherent with the desktop file),
//! plus the authoritative freedesktop validators when the tools are present
//! (CI installs `appstream` and `desktop-file-utils` for exactly this).

use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn desktop_dir() -> PathBuf {
    Path::new(ROOT).join("gui/desktop")
}

fn metainfo() -> String {
    std::fs::read_to_string(desktop_dir().join("pakmap-gui.metainfo.xml"))
        .expect("gui/desktop/pakmap-gui.metainfo.xml must exist")
}

#[test]
fn metainfo_identity_matches_the_desktop_file() {
    let xml = metainfo();
    assert!(
        xml.contains("<id>pakmap-gui.desktop</id>"),
        "the AppStream id must be the desktop file name"
    );
    assert!(
        xml.contains(r#"<launchable type="desktop-id">pakmap-gui.desktop</launchable>"#),
        "the launchable must point at the shipped .desktop entry"
    );
    let desktop = std::fs::read_to_string(desktop_dir().join("pakmap-gui.desktop")).unwrap();
    assert!(desktop.contains("Name=pakmap"));
    assert!(desktop.contains("Icon=pakmap-gui"));
    assert!(xml.contains("<name>pakmap</name>"));
}

#[test]
fn metainfo_screenshots_exist_and_are_real_pngs() {
    let xml = metainfo();
    let images: Vec<&str> = xml
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("<image")?; // attributes allowed
            let start = rest.find('>')? + 1;
            let end = rest.find("</image>")?;
            Some(&rest[start..end])
        })
        .collect();
    assert!(
        images.len() >= 2,
        "AppStream metadata must carry at least two screenshots"
    );
    for image in images {
        // Metadata uses stable raw.githubusercontent URLs; the shipped PNGs
        // live beside the metainfo so the release tarball can carry them.
        let file = image.rsplit('/').next().unwrap();
        let path = desktop_dir().join("screenshots").join(file);
        assert!(path.is_file(), "screenshot {} is missing", path.display());
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "{} is not a PNG",
            path.display()
        );
    }
}

fn tool_available(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn tool_validate(tool: &str, args: &[&str], target: &Path) {
    if !tool_available(tool) {
        // Local dev boxes without the tools still run the structural tests.
        return;
    }
    let out = Command::new(tool).args(args).arg(target).output().unwrap();
    assert!(
        out.status.success(),
        "{tool} rejected {}: {}{}",
        target.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn freedesktop_validators_accept_the_metadata() {
    // The desktop entry keeps its historic non-rDNS name (`pakmap-gui.desktop`):
    // renaming it would orphan every installed launcher and icon. Downgrade
    // that single future-compatibility warning; every other warning still
    // fails validation.
    tool_validate(
        "appstreamcli",
        &[
            "validate",
            "--no-net",
            "--override",
            "cid-desktopapp-is-not-rdns=info",
        ],
        &desktop_dir().join("pakmap-gui.metainfo.xml"),
    );
    tool_validate(
        "desktop-file-validate",
        &[],
        &desktop_dir().join("pakmap-gui.desktop"),
    );
}
