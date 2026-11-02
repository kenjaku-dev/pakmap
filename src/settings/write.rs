//! Byte-surgical, comment-preserving atomic config writer (plan §1.6,
//! phase 1b and beyond).
//!
//! The writer edits the target file's RAW LINES — the same lexical
//! discipline as `Config::strip_behavior_section` (`src/config.rs`) and the
//! settings scanner (`scanner.rs`): locate the `[section]` header, then the
//! `key = value` line inside it, and touch exactly that line — nothing
//! else. Every other byte survives: comments, blank-line rhythm,
//! indentation, the spacing around `=`, inline `# comments`, the EOL style
//! (`\n` / `\r\n`) and the trailing-newline state. A missing section is
//! appended (`\n[section]\nkey = value`, no decorative comments invented).
//!
//! Refuse-on-unparseable rule: a target line whose value does not END
//! confidently on its own line (multi-line array/string, incompatible or
//! unterminated quotes, unrecognized token shape) is NEVER rewritten or
//! dropped — `Err(Unparseable)` with an "edit it by hand" hint. A section
//! defined more than once is refused for the same reason. This replaces the
//! former `toml_edit` round-trip: line surgery decides a statement's
//! extent per line, so a half-written file can never be silently
//! reformatted by a writer that believes it owns the document.
//!
//! Writes stay atomic (tmp → fsync → rename → fsync dir best-effort) behind
//! the `(mtime, len, head32, tail32)` fingerprint conflict gate, so an
//! editor or a concurrent pakmap can never be clobbered. No new crates, no
//! document model: the file is read once, spliced line-wise, and committed.

use std::path::{Path, PathBuf};

use super::PROJECT_ALLOWED_SECTIONS;
use super::catalog::{SettingMeta, section_of};
use super::scanner;

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

/// Remove `key`. Phase-1b tidy rule: the section header follows the key
/// onto the floor only when the region keeps NO content lines at all —
/// no keys and no comments (comments survive by definition; blank lines
/// are not content). Missing user file / missing key are idempotent no-ops.
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

    let edited = if unset_key.is_some() {
        edit_document(&text, meta, Edit::Unset, &path)?
    } else {
        edit_document(
            &text,
            meta,
            Edit::Set {
                value: render_value(meta, value)?,
            },
            &path,
        )?
    };

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

// ---------------------------------------------------------------------------
// Line surgery (phase 1b): lexical position, then a one-line splice
// ---------------------------------------------------------------------------

/// One edit of `meta.key`, already kind-validated.
#[derive(Debug)]
enum Edit {
    /// Replace the edited line's VALUE token with the exact literal.
    Set { value: String },
    /// Remove the edited line (and the bare section header afterwards).
    Unset,
}

/// Where the target key can live in the document.
#[derive(Debug)]
enum Site {
    /// Under the `[section]` header: header row + region extent (the region
    /// runs to the next header row of any path, or EOF).
    Header {
        hdr: usize,
        start: usize,
        end: usize,
    },
    /// Pre-header root region with an explicit dotted key
    /// (`search.limit = 4`): the faithful edit site when no `[section]`
    /// header exists (the scanner credits that line a full origin, too).
    Root { start: usize, end: usize },
    /// Neither: the section is appended at EOF per plan §1.6.
    Append,
}

/// What a region scan learned about the target key.
#[derive(Debug, Default)]
struct Walked {
    /// target key row index (None = the key is not present in the region)
    target: Option<usize>,
    /// the target's value opens a multi-line statement (refuse per §1.6)
    opens: bool,
    /// last confidently single-line key row — the insertion anchor
    anchor: Option<usize>,
}

/// Wire one edit through the document. Byte-precise, EOL-preserving, and
/// refusing rather than guessing (see the module contract).
fn edit_document(
    text: &str,
    meta: &'static SettingMeta,
    edit: Edit,
    path: &Path,
) -> Result<String, SetError> {
    let section = section_of(meta.key);
    let file_key = meta.key.split('.').nth(1).unwrap_or(meta.key);
    let root_dotted = meta.key; // `search.limit` — the full dotted spelling
    let eol = dominant_eol(text);
    let lines = split_lines_keep_eol(text);

    // ---- pass 1: document layout, in the scanner's fail-open discipline ----
    // Header rows (any path) with row indices; the pre-header root dotted
    // key; the continuation gate so value lines are never misread as keys.
    let mut headers: Vec<(usize, Vec<String>)> = Vec::new();
    let mut root_lines: Vec<usize> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let (mut open, mut ml): (usize, Option<String>) = (0, None);
    let mut carrying = false;
    for (idx, (content, _)) in lines.iter().enumerate() {
        if carrying {
            (open, ml) = scanner::balance(content, open, &mut ml);
            carrying = open > 0 || ml.is_some();
            continue;
        }
        let t = content.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(p) = scanner::parse_header(t) {
            headers.push((idx, p.clone()));
            stack = p;
            continue;
        }
        if let Some((key, rest, _)) = scanner::split_key_value(t) {
            if stack.is_empty() && scanner::dotted(&stack, &key) == root_dotted {
                root_lines.push(idx);
            }
            (open, ml) = scanner::balance(rest, 0, &mut ml);
            carrying = open > 0 || ml.is_some();
        }
        // Any other row is prose or a half-broken statement: fail open with
        // the scanner discipline — the writer declines to reason about it.
    }

    // ---- region resolution ----
    let same_section = |p: &[String]| p.len() == 1 && p[0] == section;
    let hdrs: Vec<usize> = headers
        .iter()
        .filter(|(_, p)| same_section(p))
        .map(|(idx, _)| *idx)
        .collect();
    let site: Site = if hdrs.len() > 1 {
        return Err(SetError::Unparseable(format!(
            "[{section}] appears in {} more than once — pakmap will not guess which \
             definition to edit; keep one and edit it by hand",
            path.display()
        )));
    } else if let Some(hdr) = hdrs.first() {
        let start = hdr + 1;
        let end = headers
            .iter()
            .find(|(idx, _)| *idx > *hdr)
            .map(|(idx, _)| *idx)
            .unwrap_or(lines.len());
        Site::Header {
            hdr: *hdr,
            start,
            end,
        }
    } else {
        match root_lines.as_slice() {
            [_only] => {
                let end = headers.first().map(|(idx, _)| *idx).unwrap_or(lines.len());
                Site::Root { start: 0, end }
            }
            [] => Site::Append,
            _ => {
                return Err(SetError::Unparseable(format!(
                    "'{root_dotted}' appears in {} more than once before any \
                     [section] header — dedupe it and edit by hand",
                    path.display()
                )));
            }
        }
    };

    // ---- pass 2: region walk for the exact key row + insertion anchor ----
    let (start, end): (usize, usize) = match site {
        Site::Header { start, end, .. } => (start, end),
        Site::Root { start, end } => (start, end),
        Site::Append => (lines.len(), lines.len()),
    };
    let in_region_spelling = match site {
        Site::Header { .. } => file_key, // plain key under `[section]`
        Site::Root { .. } => root_dotted,
        Site::Append => "",
    };
    let walked = walk_region(&lines, start, end, in_region_spelling);

    match edit {
        Edit::Set { ref value } => match walked.target {
            Some(idx) => {
                if walked.opens {
                    return Err(multiline_refusal(meta, path, idx));
                }
                let Some(patched) = replace_value_token(lines[idx].0, value) else {
                    return Err(unsafe_literal_refusal(meta, path, idx));
                };
                Ok(rebuild(&lines, Some((idx, &patched)), &[]))
            }
            None => match site {
                Site::Header { hdr, .. } => {
                    // Insert after the previous key line (or right after the
                    // header), keeping the section's key-spacing style.
                    let (line, at) = match walked.anchor {
                        Some(anchor) => (styled_line(lines[anchor].0, file_key, value), anchor + 1),
                        None => (format!("{file_key} = {value}"), hdr + 1),
                    };
                    Ok(splice_insert(&lines, at, &line, eol))
                }
                Site::Root { .. } => {
                    // Pre-header dotted key: after the last confident key
                    // line in the root region, else before the first header
                    // (or at the very top of a headerless file).
                    let first_header = headers.first().map(|(idx, _)| *idx);
                    let (line, at) = match (walked.anchor, first_header) {
                        (Some(anchor), _) => {
                            (styled_line(lines[anchor].0, root_dotted, value), anchor + 1)
                        }
                        (None, Some(hdr)) => (format!("{root_dotted} = {value}"), hdr),
                        (None, None) => (format!("{root_dotted} = {value}"), lines.len()),
                    };
                    Ok(splice_insert(&lines, at, &line, eol))
                }
                Site::Append => Ok(append_section(text, section, file_key, value, eol)),
            },
        },
        Edit::Unset => match walked.target {
            None => Ok(text.to_string()), // idempotent: file bytes untouched
            Some(idx) => {
                if walked.opens {
                    return Err(multiline_refusal(meta, path, idx));
                }
                let mut drop = vec![idx];
                if let Site::Header { hdr, start, end } = site {
                    // Phase-1b tidy rule (see `unset`'s doc): drop the header
                    // only when the region keeps no content lines at all —
                    // no keys AND no comments (comments survive by
                    // definition; blank lines are not content). This is
                    // intentionally stricter than the old toml_edit
                    // empty-table rule, which also removed comment-bearing
                    // tables.
                    let any_content = (start..end)
                        .filter(|row| *row != idx)
                        .any(|row| !lines[row].0.trim_start().is_empty());
                    if !any_content {
                        drop.push(hdr);
                    }
                }
                Ok(rebuild(&lines, None, &drop))
            }
        },
    }
}

fn multiline_refusal(meta: &SettingMeta, path: &Path, row: usize) -> SetError {
    SetError::Unparseable(format!(
        "'{}' at {}:{} — its value spans multiple lines; pakmap rewrites single-line \
         values only — edit it by hand",
        meta.key,
        path.display(),
        row + 1
    ))
}

fn unsafe_literal_refusal(meta: &SettingMeta, path: &Path, row: usize) -> SetError {
    SetError::Unparseable(format!(
        "'{}' at {}:{} — value not safely rewritable (unterminated quotes or \
         unsupported token) — edit it by hand",
        meta.key,
        path.display(),
        row + 1
    ))
}

/// Walk ONE region for the exact target key + the insertion anchor, in the
/// scanner's continuation discipline (an unbalanced value swallows the rows
/// after it; nothing inside can be a key). `target` is the key's canonical
/// spelling within the region: `limit` under `[search]`,
/// `search.limit` in the pre-header root region.
fn walk_region(lines: &[(&str, &str)], start: usize, end: usize, target: &str) -> Walked {
    let mut out = Walked::default();
    let (mut open, mut ml): (usize, Option<String>) = (0, None);
    let mut carrying = false;
    for (idx, line) in lines.iter().enumerate().skip(start).take(end - start) {
        let (content, _) = *line;
        if carrying {
            (open, ml) = scanner::balance(content, open, &mut ml);
            carrying = open > 0 || ml.is_some();
            if !carrying {
                out.anchor = Some(idx); // the row that closes the value
            }
            continue;
        }
        let t = content.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if scanner::parse_header(t).is_some() {
            break; // defensive: regions end before headers by construction
        }
        if let Some((key, rest, _)) = scanner::split_key_value(t) {
            let canon = scanner::dotted(&[], &key);
            let matched = canon == target;
            (open, ml) = scanner::balance(rest, 0, &mut ml);
            carrying = open > 0 || ml.is_some();
            if carrying {
                if matched {
                    out.target = Some(idx);
                    out.opens = true;
                }
            } else {
                if matched {
                    out.target = Some(idx);
                }
                out.anchor = Some(idx);
            }
        }
        // Other rows: fail open (prose stays prose; nothing anchors on it).
    }
    out
}

/// `key = old` → `key = new`, byte-precisely: only the value token changes;
/// whatever precedes the `=` and follows the token (leading indent, key,
/// spacing, inline `# comment`) survives verbatim. Returns None when the
/// old value cannot be confidently read as a single-line token (multi-line
/// array/string, unterminated quotes, unsupported shape) — the caller then
/// refuses the write per the plan §1.6 refuse rule.
fn replace_value_token(content: &str, new_value: &str) -> Option<String> {
    let lead = content.len() - content.trim_start().len();
    let t = &content[lead..];
    let (_key, _rest, eq) = scanner::split_key_value(t)?;
    let after_eq = &t[eq + 1..];
    let ws_len = after_eq.len() - after_eq.trim_start_matches([' ', '\t']).len();
    let v_start = lead + eq + 1 + ws_len;
    let tok_len = confident_value_end(&after_eq[ws_len..])?;
    let v_end = v_start + tok_len;
    let mut out = String::with_capacity(content.len() + new_value.len());
    out.push_str(&content[..v_start]);
    out.push_str(new_value);
    out.push_str(&content[v_end..]);
    Some(out)
}

/// Byte length of a single-line value token starting at `s`, given ONLY
/// spaces/tabs and then an optional `# comment` (or the end of line) after
/// it. Shapes: booleans, unsigned integers, quoted strings (escape-aware
/// `"…"` and literal `'…'`), balanced single-line arrays and inline tables.
/// Anything else — a multi-line opener, an unterminated quote, an unknown
/// token — is None, which makes the caller refuse the whole line.
fn confident_value_end(s: &str) -> Option<usize> {
    let end = match s.as_bytes().first()? {
        b't' | b'f' => scan_bool(s)?,
        b'0'..=b'9' => scan_number(s)?,
        b'"' | b'\'' => scan_quoted(s)?,
        b'[' | b'{' => scan_balanced(s)?,
        _ => return None,
    };
    let tail = s.get(end..)?.trim_start_matches([' ', '\t']);
    if tail.is_empty() || tail.starts_with('#') {
        Some(end)
    } else {
        None
    }
}

/// `true`/`false` with a word boundary: only whitespace or end of line may
/// follow (never another TOML byte — that would be an unknown token).
fn scan_bool(s: &str) -> Option<usize> {
    let end = if s.starts_with("true") {
        4
    } else if s.starts_with("false") {
        5
    } else {
        return None;
    };
    match s.as_bytes().get(end) {
        None | Some(b' ') | Some(b'\t') => Some(end),
        _ => None,
    }
}

/// Unsigned integer run (`0-9`, `_` allowed); only whitespace or end of
/// line after. Signed floats, dates, `inf`/`nan` — None (unsigned only).
fn scan_number(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    match bytes.get(i) {
        None | Some(b' ') | Some(b'\t') => Some(i),
        _ => None,
    }
}

/// Quoted string with escape-aware closing quote (`"…"` skips `\"`; `'…'`
/// has no escapes). Triple-quote openers (`"""`/`'''`) and quotes not
/// terminated on the line are None (never rewritten).
fn scan_quoted(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let q = bytes[0];
    if bytes.len() >= 3 && bytes[1] == q && bytes[2] == q {
        return None; // multi-line string opener — refuse per contract
    }
    let mut i = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if q == b'"' => i += 2,
            c if c == q => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// Balanced single-line array `[…]` / inline table `{…}` — quote-aware, so
/// brackets inside strings never count; a `#` outside strings means the
/// value spills to the next line (refused).
fn scan_balanced(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b'"' | b'\'' => {
                i += scan_quoted(&s[i..])?;
                continue;
            }
            b'#' => return None, // comment: the value continues on the next line
            _ => {}
        }
        i += 1;
    }
    None // still open at the end of the line
}

/// Rebuild the document from line rows: swap one row's content and/or drop
/// rows (a row's content AND its EOL go together). Every other row stays a
/// byte-for-byte copy — no reformat, ever.
fn rebuild(lines: &[(&str, &str)], swap: Option<(usize, &str)>, drop: &[usize]) -> String {
    let mut out = String::new();
    for (i, (content, eol)) in lines.iter().enumerate() {
        if drop.contains(&i) {
            continue;
        }
        match swap {
            Some((j, new_content)) if j == i => {
                out.push_str(new_content);
                out.push_str(eol);
            }
            _ => {
                out.push_str(content);
                out.push_str(eol);
            }
        }
    }
    out
}

/// Insert `line` (content only; the EOL is added here) so it lands just
/// ABOVE row `at` (row `lines.len()` = at the very end of the document).
/// Untouched rows keep every byte; a dangling terminator-less final row is
/// terminated first, and the inserted row always takes the file's dominant
/// EOL so a single style is never mixed by the writer.
fn splice_insert(lines: &[(&str, &str)], at: usize, line: &str, eol: &str) -> String {
    let mut out = String::new();
    for (i, (content, row_eol)) in lines.iter().enumerate() {
        if i == at {
            out.push_str(line);
            out.push_str(eol);
        }
        out.push_str(content);
        out.push_str(row_eol);
    }
    if at >= lines.len() {
        if lines.last().is_some_and(|(_, row_eol)| row_eol.is_empty()) {
            out.push_str(eol); // terminate the dangling final row first
        }
        out.push_str(line);
        out.push_str(eol);
    }
    out
}

/// Mirror one existing key row's shape onto a NEW key: leading indent, the
/// gap between key and `=`, and the whitespace after `=` — the section's
/// local style, preserved. Falls back to `key = value` when the anchor row
/// is not trivially reducible to that shape.
fn styled_line(anchor: &str, key: &str, value: &str) -> String {
    let indent_len = anchor.len() - anchor.trim_start().len();
    let t = &anchor[indent_len..];
    let Some((kt, _rest, eq)) = scanner::split_key_value(t) else {
        return format!("{key} = {value}");
    };
    if t[..eq].trim() != kt {
        return format!("{key} = {value}"); // odd quoting: keep the plain shape
    }
    let seq = t[..eq].trim_start();
    if !seq.starts_with(kt.as_str()) {
        return format!("{key} = {value}");
    }
    let gap = &seq[kt.len()..]; // whitespace between key and `=`
    let after_eq = &t[eq + 1..];
    let vgap_len = after_eq.len() - after_eq.trim_start_matches([' ', '\t']).len();
    let indent = &anchor[..indent_len];
    let vgap = &after_eq[..vgap_len];
    format!("{indent}{key}{gap}={vgap}{value}")
}

/// `\n[section]\nkey = value` — appended at EOF exactly as plan §1.6
/// writes it (one blank separator before the header, no decorative
/// comments). Dominant EOL; the trailing-newline state mirrors the file.
fn append_section(text: &str, section: &str, file_key: &str, value: &str, eol: &str) -> String {
    let had_final_nl = text.is_empty() || text.ends_with('\n');
    let mut out = String::with_capacity(text.len() + section.len() + value.len() + 8);
    out.push_str(text);
    if !text.is_empty() && !text.ends_with('\n') {
        out.push_str(eol); // terminate the dangling last row
    }
    if !text.is_empty() {
        out.push_str(eol); // blank separator before the appended header
    }
    out.push('[');
    out.push_str(section);
    out.push(']');
    out.push_str(eol);
    out.push_str(file_key);
    out.push_str(" = ");
    out.push_str(value);
    if had_final_nl {
        out.push_str(eol);
    }
    out
}

/// File-wide dominant EOL (majority vote: `\r\n` rows vs lone `\n` rows).
fn dominant_eol(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count().saturating_sub(crlf);
    if crlf > lf { "\r\n" } else { "\n" }
}

/// Split `text` into `(content-without-EOL, EOL)` rows: the EOL intact
/// (`"\n"` or `"\r\n"`), `""` on a final row that lacks a terminator so the
/// file's EOF-newline state is carried into the rebuild untouched.
fn split_lines_keep_eol(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let content_end = if i > start && bytes[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            out.push((&text[start..content_end], &text[content_end..=i]));
            start = i + 1;
        }
        i += 1;
    }
    if start < bytes.len() {
        out.push((&text[start..], ""));
    }
    out
}

/// Exact TOML literal of a kind-validated value (plan §1.6): booleans and
/// numbers bare, strings as escaped basic strings, `(cols,rows)` as a
/// single-line inline array. Values were already validated by the `set`
/// door; this re-parses through the catalog's own setters so the file never
/// gets a value serde would reject.
fn render_value(meta: &'static SettingMeta, value: &str) -> Result<String, SetError> {
    let mut probe = crate::config::Config::default();
    meta.field
        .set(&mut probe, value)
        .map_err(|e| SetError::InvalidValue(format!("{}: {e}", meta.key)))?;
    let rendered = meta.field.get(&probe);
    Ok(match meta.kind {
        super::catalog::Kind::Bool => rendered,
        super::catalog::Kind::U16
        | super::catalog::Kind::U32
        | super::catalog::Kind::U64
        | super::catalog::Kind::Usize => {
            // Catalog numerics are far below i64::MAX in practice; the
            // validated parse already bounds them.
            rendered_str_u64(&rendered)?.to_string()
        }
        super::catalog::Kind::Enum(_)
        | super::catalog::Kind::Style
        | super::catalog::Kind::Text
        | super::catalog::Kind::OptText => toml_basic_string(&rendered),
        super::catalog::Kind::PairU16 => {
            let pair_err = || SetError::InvalidValue(format!("{}: expected (cols,rows)", meta.key));
            let mut parts = rendered.split(',').map(str::trim);
            let Some(a) = parts.next().and_then(|p| p.parse::<u16>().ok()) else {
                return Err(pair_err());
            };
            let Some(b) = parts.next().and_then(|p| p.parse::<u16>().ok()) else {
                return Err(pair_err());
            };
            format!("[{a}, {b}]")
        }
    })
}

fn rendered_str_u64(s: &str) -> Result<u64, SetError> {
    s.parse::<u64>()
        .map_err(|_| SetError::InvalidValue("expected a non-negative number".into()))
}

/// The writer's string literal: a TOML basic string with byte-exact
/// escapes — `\"`, `\\`, `\b/\t/\n/\f/\r`, other C0 controls `\uXXXX`.
fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
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

    /// §1.6 unset TIDY RULE, re-pinned for phase 1b's line surgeon (this
    /// replaces the older test that documented toml_edit's empty-table
    /// removal): the key line goes out, and the section header follows it
    /// ONLY when the region is left with NO content at all — no keys, NO
    /// comments (comments survive by definition; blank lines don't keep a
    /// section alive); multi-key sections keep their siblings.
    #[test]
    fn unset_removes_key_and_drops_the_header_only_when_bare() {
        let dir = tempfile::tempdir().unwrap();

        // Bare section: the header follows the key out.
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
        assert!(!text.contains("[search]"), "bare header dropped: {text}");

        // Siblings keep the section alive.
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

        // A comment is content: the header survives with it (comments are
        // never rewritten or dropped by the line surgeon).
        let p3 = dir.path().join("note.toml");
        std::fs::write(&p3, "[search]\n# personal note\nlimit = 5\n").unwrap();
        unset(
            "search.limit",
            &Target::User {
                path: p3.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p3).unwrap();
        assert!(
            text.contains("[search]"),
            "comment keeps the header: {text}"
        );
        assert!(text.contains("# personal note"));
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

    // ---- phase-1b line-surgery unit pins ----------------------------------

    /// A key ADDED into an existing section lands immediately after the
    /// previous key line — never before the section, never inside a
    /// neighbor's multi-line value — with all other bytes identical.
    #[test]
    fn set_inserts_after_the_last_key_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(
            &p,
            "[search]\n# intro\nlimit = 5 # keep\n\n[theme]\ntext = \"white\"\n",
        )
        .unwrap();
        set(
            "search.regex",
            "true",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            text,
            "[search]\n# intro\nlimit = 5 # keep\nregex = true\n\n[theme]\ntext = \"white\"\n"
        );
    }

    /// Absent section: appended exactly per plan §1.6, `\n[section]\nkey`.
    #[test]
    fn set_appends_absent_section() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n").unwrap();
        set(
            "gui.theme",
            "dark",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text, "[search]\nlimit = 5\n\n[gui]\ntheme = \"dark\"\n");
    }

    /// Unparseable value lines are refused untouched — multi-line arrays,
    /// dangling quotes, unsupported token shapes; bytes identical after.
    #[test]
    fn refuse_unparseable_target_lines_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        for (src, key, new_value) in [
            ("[search]\nlimit = [\n  5,\n]\n", "search.limit", "6"),
            ("[search]\nlimit = \"unterminated\n", "search.limit", "6"),
            (
                "[behavior]\ndiff_pager = 1979-05-27 # dated\n",
                "behavior.diff_pager",
                "auto",
            ),
        ] {
            let p = dir.path().join("case.toml");
            std::fs::write(&p, src).unwrap();
            let err = set(
                key,
                new_value,
                &Target::User {
                    path: p.clone(),
                    create: false,
                },
            )
            .expect_err("unparseable line must refuse");
            assert!(matches!(err, SetError::Unparseable(_)), "{err}");
            assert!(err.message().contains("edit it by hand"), "{err}");
            assert_eq!(
                std::fs::read_to_string(&p).unwrap(),
                src,
                "refused write must not touch the file"
            );
            std::fs::remove_file(&p).unwrap();
        }
    }

    /// CRLF files stay CRLF (the edited row keeps its own EOL; inserted and
    /// appended rows take the dominant style), inline comments survive.
    #[test]
    fn crlf_file_preserves_eol_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\r\nlimit = 5 # keep me\r\n").unwrap();
        set(
            "search.limit",
            "6",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = String::from_utf8(std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(text, "[search]\r\nlimit = 6 # keep me\r\n");

        // In-section insert keeps the dominant CRLF style.
        set(
            "search.bottom_up",
            "true",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            text,
            "[search]\r\nlimit = 6 # keep me\r\nbottom_up = true\r\n"
        );
    }

    /// Value-token replacement keeps spacing verbatim: `limit=5` stays
    /// glued to its equal sign, whatever the style.
    #[test]
    fn tight_spacing_style_is_reproduced_inline() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit=5\n").unwrap();
        set(
            "search.limit",
            "9",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("limit=9"), "{text}");
    }

    /// Quoted section headers (`["search"]`) resolve like the scanner.
    #[test]
    fn quoted_section_header_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[\"search\"]\nlimit = 5\n").unwrap();
        set(
            "search.limit",
            "8",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("limit = 8"), "{text}");
    }

    /// A duplicated `[section]` header makes the whole write refuse — the
    /// surgeon will not guess which definition was meant.
    #[test]
    fn duplicate_section_headers_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[search]\nlimit = 5\n[search]\nlimit = 6\n").unwrap();
        let err = set(
            "search.limit",
            "9",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .expect_err("duplicate section must refuse");
        assert!(matches!(err, SetError::Unparseable(_)), "{err}");
        assert!(err.message().contains("edit it by hand"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "[search]\nlimit = 5\n[search]\nlimit = 6\n"
        );
    }

    /// Pair kinds render the inline array `[70, 60]` and replace inline.
    #[test]
    fn pair_values_render_as_inline_array() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[tui]\npopup_info = [ 10, 20 ]\n").unwrap();
        set(
            "tui.popup_info",
            "70,60",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("popup_info = [70, 60]"), "{text}");
    }

    /// Unset on a missing key inside an existing file is a byte no-op.
    #[test]
    fn unset_missing_key_leaves_bytes_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let src = "[search]\nlimit = 5 # tail\n";
        std::fs::write(&p, src).unwrap();
        unset(
            "search.regex",
            &Target::User {
                path: p.clone(),
                create: false,
            },
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), src);
    }
}
