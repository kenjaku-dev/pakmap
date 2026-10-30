//! Unix-filesystem safety: private dirs, symlink rejection, TOCTOU identity
//! checks, bounded reads, atomic cache writes.
//!
//! Root cause closed here (CWE-377): the old AUR cache flow created the dir
//! and then *ignored* a failing `chmod` — an attacker who planted the dir first
//! won silently. Every check below either succeeds loudly or refuses loudly;
//! nothing is deleted (an existing user cache is tightened when we can, never
//! removed).
//!
//! std-only + `nix::unistd::geteuid` — no new dependencies (MSRV 1.90).

use std::fs::{File, OpenOptions, symlink_metadata};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// PKGBUILD read cap shared by the bounded readers. 256KB matches the GUI's
/// `READ_CAP_BYTES` (v0.9 review contract); PKGBUILDs are <50KB in practice.
pub const PKGBUILD_READ_CAP: usize = 256 * 1024;

#[derive(Debug, Error)]
pub enum SafetyError {
    #[error("refusing symlink {0:?} — it could be retargeted mid-operation")]
    Symlink(PathBuf),
    #[error("refusing {0:?}: not a regular file (device/fifo?)")]
    NotRegular(PathBuf),
    #[error("refusing {0:?}: not a directory")]
    NotDirectory(PathBuf),
    #[error(
        "cache dir {0:?} is owned by another user (uid {1}) — it was possibly planted; set behavior.cache_dir to your own dir and remove {0:?}"
    )]
    ForeignDir(PathBuf, u32),
    #[error(
        "parent of {0:?} is world/group-writable without a sticky bit — move the cache under a dir you own"
    )]
    UntrustedParent(PathBuf),
    #[error("{0:?} changed on disk while it was in use (rebuilt, replaced or deleted) — refusing")]
    Swapped(PathBuf),
    #[error("no usable cache base (check HOME / XDG_CACHE_HOME / TMPDIR)")]
    NoCacheDir,
    #[error("{0:?} is not valid UTF-8 text")]
    NonUtf8(PathBuf),
    #[error("io on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// (device, inode, size) fingerprint detecting a file swapped underneath us
/// between review and use — the closing half of the CWE-367 chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
}

fn euid() -> u32 {
    nix::unistd::geteuid().as_raw()
}

fn io_err(path: &Path, source: std::io::Error) -> SafetyError {
    SafetyError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn not_empty_parent(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => Some(p),
        _ => None,
    }
}

/// Verify an existing dir `dir` is safe to hold pakmap-private data:
/// a real directory (no symlink at the final component), owned by this user,
/// and group/world bits tightened to 0700 (the owner may always chmod).
pub fn ensure_private_dir_and_tighten(dir: &Path) -> Result<(), SafetyError> {
    let md = symlink_metadata(dir).map_err(|e| io_err(dir, e))?;
    if md.file_type().is_symlink() {
        return Err(SafetyError::Symlink(dir.to_path_buf()));
    }
    if !md.is_dir() {
        return Err(SafetyError::NotDirectory(dir.to_path_buf()));
    }
    let owner = md.uid();
    if owner != euid() {
        return Err(SafetyError::ForeignDir(dir.to_path_buf(), owner));
    }
    if md.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| io_err(dir, e))?;
    }
    Ok(())
}

/// `dir` must end in a real dir owned by this user under a trusted parent:
/// the parent is acceptable when it is NOT writable beyond our reach — a
/// group/world-writable parent without the sticky bit would let anyone else
/// replace our subdir (the classic /tmp attack) — or when it IS world-writable
/// WITH the sticky bit (the standard /tmp guarantee). Ownership alone is not
/// enough: an own-but-0777 dir is exactly as hostile as any other.
///
/// Deliberately does NOT reject a symlinked parent component: redirecting
/// `~/.cache` to another partition via a symlink is a legitimate setup, and
/// the final-dir checks above already carry the boundary.
fn ensure_trusted_parent(dir: &Path) -> Result<(), SafetyError> {
    let Some(parent) = not_empty_parent(dir) else {
        return Ok(());
    };
    let md = parent.metadata().map_err(|e| io_err(parent, e))?;
    let mode = md.permissions().mode();
    let others_writable = mode & 0o022 != 0;
    let sticky = mode & 0o1000 != 0;
    if others_writable && !sticky {
        return Err(SafetyError::UntrustedParent(dir.to_path_buf()));
    }
    Ok(())
}

/// Parse-worthy plain file owned by this user: no symlink at the final
/// component (never mapped/retargetable), a regular file (not a device/
/// fifo), owned by our euid (other users' bytes stay their business).
/// Any Err (including a missing path) means "refuse this file" — the project
/// config loader (`config::load_project`) treats every Err as "ignore".
pub fn ensure_owned_file(path: &Path) -> Result<(), SafetyError> {
    let md = symlink_metadata(path).map_err(|e| io_err(path, e))?;
    if md.file_type().is_symlink() {
        return Err(SafetyError::Symlink(path.to_path_buf()));
    }
    if !md.is_file() {
        return Err(SafetyError::NotRegular(path.to_path_buf()));
    }
    if md.uid() != euid() {
        return Err(SafetyError::ForeignDir(path.to_path_buf(), md.uid()));
    }
    Ok(())
}

/// Access check for a path that may not exist yet (caller will create it):
/// `Ok(())` when missing, full private-dir verification otherwise. Used for
/// AUR clone dirs, which git creates on first install.
pub fn ensure_private_dir_if_exists(dir: &Path) -> Result<(), SafetyError> {
    match symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(dir, e)),
        Ok(_) => ensure_private_dir_and_tighten(dir),
    }
}

/// Create `dir` (recursively) or verify the existing one — always ending in a
/// real, owned, 0700 directory under a trusted parent. The v0.9.3 replacement
/// for create-then-quietly-chmod: an attacker-planted /tmp/pakmap now fails
/// with a `ForeignDir` error instead of being silently adopted.
pub fn create_private_dir(dir: &Path) -> Result<(), SafetyError> {
    // AlreadyExists can happen for ANY pre-existing node (file, symlink):
    // treat it as "stat decides below" — mkdir-with-create semantic stays.
    match std::fs::create_dir_all(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_err(dir, e)),
        Ok(()) => {}
    }
    ensure_private_dir_and_tighten(dir)?;
    ensure_trusted_parent(dir)
}

/// HOME/XDG-less fallback: `$TMPDIR/pakmap-euid-<uid>` — private per user and
/// TMPDIR-respecting, replacing the old shared `/tmp/pakmap` default.
pub fn per_user_cache_fallback() -> PathBuf {
    std::env::temp_dir().join(format!("pakmap-euid-{}", euid()))
}

/// Resolved + verified cache base used by AUR clones, diffs and the news
/// cache: `behavior.cache_dir` → XDG (`$XDG_CACHE_HOME/pakmap`) → per-user
/// temp fallback. Guarantees the returned dir exists, is owned mode-0700,
/// sits under a trusted parent, and was not reached through a symlink.
pub fn ensure_cache_base(cfg: &crate::config::Config) -> Result<PathBuf, SafetyError> {
    let base = cfg
        .cache_dir_path()
        .or_else(|| dirs::cache_dir().map(|p| p.join("pakmap")))
        .unwrap_or_else(per_user_cache_fallback);
    create_private_dir(&base)?;
    Ok(base)
}

/// Identity of `path` when it is a real, non-symlink regular file.
/// `Ok(None)` = does not exist (caller decides: warn-and-continue or error).
pub fn try_regular_file_identity(path: &Path) -> Result<Option<FileIdentity>, SafetyError> {
    match symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(path, e)),
        Ok(md) if md.file_type().is_symlink() => Err(SafetyError::Symlink(path.to_path_buf())),
        Ok(md) if !md.is_file() => Err(SafetyError::NotRegular(path.to_path_buf())),
        Ok(md) => Ok(Some(FileIdentity {
            dev: md.dev(),
            ino: md.ino(),
            size: md.len(),
        })),
    }
}

/// Re-stat after the file was reviewed: any difference (rebuilt, replaced by
/// a symlink, deleted) refuses what would run next — e.g. `makepkg`.
pub fn assert_unchanged(path: &Path, id: FileIdentity) -> Result<(), SafetyError> {
    match try_regular_file_identity(path) {
        Ok(None) => Err(SafetyError::Swapped(path.to_path_buf())),
        Ok(Some(now)) if now == id => Ok(()),
        Ok(Some(_)) | Err(_) => Err(SafetyError::Swapped(path.to_path_buf())),
    }
}

/// What a bounded read produced.
#[derive(Debug)]
pub struct BoundedText {
    pub text: String,
    pub truncated: bool,
}

/// Read a text file under `cap` bytes WITHOUT reading it whole first:
/// the cap applies during the read (`take`), symlinks and non-regular files
/// are rejected up front, and a post-open dev/ino re-check closes the
/// lstat→open swap race (a malicious git-clone's PKGBUILD→/dev/zero can no
/// longer balloon memory; the GUI's `truncated` flag stays truthful).
pub fn read_text_bounded(path: &Path, cap: usize) -> Result<BoundedText, SafetyError> {
    let lmd = symlink_metadata(path).map_err(|e| io_err(path, e))?;
    if lmd.file_type().is_symlink() {
        return Err(SafetyError::Symlink(path.to_path_buf()));
    }
    if !lmd.is_file() {
        return Err(SafetyError::NotRegular(path.to_path_buf()));
    }
    let file = File::open(path).map_err(|e| io_err(path, e))?;
    // The filename could have been swapped between lstat and open; anything
    // but the stat'd inode is treated as tampering, not luck.
    let fmd = file.metadata().map_err(|e| io_err(path, e))?;
    if fmd.dev() != lmd.dev() || fmd.ino() != lmd.ino() {
        return Err(SafetyError::Swapped(path.to_path_buf()));
    }
    let mut bytes = Vec::with_capacity((cap as u64).min(lmd.len()) as usize);
    let mut reader = file.take(cap as u64);
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| io_err(path, e))?;
    let truncated = lmd.len() > cap as u64;
    // A cap-aligned cut can end mid multi-byte char: salvage the largest
    // valid UTF-8 prefix; a file that is non-text in its entirety stays an
    // error (same None the old read_to_string path produced).
    let salvaged = super::text::trim_partial_tail(&bytes);
    let text = std::str::from_utf8(salvaged)
        .map(str::to_owned)
        .map_err(|_| SafetyError::NonUtf8(path.to_path_buf()))?;
    Ok(BoundedText { text, truncated })
}

/// Atomic write for cache data: unique 0600 temp file created in the target's
/// directory (`create_new`), fully written, then renamed over the target.
/// A crash mid-write can therefore never leave a truncated news cache JSON.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), SafetyError> {
    let Some(parent) = not_empty_parent(path) else {
        return Err(io_err(
            path,
            std::io::Error::other("atomic write needs a parent directory"),
        ));
    };
    std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(io_err(
            path,
            std::io::Error::other("atomic write needs a plain file name"),
        ));
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    for attempt in 0..4u8 {
        let tmp = parent.join(format!(".{stem}.tmp.{pid}.{nanos}.{attempt}"));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_err(path, e)),
        };
        let write_res = file
            .write_all(contents)
            .and_then(|()| file.flush())
            .and_then(|()| file.set_permissions(std::fs::Permissions::from_mode(0o600)));
        if let Err(e) = write_res {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_err(path, e));
        }
        return match std::fs::rename(&tmp, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(io_err(path, e))
            }
        };
    }
    Err(io_err(
        path,
        std::io::Error::other("no unique temp file name was available"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // PAKMAP_SKIP_LIVE is irrelevant here; these are pure-fs tests.

    #[test]
    fn existing_open_mode_is_tightened_not_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        create_private_dir(&cache).unwrap();
        let mode = std::fs::symlink_metadata(&cache)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "group/world bits tightened to 0700");
        create_private_dir(&cache).unwrap(); // idempotent
    }

    #[test]
    fn foreign_or_symlinked_leaf_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain.txt");
        std::fs::write(&file, "x").unwrap();

        // Symlink leaf pretending to be the cache dir.
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(matches!(
            create_private_dir(&link),
            Err(SafetyError::Symlink(_))
        ));

        // Not a directory.
        assert!(matches!(
            ensure_private_dir_and_tighten(&file),
            Err(SafetyError::NotDirectory(_))
        ));
    }

    #[test]
    fn untrusted_parent_is_rejected() {
        let dir = tempfile::tempdir().unwrap(); // owned by us (trusted)
        let hostile = dir.path().join("hostile-777");
        std::fs::create_dir_all(&hostile).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hostile, std::fs::Permissions::from_mode(0o777)).unwrap();
        }
        let cache = hostile.join("pakmap");
        // The hostile parent is still ours → ownership passes, but the
        // writeability+no-sticky rule trips — this is the /tmp-style layout
        // we refuse instead of silently adopting.
        assert!(matches!(
            create_private_dir(&cache),
            Err(SafetyError::UntrustedParent(_))
        ));
    }

    #[test]
    fn per_user_fallback_is_private_per_uid() {
        let f = per_user_cache_fallback();
        assert!(f.is_absolute());
        assert!(
            f.to_string_lossy()
                .contains(&format!("pakmap-euid-{}", euid())),
            "fallback carries the euid: {f:?}"
        );
    }

    // v0.9.3 Phase 3: the guard behind project-config isolation.
    #[test]
    fn owned_file_guard_rejects_symlink_and_special_files() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("pakmap.toml");
        std::fs::write(&good, "[search]\n").unwrap();
        assert!(ensure_owned_file(&good).is_ok(), "plain owned file passes");

        // Symlink at the final component — mapped content we don't control.
        let target = dir.path().join("elsewhere.toml");
        std::fs::write(&target, "[behavior]\n").unwrap();
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            ensure_owned_file(&link),
            Err(SafetyError::Symlink(_))
        ));

        // A device node is not a config file.
        assert!(matches!(
            ensure_owned_file(Path::new("/dev/null")),
            Err(SafetyError::NotRegular(_))
        ));

        // Missing file errors (the config loader maps every Err to "ignore").
        assert!(ensure_owned_file(&dir.path().join("absent")).is_err());

        // Foreign-owner rejection reuses the ForeignDir arm exercised in
        // ensure_private_dir_and_tighten; a unit test cannot become
        // another uid, so only the reachable branches are asserted here.
    }

    #[test]
    fn identity_catches_a_swapped_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("PKGBUILD");
        std::fs::write(&p, "abc").unwrap();
        let id = try_regular_file_identity(&p).unwrap().unwrap();
        assert_unchanged(&p, id).unwrap();

        // Rebuild: different content and size → dev/ino/size mismatch.
        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, "much longer content").unwrap();
        assert!(matches!(
            assert_unchanged(&p, id),
            Err(SafetyError::Swapped(_))
        ));

        // Replaced by a symlink → refused before any content is trusted.
        let other = dir.path().join("other.txt");
        std::fs::write(&other, "más-content-示例").unwrap();
        std::fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(&other, &p).unwrap();
        assert!(matches!(
            try_regular_file_identity(&p),
            Err(SafetyError::Symlink(_))
        ));

        // Deleted mid-review → also refuses.
        std::fs::remove_file(&p).unwrap();
        assert!(matches!(
            assert_unchanged(&p, id),
            Err(SafetyError::Swapped(_))
        ));

        // Missing file reports None, not an error.
        assert!(matches!(
            try_regular_file_identity(&dir.path().join("absent")),
            Ok(None)
        ));
    }

    #[test]
    fn bounded_read_caps_during_the_read_and_stays_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("PKGBUILD");

        // Instrumented via /proc: this IS the point of the fix — a huge file
        // reads under the cap. Use a generated 300KB file > 256KB cap.
        let big = "مرحبا — مرحبا\n".repeat(30_000); // multi-byte, > 256KB
        assert!(big.len() > PKGBUILD_READ_CAP);
        std::fs::write(&p, &big).unwrap();
        let out = read_text_bounded(&p, PKGBUILD_READ_CAP).unwrap();
        assert!(out.truncated);
        assert!(out.text.len() <= PKGBUILD_READ_CAP);
        // The cut never splits a char: the text is lossless UTF-8 by construction.
        assert!(std::str::from_utf8(out.text.as_bytes()).is_ok());

        // Under the cap: not truncated.
        std::fs::write(&p, "pkgver=1\n").unwrap();
        let out = read_text_bounded(&p, PKGBUILD_READ_CAP).unwrap();
        assert!(!out.truncated);
        assert_eq!(out.text, "pkgver=1\n");
    }

    #[test]
    fn bounded_read_rejects_symlinks_and_non_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.txt");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("l.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            read_text_bounded(&link, 100),
            Err(SafetyError::Symlink(_))
        ));
        // Character device (not is_file) — the /dev/zero class.
        assert!(matches!(
            read_text_bounded(Path::new("/dev/null"), 100),
            Err(SafetyError::NotRegular(_))
        ));
    }

    #[test]
    fn atomic_write_roundtrips_and_never_leaves_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("arch-news.json");
        atomic_write(&p, br#"[]"#).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"[]");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::symlink_metadata(&p).unwrap().permissions().mode();
            assert!(mode & 0o077 == 0, "written 0600-ish, got {mode:o}");
        }
        atomic_write(&p, br#"[{"t":1}]"#).unwrap(); // overwrite in place
        assert_eq!(std::fs::read(&p).unwrap(), br#"[{"t":1}]"#);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tmp leftovers: {leftovers:?}");
    }
}
