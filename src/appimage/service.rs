//! Shared AppImage service — used by both the CLI and the Tauri GUI.
//!
//! All functions are synchronous and blocking by design: the GUI wraps them
//! in `spawn_blocking`, the CLI calls them directly. They never touch stdout
//! or process exit codes — that is the CLI layer's job.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::appimage::download;
use crate::appimage::extract;
use crate::appimage::scratch::TempFile;
use crate::appimage::store::{Entry, Staged, Store, appimage_rel, desktop_rel, icon_rel};
use crate::appimage::{desktop, inspect};

/// Result of a URL install — the CLI prints the byte count, the GUI ignores it.
pub struct Installed {
    pub entry: Entry,
    pub downloaded_bytes: u64,
}

fn data_root() -> Result<PathBuf, String> {
    dirs::data_dir().ok_or_else(|| "no XDG data directory (set XDG_DATA_HOME)".to_string())
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Inspect/extract the staged copy (never the user's original), derive the
/// id-bound paths, and commit atomically.
fn integrate_staged(
    store: &Store,
    staged: Staged,
    name: Option<&str>,
    source_url: Option<String>,
) -> Result<Entry, String> {
    let layout = {
        let mut file = std::fs::File::open(staged.path())
            .map_err(|e| format!("cannot reopen staged AppImage: {e}"))?;
        let len = file
            .metadata()
            .map_err(|e| format!("cannot stat staged AppImage: {e}"))?
            .len();
        inspect(&mut file, len)?
    };
    let extracted = extract::extract(staged.path(), layout.fs_offset)?;
    let parsed = desktop::parse_desktop(&extracted.desktop, name)?;
    let entry = Entry {
        id: parsed.id.clone(),
        name: parsed.name.clone(),
        sha256: staged.sha256.clone(),
        appimage: appimage_rel(&parsed.id),
        desktop: desktop_rel(&parsed.id),
        icon: extracted.icon.as_ref().map(|_| icon_rel(&parsed.id)),
        source_url,
        installed_at: epoch_secs(),
    };
    let installed = store.root.join(&entry.appimage);
    let rendered = desktop::rewrite(&parsed, &installed)?;
    store.commit(entry.clone(), staged, &rendered, extracted.icon.as_deref())?;
    Ok(entry)
}

pub fn list() -> Result<Vec<Entry>, String> {
    Store::new(data_root()?).list()
}

/// Integrate a local file. Its hash is computed from the exact bytes staged
/// for installation; there is no expected checksum because the user already
/// owns the file.
pub fn integrate(path: &Path, name: Option<&str>) -> Result<Entry, String> {
    let store = Store::new(data_root()?);
    let staged = store.stage_copy(path)?;
    integrate_staged(&store, staged, name, None)
}

/// Download over HTTPS (sha256 mandatory unless explicitly waived), verify,
/// then integrate the verified bytes directly from the private download file.
pub fn install(
    url: &str,
    sha256: Option<&str>,
    allow_unverified: bool,
    name: Option<&str>,
) -> Result<Installed, String> {
    let store = Store::new(data_root()?);
    store.ensure_root()?;
    let mut temp = TempFile::create_in(&store.root, "download")?;
    let (digest, bytes) = download::download_into(url, temp.file_mut(), sha256, allow_unverified)?;
    temp.flush()?;
    let staged = store.adopt(temp, digest);
    let entry = integrate_staged(&store, staged, name, Some(url.to_string()))?;
    Ok(Installed {
        entry,
        downloaded_bytes: bytes,
    })
}

pub fn remove(id: &str) -> Result<(), String> {
    Store::new(data_root()?).unregister(id).map(|_| ())
}
