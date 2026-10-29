//! Central package-name validation (CWE-88 hardening).
//!
//! One allowlist shared by the repo installer, remover, AUR flows and the GUI
//! (`pakmap::safety::names`). It refuses ONLY shapes that could turn into
//! something else — an option (`-Syu`), a hidden file (`.hidden`), a path
//! (`a/b`, `..`) or a control char — while every real-world package name
//! stays valid: `htop`, `0ad`, `libsigc++`, `python-orjson`,
//! `org.gnome.Builder`.

use thiserror::Error;

/// Practical upper bound: the filesystem (NAME_MAX 255) is the real ceiling
/// and real names stay far lower (deepest `python-*` names ~60 chars).
pub const NAME_MAX: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("empty name")]
    Empty,
    #[error("surrounding whitespace is not allowed")]
    Padding,
    #[error("too long ({0} > {1} byte cap)")]
    TooLong(usize, usize),
    #[error("'{0}' may not start a package name")]
    BadStart(char),
    #[error("character '{0}' is not allowed in a package name")]
    BadChar(char),
    #[error("a relative path component (\"..\") is not allowed")]
    Traversal,
}

/// Validate one untrusted package/object name.
///
/// Rules: non-empty after trim; no surrounding padding; ≤ [`NAME_MAX`]; the
/// first char must be ASCII alphanumeric; the rest may additionally be
/// `@ . _ + -`; `..` sequences are refused (traversal parity with the rest of
/// pakmap's guards); separators, spaces and control characters never pass.
pub fn validate_package_name(raw: &str) -> Result<(), NameError> {
    if raw.is_empty() {
        return Err(NameError::Empty);
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(NameError::Empty);
    }
    if trimmed.len() != raw.len() {
        return Err(NameError::Padding);
    }
    if trimmed.len() > NAME_MAX {
        return Err(NameError::TooLong(trimmed.len(), NAME_MAX));
    }
    if trimmed.contains("..") {
        return Err(NameError::Traversal);
    }
    match trimmed.chars().next() {
        // `0ad`-style digit-leading names are real; `.hidden` and `-Syu` are not.
        Some(first) if first.is_ascii_alphanumeric() => {}
        Some(first) => return Err(NameError::BadStart(first)),
        None => return Err(NameError::Empty),
    }
    for c in trimmed.chars() {
        if !(c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-')) {
            return Err(NameError::BadChar(c));
        }
    }
    Ok(())
}

/// Package targets prefixed with the POSIX `--` option terminator: a crafted
/// target can never be parsed as an option (`pacman -S --needed -Syu` would
/// otherwise read `-Syu` as a flag — a full, silent system upgrade). Empty
/// target lists return an empty vec, keeping option-only invocations
/// (`pacman -Sy`) byte-identical to today.
#[must_use]
pub fn terminated_targets(targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut v = Vec::with_capacity(targets.len() + 1);
    v.push("--".to_string());
    v.extend(targets.iter().cloned());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_world_names_pass() {
        for ok in [
            "htop",
            "0ad",
            "libsigc++",
            "python-orjson",
            "linux-zen-headers",
            "org.gnome.Builder",
            "a",
            "x1_2.3+4-5",
            "gtk_2",
            "foo-bar-2",
        ] {
            assert_eq!(validate_package_name(ok), Ok(()), "{ok} must pass");
        }
    }

    #[test]
    fn dangerous_names_fail() {
        // Option-shaped first token (the CWE-88 core).
        for bad in ["-Syu", "--noconfirm", "-", "-"] {
            assert!(validate_package_name(bad).is_err(), "{bad} must fail");
        }
        // Hidden/relative/path shapes.
        for bad in [
            ".hidden",
            "..",
            "../evil",
            "a/b",
            "a\\b",
            "two..dots",
            "a..b..c..",
            "nul\0name",
            "foo bar",
            "extra\tpadding",
            " foo",
            "foo ",
            "  ",
            "",
            "çok",
            "foo:bar",
            "foo;$PATH",
            "foo`x`",
        ] {
            assert!(
                validate_package_name(bad).is_err(),
                "{bad:?} must fail: {:?}",
                validate_package_name(bad)
            );
        }
        let long = "x".repeat(NAME_MAX + 1);
        assert!(matches!(
            validate_package_name(&long),
            Err(NameError::TooLong(_, _))
        ));
    }

    #[test]
    fn terminator_sits_before_targets_and_skips_empty_lists() {
        assert_eq!(
            terminated_targets(&["foo".to_string(), "-bar".to_string()]),
            vec!["--".to_string(), "foo".to_string(), "-bar".to_string()],
            "even 'looks-like-an-option' targets sit after the terminator"
        );
        assert_eq!(
            terminated_targets(&["-Syu".to_string()]),
            vec!["--".to_string(), "-Syu".to_string()]
        );
        assert!(
            terminated_targets(&[]).is_empty(),
            "no targets → no terminator (pacman -Sy shape unchanged)"
        );
    }
}
