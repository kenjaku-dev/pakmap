//! Comment-preserving atomic config writer (plan §1.6).
//!
//! `toml_edit::DocumentMut` round-trips an existing file so `set` only
//! replaces the edited line's VALUE (key decor and trailing comments stay;
//! new tables append in catalog order). Writes are atomic
//! (tmp → fsync → rename → fsync dir best-effort) with a
//! `(mtime, len, head32, tail32)` fingerprint conflict check so an editor or
//! a concurrent pakmap can never be clobbered. No new crates: the writer
//! reuses `toml_edit 0.20.2`, already pinned in the tree via `toml 0.8`.

use std::path::{Path, PathBuf};

use super::PROJECT_ALLOWED_SECTIONS;
use super::catalog::{SettingMeta, section_of};

pub use super::validate::validate as validate_value;

/// Which file a write targets (plan D2: user file default; project file is
/// gated — never implicitly created by the engine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The user's XDG config, or `--config PATH` when given. `create`
    /// bootstraps a missing file from the example template (the CLI asks
    /// first; the engine never prompts).
    User { path: PathBuf, create: bool },
    /// `./pakmap.toml`-style project file. `trusted` mirrors
    /// `PAKMAP_TRUST_PROJECT=1`. The stated path must already exist.
    Project { path: PathBuf, trusted: bool },
}

/// Distinct write-failure taxonomy (plan §1.6); `code()`/`message()` are the
/// stable surface the CLI (and later the GUI) map to hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetError {
    UnknownKey(String),
    InvalidValue(String),
    Unparseable(String),
    Conflict(String),
    PolicyRefused(String),
    Io(String),
}

impl SetError {
    pub fn code(&self) -> &'static str {
        match self {
            SetError::UnknownKey(_) => "unknown-key",
            SetError::InvalidValue(_) => "invalid-value",
            SetError::Unparseable(_) => "unparseable",
            SetError::Conflict(_) => "conflict",
            SetError::PolicyRefused(_) => "policy-refused",
            SetError::Io(_) => "io",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SetError::UnknownKey(m)
            | SetError::InvalidValue(m)
            | SetError::Unparseable(m)
            | SetError::Conflict(m)
            | SetError::PolicyRefused(m)
            | SetError::Io(m) => m.clone(),
        }
    }
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// Success detail of a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetOutcome {
    pub path: PathBuf,
    /// true when the user file was bootstrapped from the example template.
    pub created: bool,
    /// post-write scan findings (warnings list, plan §2 set output).
    pub warnings: Vec<String>,
}

/// (mtime_secs, len, first 32B, last 32B) — zero new deps (plan §1.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    mtime_secs: u64,
    len: u64,
    head: [u8; 32],
    tail: [u8; 32],
}

fn fingerprint_of(meta: &std::fs::Metadata, bytes: &[u8]) -> Fingerprint {
    let mtime_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut head = [0u8; 32];
    for (i, b) in bytes.iter().take(32).enumerate() {
        head[i] = *b;
    }
    let mut tail = [0u8; 32];
    let start = bytes.len().saturating_sub(32);
    for (i, b) in bytes[start..].iter().enumerate() {
        tail[i] = *b;
    }
    Fingerprint {
        mtime_secs,
        len: meta.len(),
        head,
        tail,
    }
}

fn file_fingerprint(path: &Path) -> Result<Fingerprint, SetError> {
    let bytes = std::fs::read(path)
        .map_err(|e| SetError::Io(format!("cannot read {}: {e}", path.display())))?;
    let md = std::fs::symlink_metadata(path)
        .map_err(|e| SetError::Io(format!("cannot stat {}: {e}", path.display())))?;
    Ok(fingerprint_of(&md, &bytes))
}

/// Set `key = value` in `target` per plan §1.6.
pub fn set(key: &str, value: &str, target: &Target) -> Result<SetOutcome, SetError> {
    let meta = lookup_or_alias_err(key)?;
    validate_value(meta, value).map_err(|msg| SetError::InvalidValue(format!("'{key}': {msg}")))?;
    apply_write(meta, value, target, None)
}

/// Remove `key` (and its table, when the table ends up empty — plan §1.6
/// keeps files tidy). Missing user file / missing key are idempotent no-ops.
pub fn unset(key: &str, target: &Target) -> Result<SetOutcome, SetError> {
    let meta = lookup_or_alias_err(key)?;
    apply_write(meta, "", target, Some(key))
}

fn lookup_or_alias_err(key: &str) -> Result<&'static SettingMeta, SetError> {
    if key == super::BOTTOM_UP_ALIAS {
        return Err(SetError::UnknownKey(
            "behavior.bottom_up is a deprecated alias — use `[search] bottom_up` \
             (pakmap config set search.bottom_up <true|false>)"
                .into(),
        ));
    }
    SettingMeta::lookup(key).ok_or_else(|| {
        let near = super::validate::nearest_key(key);
        let hint = near
            .map(|n| format!(" — did you mean '{n}'?"))
            .unwrap_or_default();
        SetError::UnknownKey(format!(
            "unknown setting key '{key}'{hint}; run `pakmap config docs` for all keys"
        ))
    })
}

/// D2 policy gate: project writes are search/tui/theme (+ behavior & gui
/// when `PAKMAP_TRUST_PROJECT=1`); project files are never created.
fn check_policy(meta: &SettingMeta, target: &Target, path: &Path) -> Result<(), SetError> {
    let Target::Project { trusted, .. } = target else {
        return Ok(());
    };
    let section = section_of(meta.key);
    if *trusted {
        // Full trust: everything is writable, as the read path already honors.
        return Ok(());
    }
    if PROJECT_ALLOWED_SECTIONS.contains(&section) {
        return Ok(());
    }
    let msg = if section == "behavior" {
        format!(
            "refusing '[{section}]' write into project file {} — project-local configs are \
             untrusted; set PAKMAP_TRUST_PROJECT=1 to allow, or write to the user config \
             (`pakmap config set {key} <value>`)",
            path.display(),
            key = meta.key,
            section = section,
        )
    } else {
        format!(
            "refusing '[{section}]' write into project file {} — allowed project sections \
             are search, tui, theme (`PAKMAP_TRUST_PROJECT=1` opts out)",
            path.display(),
            section = section,
        )
    };
    Err(SetError::PolicyRefused(msg))
}

fn apply_write(
    meta: &'static SettingMeta,
    value: &str,
    target: &Target,
    unset_key: Option<&str>,
) -> Result<SetOutcome, SetError> {
    let (path, allow_create) = match target {
        Target::User { path, create } => {
            // Follow a symlinked user config (common in dotfile setups) so
            // the atomic rename updates the real file, not the link.
            let real = if path.is_symlink() {
                std::fs::canonicalize(path)
                    .map_err(|e| SetError::Io(format!("cannot resolve {}: {e}", path.display())))?
            } else {
                path.clone()
            };
            (real, *create)
        }
        Target::Project { path, .. } => (path.clone(), false),
    };

    let mut created = false;
    if !path.exists() {
        if unset_key.is_some() {
            // Unset against a missing user file is a no-op (already default);
            // a project file must exist per D2 (never implicitly created).
            if matches!(target, Target::User { .. }) {
                return Ok(SetOutcome {
                    path,
                    created: false,
                    warnings: Vec::new(),
                });
            }
            return Err(SetError::PolicyRefused(format!(
                "project config {} does not exist — create it yourself (never implicit), \
                 e.g. `pakmap config docs > {}`",
                path.display(),
                path.display()
            )));
        }
        if !allow_create {
            return Err(SetError::PolicyRefused(format!(
                "config {} does not exist — create it with \
                 `pakmap config docs > {}` (or retry with --yes)",
                path.display(),
                path.display()
            )));
        }
        bootstrap(&path)?;
        created = true;
    } else {
        // Owner/symlink/special-file refusal (safety::fs discipline).
        if let Err(e) = crate::safety::fs::ensure_owned_file(&path) {
            return Err(SetError::PolicyRefused(format!(
                "refusing to write {}: {e}",
                path.display()
            )));
        }
    }

    let before = file_fingerprint(&path)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| SetError::Io(format!("cannot read {}: {e}", path.display())))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| SetError::Io(format!("{} is not valid UTF-8", path.display())))?;
    check_policy(meta, target, &path)?;

    let mut doc = text.parse::<toml_edit::Document>().map_err(|e| {
        SetError::Unparseable(format!(
            "{} is not a valid TOML document — fix it by hand first: {e}",
            path.display()
        ))
    })?;

    let section = section_of(meta.key).to_string();
    let file_key = meta.key.split('.').nth(1).unwrap_or(meta.key);
    let root = doc.as_table_mut();
    if unset_key.is_some() {
        // Idempotent unset: a not-present key (or file) is success; a table
        // that empties out is removed to keep the file tidy.
        if let Some(item) = root.get_mut(&section)
            && let Some(tbl) = item.as_table_mut()
        {
            tbl.remove(file_key);
            if tbl.is_empty() {
                root.remove(&section);
            }
        }
    } else {
        let item = match root.get_mut(&section) {
            Some(item) => item,
            None => {
                // Absent table: append in catalog order (existing tables
                // never move; the new one lands after the last table).
                let mut tbl = toml_edit::Table::new();
                tbl.decor_mut()
                    .set_prefix(if text.trim().is_empty() { "" } else { "\n" });
                root.insert(&section, toml_edit::Item::Table(tbl));
                root.get_mut(&section).ok_or_else(|| {
                    SetError::Io(format!("cannot insert [{section}] into {}", path.display()))
                })?
            }
        };
        let some = item.as_table_mut().ok_or_else(|| {
            SetError::Unparseable(format!(
                "[{section}] is not a plain table — edit {} by hand",
                path.display()
            ))
        })?;
        let mut new_value = build_edit_value(meta, value)?;
        match some.get_mut(file_key) {
            Some(existing) => {
                // Replace value only: keep the key's own decor and any
                // trailing comment (`key = old # comment`).
                let keep_suffix = existing
                    .as_value()
                    .and_then(|v| v.decor().suffix())
                    .map(|s| s.to_owned());
                if let Some(sfx) = keep_suffix {
                    new_value.decor_mut().set_suffix(sfx);
                }
                *existing = toml_edit::Item::Value(new_value);
            }
            None => {
                some.insert(file_key, toml_edit::Item::Value(new_value));
            }
        }
    }

    let edited = doc.to_string();

    // Conflict gate: the on-disk file must still match what we read.
    commit_with_conflict_gate(&path, &edited, before, meta.key)?;

    let warnings = scan_warnings(&edited, &path);
    Ok(SetOutcome {
        path,
        created,
        warnings,
    })
}

/// The conflict gate + commit split out of `apply_write` (plan §1.6): the
/// on-disk file must still match the fingerprint taken before the edit — an
/// editor or a concurrent pakmap is never clobbered. Kept as a small function
/// so the refusal path is deterministically testable.
fn commit_with_conflict_gate(
    path: &Path,
    edited: &str,
    before: Fingerprint,
    key: &str,
) -> Result<(), SetError> {
    let now = file_fingerprint(path)?;
    if now != before {
        return Err(SetError::Conflict(format!(
            "{} changed on disk while it was read (editor or another pakmap?) — \
             re-check `pakmap config get {key}` and retry",
            path.display(),
            key = key
        )));
    }
    atomic_write(path, edited.as_bytes())
}

/// Build an exact `toml_edit` value per kind. Values were already validated;
/// parsing again guarantees the file never gets a value serde would reject.
fn build_edit_value(meta: &'static SettingMeta, value: &str) -> Result<toml_edit::Value, SetError> {
    let mut probe = crate::config::Config::default();
    meta.field
        .set(&mut probe, value)
        .map_err(|e| SetError::InvalidValue(format!("{}: {e}", meta.key)))?;
    let rendered = meta.field.get(&probe);
    match meta.kind {
        super::catalog::Kind::Bool => Ok(toml_edit::Value::from(rendered == "true")),
        super::catalog::Kind::U16
        | super::catalog::Kind::U32
        | super::catalog::Kind::U64
        | super::catalog::Kind::Usize => {
            // Catalog numerics are far below i64::MAX in practice; the
            // validated parse already bounds them.
            Ok(toml_edit::Value::from(rendered_str_u64(&rendered)? as i64))
        }
        super::catalog::Kind::Enum(_)
        | super::catalog::Kind::Style
        | super::catalog::Kind::Text
        | super::catalog::Kind::OptText => Ok(toml_edit::Value::from(rendered)),
        super::catalog::Kind::PairU16 => {
            let parts = rendered
                .split(',')
                .filter_map(|p| p.trim().parse::<i64>().ok())
                .collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(SetError::InvalidValue(format!(
                    "{}: expected (cols,rows)",
                    meta.key
                )));
            }
            let arr = toml_edit::Array::from_iter(parts);
            Ok(toml_edit::Value::Array(arr))
        }
    }
}

fn rendered_str_u64(s: &str) -> Result<u64, SetError> {
    s.parse::<u64>()
        .map_err(|_| SetError::InvalidValue("expected a non-negative number".into()))
}

/// Post-write scan of the edited document: current unknown keys / aliases in
/// the file surface in the `set` result (plan §2).
fn scan_warnings(text: &str, path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for found in super::scanner::scan(text).keys {
        if found.key == super::BOTTOM_UP_ALIAS {
            out.push(format!(
                "note: [behavior] bottom_up is a deprecated alias of [search] bottom_up ({path}:{line})",
                path = path.display(),
                line = found.line_no
            ));
        } else if SettingMeta::lookup(&found.key).is_none() {
            out.push(format!(
                "warn: unknown key '{}' in {path}:{line} — not a pakmap setting",
                found.key,
                path = path.display(),
                line = found.line_no
            ));
        }
    }
    out
}

/// Create the missing user file from the generated example template (0644;
/// parent dirs created). PLAN §1.6: bootstrapped "from example_toml() minus
/// nothing" — the full documented file.
fn bootstrap(path: &Path) -> Result<(), SetError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SetError::Io(format!("cannot create {}: {e}", parent.display())))?;
    }
    let example = crate::config::Config::example_toml();
    atomic_write(path, example.as_bytes())
}

/// tmp → fsync file → rename → fsync dir best-effort, perms 0644 (plan §1.6;
/// config files are user-readable, unlike safety's 0600 cache writes).
fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), SetError> {
    let Some(parent) = path.parent() else {
        return Err(SetError::Io(format!("{:?} has no parent dir", path)));
    };
    let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(SetError::Io(format!("{:?} is not a plain file name", path)));
    };
    let tmp = parent.join(format!("{stem}.tmp-{}", std::process::id()));
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&tmp)
        .map_err(|e| SetError::Io(format!("cannot open {}: {e}", tmp.display())))?;
    file.write_all(contents)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .and_then(|()| file.set_permissions(std::fs::Permissions::from_mode(0o644)))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            SetError::Io(format!("cannot write {}: {e}", tmp.display()))
        })?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => {
            // Best-effort dir sync (rename durability); failure is not fatal.
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(SetError::Io(format!(
                "cannot replace {}: {e}",
                path.display()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    //! §1.4 writer gates 7–10 + §1.6 unit behavior. Unwraps are re-allowed
    //! for the test crate via `lib.rs`'s cfg_attr.

    use super::*;

    /// §1.4.7: preset comments and unspecified ordering survive a set.
    #[test]
    fn comment_preserving_write() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(
            &p,
            concat!(
                "# header comment\n",
                "\n",
                "[search]\n",
                "# personal note\n",
                "limit = 5 # trailing stays\n",
                "\n",
                "[theme]\n",
                "text = \"white\" # keep me\n",
            ),
        )
        .unwrap();
        let out = set(
            "search.limit",
            "12",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        );
        let out = out.expect("set must succeed");
        assert!(!out.created);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# header comment"));
        assert!(text.contains("# personal note"));
        assert!(text.contains("limit = 12 # trailing stays"));
        assert!(text.contains("[theme]\ntext = \"white\" # keep me"));
        assert!(
            text.find("# header comment").unwrap() < text.find("[theme]").unwrap(),
            "existing ordering preserved"
        );
    }

    /// §1.6 unset: key removed; when its table ends up empty the table goes
    /// too (file stays tidy); multi-key tables keep their siblings.
    #[test]
    fn unset_removes_key_and_empties_the_table() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n").unwrap();
        set(
            "search.limit",
            "9",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        unset(
            "search.limit",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("[search]"), "empty table removed: {text}");

        let p2 = dir.path().join("two.toml");
        std::fs::write(&p2, "[search]\nlimit = 5\nbottom_up = true\n").unwrap();
        unset(
            "search.limit",
            &Target::User {
                path: p2.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p2).unwrap();
        assert!(text.contains("[search]"));
        assert!(text.contains("bottom_up = true"));
        assert!(!text.contains("limit ="));
    }

    /// §1.4.9 (+§1.6): atomic temp+rename commit, 0644 perms, no tmp leftover.
    #[test]
    fn write_is_atomic_0644_with_no_tmp_leftover() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n").unwrap();
        set(
            "search.limit",
            "6",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1, "no tmp file left behind");
        assert_eq!(entries[0].path(), p);
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o644);
    }

    /// §1.6: a missing user file is bootstrapped from the FULL example
    /// template (never trimmed), with the new value applied afterwards.
    #[test]
    fn bootstrap_creates_full_example_then_applies_value() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let out = set(
            "search.limit",
            "9",
            &Target::User {
                path: p.clone(),
                create: true,
            },
        )
        .expect("consented create must succeed");
        assert!(out.created);
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            text.lines().next(),
            Some("# pakmap config — edit and save, changes apply on next launch")
        );
        assert!(text.contains("limit = 9"));
        // The example's own inline documentation survives the value edit.
        assert!(text.contains("# max results per source; 0 = no limit"));
        std::fs::metadata(&p).unwrap();
    }

    /// §1.4.8: fingerprint mismatch → `Err(Conflict)`, on-disk text intact.
    #[test]
    fn write_conflict_refuses_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n").unwrap();
        let bogus = Fingerprint {
            mtime_secs: 1,
            len: 12345,
            head: [0u8; 32],
            tail: [0u8; 32],
        };
        let err = commit_with_conflict_gate(&p, "[search]\nlimit = 6\n", bogus, "search.limit")
            .expect_err("pinned mismatch must conflict");
        assert!(matches!(err, SetError::Conflict(_)));
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "[search]\nlimit = 5\n"
        );
        // A matching fingerprint proceeds to the atomic commit.
        let fresh = file_fingerprint(&p).unwrap();
        commit_with_conflict_gate(&p, "[search]\nlimit = 6\n", fresh, "search.limit")
            .expect("matching fingerprint must commit");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "[search]\nlimit = 6\n"
        );
    }

    /// §1.4.10 (D2): `[behavior]` into a project file refused untrusted;
    /// allowed with trust; the project file is never implicitly created.
    #[test]
    fn project_write_policy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pakmap.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n").unwrap();

        let untrusted = Target::Project {
            path: p.clone(),
            trusted: false,
        };
        let err = set("behavior.readonly", "true", &untrusted)
            .expect_err("behavior into untrusted project must refuse");
        assert!(matches!(err, SetError::PolicyRefused(_)));
        assert!(
            err.message().contains("PAKMAP_TRUST_PROJECT"),
            "refusal must name the opt-in: {}",
            err.message()
        );
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "[search]\nlimit = 5\n"
        );

        let trusted = Target::Project {
            path: p.clone(),
            trusted: true,
        };
        set("behavior.readonly", "true", &trusted).expect("trusted project write ok");
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("readonly = true")
        );

        // Never implicitly created (even unset on a missing target refused).
        let missing = dir.path().join("absent.toml");
        let t = Target::Project {
            path: missing.clone(),
            trusted: false,
        };
        assert!(set("search.limit", "5", &t).is_err());
        assert!(unset("search.limit", &t).is_err());
        assert!(!missing.exists());
    }

    /// Missing user file unset is an idempotent no-op success.
    #[test]
    fn unset_on_missing_user_file_is_noop_ok() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let out = unset(
            "search.limit",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        );
        let out = out.expect("no-op must succeed");
        assert!(!out.created);
        assert!(out.warnings.is_empty());
        assert!(!p.exists(), "never created by unset");
    }

    /// Strict door validation: unknown key / invalid value / the alias —
    /// each with its taxonomy entry and the file untouched.
    #[test]
    fn set_errors_leave_the_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n").unwrap();
        let t = Target::User {
            path: p.clone(),
            create: false,
        };

        assert!(matches!(
            set("search.limt", "5", &t),
            Err(SetError::UnknownKey(_))
        ));
        let err = set("tui.debounce_ms", "0", &t).expect_err("clamp must reject");
        assert!(matches!(err, SetError::InvalidValue(_)));
        assert!(err.message().contains("50..=5000"));
        let err = set("behavior.bottom_up", "true", &t).expect_err("alias is refused");
        assert!(matches!(err, SetError::UnknownKey(_)));
        assert!(
            err.message().contains("[search] bottom_up"),
            "alias hint: {}",
            err.message()
        );
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "[search]\nlimit = 5\n"
        );
    }

    /// Post-write warnings surface current unknown keys and the alias.
    #[test]
    fn set_reports_post_write_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(
            &p,
            "[search]\nlimit = 5\ntypo = 1\n[behavior]\nbottom_up = true\n",
        )
        .unwrap();
        let out = set(
            "search.limit",
            "7",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("unknown key 'search.typo'"))
        );
        assert!(out.warnings.iter().any(|w| w.contains("deprecated alias")));
    }
}
