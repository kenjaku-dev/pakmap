//! Manifest and filesystem layout for integrated AppImages.
//!
//! Every path stored in the manifest is RELATIVE to a known base and is
//! re-validated on read; `remove` deletes only files it can prove are inside
//! the managed tree. The manifest is written atomically.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

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

impl Store {
    pub fn new(data: PathBuf) -> Self {
        let root = data.join("pakmap").join("appimages");
        Self { data, root }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("appimages.json")
    }

    pub fn load(&self) -> Result<Manifest, String> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(Manifest::default());
        }
        let raw = crate::safety::fs::read_text_bounded(&path, 1024 * 1024)
            .map_err(|e| format!("cannot read manifest: {e}"))?;
        let manifest: Manifest =
            serde_json::from_str(&raw.text).map_err(|e| format!("corrupt manifest: {e}"))?;
        for entry in &manifest.entries {
            if !valid_id(&entry.id)
                || !safe_rel(&entry.appimage)
                || !safe_rel(&entry.desktop)
                || entry.icon.as_deref().is_some_and(|p| !safe_rel(p))
            {
                return Err("manifest contains an unsafe path".to_string());
            }
        }
        Ok(manifest)
    }

    pub fn list(&self) -> Result<Vec<Entry>, String> {
        Ok(self.load()?.entries)
    }

    /// Copy the binary and metadata into place, then commit the manifest.
    /// Every relative path is re-validated before it is touched.
    pub fn register(
        &self,
        entry: Entry,
        appimage_src: &Path,
        desktop: &str,
        icon: Option<&[u8]>,
    ) -> Result<(), String> {
        if !valid_id(&entry.id)
            || !safe_rel(&entry.appimage)
            || !safe_rel(&entry.desktop)
            || entry.icon.as_deref().is_some_and(|p| !safe_rel(p))
        {
            return Err("unsafe identifier or path".to_string());
        }
        let mut manifest = self.load()?;
        if manifest.entries.iter().any(|e| e.id == entry.id) {
            return Err(format!("{} is already integrated", entry.id));
        }
        if !appimage_src.is_file() {
            return Err("AppImage source is not a regular file".to_string());
        }
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(self.data.join("applications")).map_err(|e| e.to_string())?;
        if let Some(parent) = entry
            .icon
            .as_deref()
            .and_then(|r| self.data.join(r).parent().map(Path::to_path_buf))
        {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let binary = self.appimage_path(&entry);
        let desktop_path = self.data.join(&entry.desktop);
        std::fs::copy(appimage_src, &binary).map_err(|e| e.to_string())?;
        // The stored AppImage must stay executable — that is how the user
        // launches it later. We never execute it ourselves.
        let mut perms = std::fs::metadata(&binary)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms).map_err(|e| e.to_string())?;
        std::fs::write(&desktop_path, desktop).map_err(|e| e.to_string())?;
        if let (Some(rel), Some(bytes)) = (entry.icon.as_deref(), icon) {
            std::fs::write(self.data.join(rel), bytes).map_err(|e| e.to_string())?;
        }

        manifest.entries.push(entry);
        match self.save(&manifest) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Best-effort rollback: never leave an untracked file behind.
                let _ = std::fs::remove_file(&binary);
                let _ = std::fs::remove_file(&desktop_path);
                Err(e)
            }
        }
    }

    fn appimage_path(&self, entry: &Entry) -> PathBuf {
        self.root.join(&entry.appimage)
    }

    fn save(&self, manifest: &Manifest) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
        atomic_write(&self.manifest_path(), &bytes).map_err(|e| e.to_string())
    }

    pub fn unregister(&self, id: &str) -> Result<Entry, String> {
        if !valid_id(id) {
            return Err("unsafe identifier".to_string());
        }
        let mut manifest = self.load()?;
        let Some(index) = manifest.entries.iter().position(|e| e.id == id) else {
            return Err(format!("{id} is not integrated"));
        };
        let entry = manifest.entries[index].clone();
        // `load` already validated the paths, but re-check before deletion:
        // a TOCTOU swap of the manifest is not a license to delete outside.
        if !safe_rel(&entry.appimage) || !safe_rel(&entry.desktop) {
            return Err("manifest contains an unsafe path".to_string());
        }
        let _ = std::fs::remove_file(self.appimage_path(&entry));
        let _ = std::fs::remove_file(self.data.join(&entry.desktop));
        if let Some(rel) = entry.icon.as_deref() {
            let _ = std::fs::remove_file(self.data.join(rel));
        }
        manifest.entries.remove(index);
        self.save(&manifest)?;
        Ok(entry)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

use std::path::PathBuf;

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
    use super::{Entry, Store, safe_rel};

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.to_string(),
            name: format!("App {id}"),
            sha256: "ab".repeat(32),
            appimage: format!("{id}.AppImage"),
            desktop: format!("applications/pakmap-{id}.desktop"),
            icon: Some(format!("icons/hicolor/256x256/apps/pakmap-{id}.png")),
            source_url: None,
            installed_at: 1,
        }
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
        let src = dir.path().join("Source.AppImage");
        std::fs::write(&src, b"fake appimage").unwrap();
        store
            .register(entry("firefox"), &src, "Exec=...", Some(b"PNGFAKE"))
            .unwrap();

        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(
            std::fs::read(store.root.join("firefox.AppImage")).unwrap(),
            b"fake appimage"
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
    fn refuses_unsafe_ids_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let src = dir.path().join("Source.AppImage");
        std::fs::write(&src, b"fake").unwrap();
        assert!(store.register(entry("../evil"), &src, "", None).is_err());
        assert!(store.register(entry("ok"), &src, "", None).is_ok());
        assert!(
            store.register(entry("ok"), &src, "", None).is_err(),
            "duplicate id"
        );
    }

    #[test]
    fn remove_refuses_an_entry_with_an_escaping_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        std::fs::create_dir_all(&store.root).unwrap();
        let outside = dir.path().join("victim.txt");
        std::fs::write(&outside, b"keep me").unwrap();
        let mut manifest = super::Manifest::default();
        let mut bad = entry("bad");
        bad.appimage = "../victim.txt".to_string();
        manifest.entries.push(bad);
        std::fs::write(
            store.manifest_path(),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        assert!(store.unregister("bad").is_err());
        assert_eq!(
            std::fs::read(&outside).unwrap(),
            b"keep me",
            "nothing outside may be deleted"
        );
    }
}
