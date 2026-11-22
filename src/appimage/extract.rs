//! Read metadata out of an AppImage with `unsquashfs` — never by executing it.
//!
//! Nothing is extracted to disk: the archive is listed first (sizes and types),
//! oversize/non-regular entries are rejected, and only the chosen desktop
//! entry/icon are streamed with `-cat` under a hard cap. That removes the old
//! disk-fill, OOM and host-symlink read-through surfaces in one move.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// What we need from the archive: the desktop entry and the dir icon.
pub struct Extracted {
    pub desktop: String,
    pub icon: Option<Vec<u8>>,
}

const DESKTOP_CAP: u64 = 64 * 1024;
const ICON_CAP: u64 = 4 * 1024 * 1024;
const LIST_TIMEOUT: Duration = Duration::from_secs(120);
const CAT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    kind: char,
    path: String,
    size: u64,
    target: Option<String>,
}

/// Read and return only the metadata we need. The image path is canonicalized
/// so a path beginning with `-` can never be parsed as an unsquashfs option.
pub fn extract(appimage: &Path, fs_offset: u64) -> Result<Extracted, String> {
    if !crate::process::has_tool("unsquashfs") {
        return Err("unsquashfs is required — install squashfs-tools".to_string());
    }
    let image: PathBuf = std::fs::canonicalize(appimage)
        .map_err(|e| format!("cannot resolve AppImage path: {e}"))?;
    let image = image.to_string_lossy().into_owned();
    let offset = fs_offset.to_string();

    let listed = list(
        &image,
        &offset,
        &["*.desktop", "usr/share/applications/*.desktop", ".DirIcon"],
    )?;
    let desktop = pick_desktop(&listed)?;
    if desktop.size > DESKTOP_CAP {
        return Err(format!(
            "desktop entry is {} bytes, above the {DESKTOP_CAP} byte cap",
            desktop.size
        ));
    }
    let raw = cat(&image, &offset, &desktop.path, DESKTOP_CAP)?;
    let text = String::from_utf8(raw).map_err(|_| "desktop entry is not valid UTF-8")?;
    let icon = read_icon(&image, &offset, &listed)?;
    Ok(Extracted {
        desktop: text,
        icon,
    })
}

fn list(image: &str, offset: &str, patterns: &[&str]) -> Result<Vec<Listed>, String> {
    let mut args = vec!["-o", offset, "-lls", image];
    args.extend_from_slice(patterns);
    let out = crate::process::output_bounded("unsquashfs", &args, LIST_TIMEOUT)
        .ok_or("unsquashfs timed out while listing the archive")?;
    if !out.status.success() {
        return Err(format!(
            "unsquashfs could not list the archive: {}",
            stderr_tail(&out.stderr)
        ));
    }
    Ok(parse_listing(&String::from_utf8_lossy(&out.stdout)))
}

fn cat(image: &str, offset: &str, path: &str, cap: u64) -> Result<Vec<u8>, String> {
    if !safe_archive_path(path) {
        return Err("archive entry has an unsafe path".to_string());
    }
    let out = crate::process::output_bounded(
        "unsquashfs",
        &["-o", offset, "-cat", image, path],
        CAT_TIMEOUT,
    )
    .ok_or("unsquashfs timed out while reading an archive entry")?;
    if !out.status.success() {
        return Err(format!(
            "unsquashfs could not read {path}: {}",
            stderr_tail(&out.stderr)
        ));
    }
    if out.stdout.len() as u64 > cap {
        return Err(format!("{path} exceeds the {cap} byte cap"));
    }
    Ok(out.stdout)
}

fn stderr_tail(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Parse `unsquashfs -lls` output. Only regular files (`-`) are usable; the
/// caller rejects every other type. Paths are stripped of the archive's
/// single root component (`squashfs-root/`).
fn parse_listing(stdout: &str) -> Vec<Listed> {
    let pattern = r"^([-dlbcps])\S{9}\s+\S+\s+(\d+)\s+\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}\s+(.+)$";
    let Ok(re) = regex::Regex::new(pattern) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in stdout.lines() {
        let Some(caps) = re.captures(line) else {
            continue;
        };
        let Some(kind) = caps.get(1).and_then(|m| m.as_str().chars().next()) else {
            continue;
        };
        let Some(size) = caps.get(2).and_then(|m| m.as_str().parse::<u64>().ok()) else {
            continue;
        };
        let Some(raw_path) = caps.get(3).map(|m| m.as_str()) else {
            continue;
        };
        let (raw_path, target) = match raw_path.rsplit_once(" -> ") {
            Some((path, target)) => (path, Some(target.to_string())),
            None => (raw_path, None),
        };
        let Some((_, path)) = raw_path.split_once('/') else {
            continue;
        };
        if !safe_archive_path(path) {
            continue;
        }
        out.push(Listed {
            kind,
            path: path.to_string(),
            size,
            target,
        });
    }
    out
}

fn pick_desktop(listed: &[Listed]) -> Result<&Listed, String> {
    let mut candidates: Vec<&Listed> = listed
        .iter()
        .filter(|entry| {
            entry.kind == '-'
                && entry.path.ends_with(".desktop")
                && (!entry.path.contains('/') || entry.path.starts_with("usr/share/applications/"))
        })
        .collect();
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| "no regular .desktop file found in the AppImage".to_string())
}

fn read_icon(image: &str, offset: &str, listed: &[Listed]) -> Result<Option<Vec<u8>>, String> {
    let Some(dir_icon) = listed.iter().find(|entry| entry.path == ".DirIcon") else {
        return Ok(None);
    };
    let path = match dir_icon.kind {
        '-' => dir_icon.path.clone(),
        'l' => {
            let Some(target) = dir_icon.target.as_deref().and_then(resolve_link) else {
                return Ok(None);
            };
            // The target must itself be a regular, bounded entry. List it
            // explicitly; a symlink chain is deliberately not followed.
            let target_listed = list(image, offset, &[&target])?;
            let Some(target_entry) = target_listed
                .iter()
                .find(|entry| entry.path == target && entry.kind == '-')
            else {
                return Ok(None);
            };
            if target_entry.size > ICON_CAP {
                return Ok(None);
            }
            target
        }
        _ => return Ok(None),
    };
    if path != ".DirIcon" && !safe_archive_path(&path) {
        return Ok(None);
    }
    let bytes = match cat(image, offset, &path, ICON_CAP) {
        Ok(bytes) => bytes,
        // An icon is optional; a broken one must not fail the integration.
        Err(_) => return Ok(None),
    };
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(Some(bytes))
    } else {
        // A non-PNG .DirIcon is common (SVG, or a symlink to one). Skip it
        // rather than installing a broken icon.
        Ok(None)
    }
}

/// Resolve a relative symlink target inside the archive without ever leaving
/// it. Absolute targets, `..` escapes and glob characters are refused.
fn resolve_link(target: &str) -> Option<String> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.chars().any(char::is_control)
        || target.contains(['*', '?', '['])
    {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// Paths that can be handed to `unsquashfs -cat` safely: relative, no control
/// characters, no traversal, and no glob metacharacters (unsquashfs would
/// treat the argument as a pattern and could match a different entry).
fn safe_archive_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.chars().any(char::is_control)
        && !path.contains(['*', '?', '['])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{DESKTOP_CAP, ICON_CAP, Listed, parse_listing, pick_desktop, resolve_link};

    const LISTING: &str = "\
lrwxrwxrwx root/root                 9 2026-10-29 08:58 squashfs-root/.DirIcon -> Prism.png
lrwxrwxrwx root/root                36 2026-10-29 08:59 squashfs-root/Prism.desktop -> usr/share/applications/Prism.desktop
-rw-r--r-- root/root               231 2026-10-29 08:58 squashfs-root/usr/share/applications/Prism.desktop
-rw-r--r-- root/root             51273 2026-10-29 08:58 squashfs-root/Prism.png
";

    fn listed(kind: char, path: &str, size: u64) -> Listed {
        Listed {
            kind,
            path: path.to_string(),
            size,
            target: None,
        }
    }

    #[test]
    fn parses_real_unsquashfs_output_and_strips_the_root() {
        let entries = parse_listing(LISTING);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].path, ".DirIcon");
        assert_eq!(entries[0].target.as_deref(), Some("Prism.png"));
        assert_eq!(entries[2].path, "usr/share/applications/Prism.desktop");
        assert_eq!(entries[2].size, 231);
    }

    #[test]
    fn picks_the_regular_desktop_entry_and_ignores_symlinks() {
        let entries = parse_listing(LISTING);
        let picked = pick_desktop(&entries).unwrap();
        assert_eq!(picked.path, "usr/share/applications/Prism.desktop");
        assert!(pick_desktop(&[]).is_err());
        assert!(pick_desktop(&[listed('l', "Evil.desktop", 1)]).is_err());
    }

    #[test]
    fn resolves_only_safe_relative_links() {
        assert_eq!(resolve_link("Prism.png").as_deref(), Some("Prism.png"));
        assert_eq!(
            resolve_link("usr/share/icons/app.png").as_deref(),
            Some("usr/share/icons/app.png")
        );
        assert_eq!(resolve_link("/etc/passwd"), None);
        assert_eq!(resolve_link("../../etc/passwd"), None);
        assert_eq!(resolve_link("a/../../b"), None);
        assert_eq!(resolve_link("a\\b"), None);
        assert_eq!(resolve_link("*.png"), None);
        assert_eq!(resolve_link(""), None);
    }

    #[test]
    fn caps_are_real_bounds() {
        const { assert!(DESKTOP_CAP > 0 && DESKTOP_CAP <= 1024 * 1024) };
        const { assert!(ICON_CAP > 0 && ICON_CAP <= 8 * 1024 * 1024) };
    }
}
