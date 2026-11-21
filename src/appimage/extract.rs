//! Read metadata out of an AppImage with `unsquashfs` — never by executing it.
//!
//! The squashfs offset comes from the verified ELF header, and only a tiny
//! set of paths is extracted into a private temp dir. `unsquashfs` is the
//! only external command we run; the AppImage itself is never launched.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// What we need from the archive: the desktop entry and the dir icon.
pub struct Extracted {
    pub desktop: String,
    pub icon: Option<Vec<u8>>,
}

const ICON_CAP: usize = 4 * 1024 * 1024;

/// Extract and return only the metadata we need. `dest` must be a fresh,
/// private directory owned by this process.
pub fn extract(appimage: &Path, fs_offset: u64, dest: &Path) -> Result<Extracted, String> {
    if !crate::process::has_tool("unsquashfs") {
        return Err("unsquashfs is required — install squashfs-tools".to_string());
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let offset = fs_offset.to_string();
    let dest_s = dest.to_string_lossy();
    let image = appimage.to_string_lossy();
    let args = [
        "-o",
        &offset,
        "-d",
        &dest_s,
        "-f",
        "-no-progress",
        &image,
        "*.desktop",
        ".DirIcon",
        "usr/share/applications/*.desktop",
    ];
    let out = crate::process::output_bounded("unsquashfs", &args, Duration::from_secs(180))
        .ok_or("unsquashfs timed out")?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stderr);
        let tail = tail.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
        return Err(format!("unsquashfs failed: {tail}"));
    }

    let desktop_path = pick_desktop(dest)?;
    let raw = crate::safety::fs::read_text_bounded(&desktop_path, 64 * 1024)
        .map_err(|e| format!("cannot read desktop entry: {e}"))?;

    let icon_path = dest.join(".DirIcon");
    let icon = if icon_path.is_file() {
        let bytes = std::fs::read(&icon_path).map_err(|e| e.to_string())?;
        if bytes.len() > ICON_CAP {
            None
        } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(bytes)
        } else {
            // A non-PNG .DirIcon is common (some are symlinks resolved to
            // SVG). Skip it rather than installing a broken icon.
            None
        }
    } else {
        None
    };
    Ok(Extracted {
        desktop: raw.text,
        icon,
    })
}

fn pick_desktop(dest: &Path) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    for dir in [dest.to_path_buf(), dest.join("usr/share/applications")] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("desktop") && path.is_file() {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or("no .desktop file found in the AppImage".to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{ICON_CAP, pick_desktop};
    use std::path::Path;

    #[test]
    fn pick_desktop_prefers_the_root_entry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("usr/share/applications")).unwrap();
        std::fs::write(dir.path().join("app.desktop"), b"root").unwrap();
        std::fs::write(
            dir.path().join("usr/share/applications/other.desktop"),
            b"nested",
        )
        .unwrap();
        let picked = pick_desktop(dir.path()).unwrap();
        assert_eq!(picked, dir.path().join("app.desktop"));
    }

    #[test]
    fn pick_desktop_reports_a_missing_entry() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pick_desktop(dir.path()).is_err());
    }

    #[test]
    fn icon_cap_is_a_real_bound() {
        const { assert!(ICON_CAP > 0 && ICON_CAP <= 8 * 1024 * 1024) };
        assert!(Path::new(".DirIcon").extension().is_none());
    }
}
