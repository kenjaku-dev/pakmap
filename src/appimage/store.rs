//! Manifest and filesystem layout for integrated AppImages.
//!
//! Every path stored in the manifest is derived from the entry id and is
//! re-validated on read; `remove` deletes only files it can prove belong to
//! that id inside the managed tree. Installs are staged into a private temp
//! file, hashed from the exact bytes that get renamed into place, and the
//! manifest read-modify-write cycle is serialized with an advisory lock so a
//! concurrent CLI + GUI cannot lose an entry.

use std::io::Write as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::appimage::download;
use crate::appimage::scratch::TempFile;
use crate::safety::fs::atomic_write;

/// One integrated AppImage. Paths are relative: `appimage` to
/// `<data>/pakmap/appimages`, the rest to `<data>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub sha256: String,
    pub appimage: String,
    pub desktop: String,
    pub icon: Option<String>,
    pub source_url: Option<String>,
    pub installed_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub entries: Vec<Entry>,
}

/// The managed store: `data` is XDG_DATA_HOME (or ~/.local/share).
#[derive(Debug, Clone)]
pub struct Store {
    pub data: PathBuf,
    pub root: PathBuf,
}

/// A fully copied, hashed AppImage waiting to be renamed into the store.
pub struct Staged {
    file: TempFile,
    pub sha256: String,
}

impl Staged {
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}

pub fn appimage_rel(id: &str) -> String {
    format!("{id}.AppImage")
}

pub fn desktop_rel(id: &str) -> String {
    format!("applications/pakmap-{id}.desktop")
}

pub fn icon_rel(id: &str) -> String {
    format!("icons/hicolor/256x256/apps/pakmap-{id}.png")
}

impl Store {
    pub fn new(data: PathBuf) -> Self {
        let root = data.join("pakmap").join("appimages");
        Self { data, root }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("appimages.json")
    }

    /// Create/verify the private 0700 store directory.
    pub fn ensure_root(&self) -> Result<(), String> {
        crate::safety::fs::create_private_dir(&self.root)
            .map_err(|e| format!("cannot prepare {}: {e}", self.root.display()))
    }

    pub fn load(&self) -> Result<Manifest, String> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(Manifest::default());
        }
        let raw = crate::safety::fs::read_text_bounded(&path, 1024 * 1024)
            .map_err(|e| format!("cannot read manifest: {e}"))?;
        if raw.truncated {
            return Err("manifest is too large".to_string());
        }
        let manifest: Manifest =
            serde_json::from_str(&raw.text).map_err(|e| format!("corrupt manifest: {e}"))?;
        if manifest.entries.iter().any(|entry| !valid_entry(entry)) {
            return Err("manifest contains an invalid entry".to_string());
        }
        Ok(manifest)
    }

    pub fn list(&self) -> Result<Vec<Entry>, String> {
        Ok(self.load()?.entries)
    }

    /// Copy `source` into a private staging file while hashing it. The digest
    /// therefore describes exactly the bytes that will be installed, closing
    /// the old hash-then-copy race.
    pub fn stage_copy(&self, source: &Path) -> Result<Staged, String> {
        self.ensure_root()?;
        // O_NONBLOCK makes a FIFO open immediately instead of blocking the
        // worker forever; the regular-file check below then rejects it.
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(source)
            .map_err(|e| format!("cannot open AppImage: {e}"))?;
        let metadata = file
            .metadata()
            .map_err(|e| format!("cannot stat AppImage: {e}"))?;
        if !metadata.is_file() {
            return Err("AppImage source is not a regular file".to_string());
        }
        if metadata.len() > download::MAX_BYTES {
            return Err(format!(
                "AppImage is larger than the {} byte cap",
                download::MAX_BYTES
            ));
        }
        let mut temp = TempFile::create_in(&self.root, "stage")?;
        let (sha256, _) = download::hash_reader(&mut file, temp.file_mut(), download::MAX_BYTES)?;
        temp.flush()?;
        Ok(Staged { file: temp, sha256 })
    }

    /// Adopt a file the caller already wrote and hashed (the download path).
    pub fn adopt(&self, file: TempFile, sha256: String) -> Staged {
        Staged { file, sha256 }
    }

    /// Rename the staged binary into place, write the desktop entry/icon, and
    /// commit the manifest — all under the manifest lock. Any failure removes
    /// every file this call created.
    pub fn commit(
        &self,
        entry: Entry,
        staged: Staged,
        desktop: &str,
        icon: Option<&[u8]>,
    ) -> Result<(), String> {
        if !valid_entry(&entry) {
            return Err("unsafe or inconsistent entry".to_string());
        }
        self.ensure_root()?;
        let _lock = self.lock()?;
        let mut manifest = self.load()?;
        if manifest.entries.iter().any(|e| e.id == entry.id) {
            return Err(format!("{} is already integrated", entry.id));
        }

        let binary = self.root.join(&entry.appimage);
        let desktop_path = self.data.join(&entry.desktop);
        let icon_path = entry.icon.as_deref().map(|rel| self.data.join(rel));
        for path in std::iter::once(&binary)
            .chain(std::iter::once(&desktop_path))
            .chain(icon_path.iter())
        {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(format!(
                    "refusing to overwrite an untracked file: {}",
                    path.display()
                ));
            }
        }

        std::fs::create_dir_all(self.data.join("applications")).map_err(|e| e.to_string())?;
        if let Some(parent) = icon_path.as_deref().and_then(Path::parent) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let mut cleanup = Cleanup::default();
        staged.file.persist(&binary)?;
        cleanup.track(binary.clone());
        // The stored AppImage must stay executable — that is how the user
        // launches it later. We never execute it ourselves.
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        write_file(&desktop_path, desktop.as_bytes(), 0o644)?;
        cleanup.track(desktop_path);
        if let (Some(path), Some(bytes)) = (icon_path, icon) {
            write_file(&path, bytes, 0o644)?;
            cleanup.track(path);
        }

        manifest.entries.push(entry);
        self.save(&manifest)?;
        cleanup.disarm();
        Ok(())
    }

    fn appimage_path(&self, entry: &Entry) -> PathBuf {
        self.root.join(&entry.appimage)
    }

    fn save(&self, manifest: &Manifest) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
        atomic_write(&self.manifest_path(), &bytes).map_err(|e| e.to_string())
    }

    /// Advisory exclusive lock around every read-modify-write of the manifest.
    /// Non-blocking with a 5s budget: a stuck peer produces a clear error
    /// instead of hanging the CLI/GUI worker forever.
    fn lock(&self) -> Result<ManifestLock, String> {
        let path = self.root.join(".lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("cannot open manifest lock: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match flock(&file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
                Ok(()) => return Ok(ManifestLock(file)),
                Err(nix::errno::Errno::EAGAIN) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(format!("manifest is busy: {e}")),
            }
        }
    }

    pub fn unregister(&self, id: &str) -> Result<Entry, String> {
        if !valid_id(id) {
            return Err("unsafe identifier".to_string());
        }
        if !self.manifest_path().exists() {
            return Err(format!("{id} is not integrated"));
        }
        self.ensure_root()?;
        let _lock = self.lock()?;
        let mut manifest = self.load()?;
        let Some(index) = manifest.entries.iter().position(|e| e.id == id) else {
            return Err(format!("{id} is not integrated"));
        };
        let entry = manifest.entries[index].clone();
        if !valid_entry(&entry) {
            return Err("manifest contains an invalid entry".to_string());
        }
        let mut paths = vec![self.appimage_path(&entry), self.data.join(&entry.desktop)];
        if let Some(rel) = entry.icon.as_deref() {
            paths.push(self.data.join(rel));
        }
        let mut errors = Vec::new();
        for path in paths {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        if !errors.is_empty() {
            // Keep the entry so the user can fix permissions and retry; the
            // old code silently forgot the files it failed to delete.
            return Err(format!(
                "cannot remove every file; manifest kept: {}",
                errors.join("; ")
            ));
        }
        manifest.entries.remove(index);
        self.save(&manifest)?;
        Ok(entry)
    }
}

struct ManifestLock(std::fs::File);

impl Drop for ManifestLock {
    fn drop(&mut self) {
        let _ = flock(&self.0, nix::fcntl::FlockArg::Unlock);
    }
}

/// Raw `flock` wrapper. `nix::fcntl::Flock`'s Drop panics when unlocking fails,
/// which is unacceptable in a long-lived GUI process; this returns the error
/// instead (the close that follows releases the lock anyway).
#[allow(deprecated)]
fn flock(file: &std::fs::File, arg: nix::fcntl::FlockArg) -> Result<(), nix::errno::Errno> {
    nix::fcntl::flock(file.as_raw_fd(), arg)
}

#[derive(Default)]
struct Cleanup {
    paths: Vec<PathBuf>,
}

impl Cleanup {
    fn track(&mut self, path: PathBuf) {
        self.paths.push(path);
    }

    fn disarm(&mut self) {
        self.paths.clear();
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Write through a private temp file in the destination directory, then
/// rename. The final component is never followed.
fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temp = TempFile::create_in(parent, "tmp")?;
    temp.file_mut()
        .write_all(bytes)
        .map_err(|e| e.to_string())?;
    temp.flush()?;
    temp.file_mut()
        .set_permissions(std::fs::Permissions::from_mode(mode))
        .map_err(|e| e.to_string())?;
    temp.persist(path)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The manifest is attacker-editable (same user), so every field is checked
/// against the id-derived shape we write ourselves.
fn valid_entry(entry: &Entry) -> bool {
    valid_id(&entry.id)
        && !entry.name.is_empty()
        && entry.name.chars().count() <= 256
        && !entry.name.chars().any(char::is_control)
        && download::valid_hex_sha256(&entry.sha256)
        && safe_rel(&entry.appimage)
        && safe_rel(&entry.desktop)
        && entry.icon.as_deref().is_none_or(safe_rel)
        && entry.appimage == appimage_rel(&entry.id)
        && entry.desktop == desktop_rel(&entry.id)
        && entry
            .icon
            .as_deref()
            .is_none_or(|rel| rel == icon_rel(&entry.id))
        && entry.source_url.as_deref().is_none_or(|url| {
            url.starts_with("https://") && crate::safety::url::validate_external_url(url).is_ok()
        })
}

/// Reject absolute paths, parent traversal, backslashes and NUL — the only
/// shapes a path from an untrusted manifest must never take.
pub fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains('\0')
        && Path::new(rel).components().all(|c| c.as_os_str() != "..")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{Entry, Manifest, Store, appimage_rel, desktop_rel, icon_rel, safe_rel};

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.to_string(),
            name: format!("App {id}"),
            sha256: "ab".repeat(32),
            appimage: appimage_rel(id),
            desktop: desktop_rel(id),
            icon: Some(icon_rel(id)),
            source_url: None,
            installed_at: 1,
        }
    }

    fn stage(store: &Store, dir: &std::path::Path, id: &str) -> super::Staged {
        let src = dir.join(format!("{id}-source.AppImage"));
        std::fs::write(&src, format!("fake appimage {id}")).unwrap();
        store.stage_copy(&src).unwrap()
    }

    fn commit(store: &Store, dir: &std::path::Path, id: &str) -> Result<(), String> {
        let staged = stage(store, dir, id);
        let mut e = entry(id);
        e.sha256 = staged.sha256.clone();
        store.commit(e, staged, "Exec=...", Some(b"PNGFAKE"))
    }

    #[test]
    fn safe_rel_rejects_traversal_and_absolute_paths() {
        assert!(safe_rel("applications/pakmap-x.desktop"));
        assert!(!safe_rel("../escape"));
        assert!(!safe_rel("a/../b"));
        assert!(!safe_rel("/etc/passwd"));
        assert!(!safe_rel("a\\b"));
        assert!(!safe_rel(""));
    }

    #[test]
    fn missing_manifest_loads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        assert!(store.load().unwrap().entries.is_empty());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn register_list_and_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        commit(&store, dir.path(), "firefox").unwrap();

        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(
            std::fs::read(store.root.join("firefox.AppImage")).unwrap(),
            b"fake appimage firefox"
        );
        assert!(
            dir.path()
                .join("applications/pakmap-firefox.desktop")
                .is_file()
        );
        assert!(
            dir.path()
                .join("icons/hicolor/256x256/apps/pakmap-firefox.png")
                .is_file()
        );

        assert_eq!(store.unregister("firefox").unwrap().id, "firefox");
        assert!(store.list().unwrap().is_empty());
        assert!(!store.root.join("firefox.AppImage").exists());
        assert!(
            !dir.path()
                .join("applications/pakmap-firefox.desktop")
                .exists()
        );
        assert!(
            !dir.path()
                .join("icons/hicolor/256x256/apps/pakmap-firefox.png")
                .exists()
        );
    }

    #[test]
    fn the_recorded_hash_matches_the_installed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let staged = stage(&store, dir.path(), "hashed");
        let expected = staged.sha256.clone();
        let mut e = entry("hashed");
        e.sha256 = expected.clone();
        store.commit(e, staged, "Exec=...", None).unwrap();

        let installed = std::fs::read(store.root.join("hashed.AppImage")).unwrap();
        let (digest, _) = super::download::hash_reader(
            std::io::Cursor::new(installed),
            &mut std::io::sink(),
            u64::MAX,
        )
        .unwrap();
        assert_eq!(digest, expected);
    }

    #[test]
    fn refuses_unsafe_ids_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let staged = stage(&store, dir.path(), "evil");
        let mut evil = entry("../evil");
        evil.sha256 = staged.sha256.clone();
        assert!(store.commit(evil, staged, "", None).is_err());

        commit(&store, dir.path(), "ok").unwrap();
        let staged = stage(&store, dir.path(), "ok");
        let mut duplicate = entry("ok");
        duplicate.sha256 = staged.sha256.clone();
        assert!(
            store.commit(duplicate, staged, "", None).is_err(),
            "duplicate id"
        );
    }

    #[test]
    fn refuses_to_follow_a_planted_symlink_in_the_destination() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        store.ensure_root().unwrap();
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        symlink(&victim, store.root.join("firefox.AppImage")).unwrap();

        let staged = stage(&store, dir.path(), "firefox");
        let mut e = entry("firefox");
        e.sha256 = staged.sha256.clone();
        let err = store.commit(e, staged, "", None).unwrap_err();
        assert!(err.contains("untracked file"), "{err}");
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        assert!(
            std::fs::symlink_metadata(store.root.join("firefox.AppImage"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_failed_commit_leaves_no_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        // A file where the icon directory should be makes the last write fail
        // after the binary and desktop entry already exist.
        std::fs::write(dir.path().join("icons"), b"not a directory").unwrap();
        let staged = stage(&store, dir.path(), "firefox");
        let mut e = entry("firefox");
        e.sha256 = staged.sha256.clone();
        assert!(store.commit(e, staged, "Exec=...", Some(b"PNG")).is_err());

        assert!(!store.root.join("firefox.AppImage").exists());
        assert!(
            !dir.path()
                .join("applications/pakmap-firefox.desktop")
                .exists()
        );
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn a_manifest_path_must_match_the_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        std::fs::create_dir_all(&store.root).unwrap();
        let mut manifest = Manifest::default();
        let mut bad = entry("bad");
        bad.desktop = desktop_rel("other");
        manifest.entries.push(bad);
        std::fs::write(
            store.manifest_path(),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        assert!(store.load().is_err());
        assert!(store.unregister("bad").is_err());
        assert!(
            dir.path()
                .join("applications/pakmap-other.desktop")
                .parent()
                .is_some(),
            "no file is touched when the manifest is rejected"
        );
    }

    #[test]
    fn a_manifest_with_bad_fields_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        std::fs::create_dir_all(&store.root).unwrap();
        for mutate in [
            |e: &mut Entry| e.sha256 = "not-a-hash".to_string(),
            |e: &mut Entry| e.name = "line\nbreak".to_string(),
            |e: &mut Entry| e.source_url = Some("http://example.org/a".to_string()),
            |e: &mut Entry| e.icon = Some("icons/other.png".to_string()),
        ] {
            let mut manifest = Manifest::default();
            let mut bad = entry("bad");
            mutate(&mut bad);
            manifest.entries.push(bad);
            std::fs::write(
                store.manifest_path(),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            assert!(store.load().is_err());
        }
    }

    #[test]
    fn concurrent_commits_keep_every_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let root = dir.path().to_path_buf();
        std::thread::scope(|scope| {
            for id in ["one", "two", "three", "four"] {
                let store = store.clone();
                let root = root.clone();
                scope.spawn(move || commit(&store, &root, id).unwrap());
            }
        });
        let mut ids: Vec<String> = store.list().unwrap().into_iter().map(|e| e.id).collect();
        ids.sort();
        assert_eq!(ids, ["four", "one", "three", "two"]);
    }
}
