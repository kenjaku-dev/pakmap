//! Shared AppImage service — used by both the CLI and the Tauri GUI.
//!
//! All functions are synchronous and blocking by design: the GUI wraps them
//! in `spawn_blocking`, the CLI calls them directly. They never touch stdout
//! or process exit codes — that is the CLI layer's job.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::appimage::download::{self, Downloaded};
use crate::appimage::extract;
use crate::appimage::store::{Entry, Store};
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

/// Verify the on-disk layout, extract metadata, register in the manifest.
fn integrate_file(
    source: &Path,
    name: Option<&str>,
    source_url: Option<String>,
    expected: Option<&str>,
    allow_unverified: bool,
) -> Result<Entry, String> {
    let store = Store::new(data_root()?);
    let mut file = std::fs::File::open(source).map_err(|e| format!("cannot open AppImage: {e}"))?;
    let mut header = [0u8; 128];
    file.read_exact(&mut header)
        .map_err(|_| "file is too small to be an AppImage".to_string())?;
    let layout = inspect(&header)?;
    let sha256 = download::sha256_file(source)?;
    if let Some(hash) = expected {
        if !sha256.eq_ignore_ascii_case(hash) {
            return Err(format!(
                "sha256 MISMATCH — expected {hash}, downloaded {sha256}"
            ));
        }
    } else if !allow_unverified {
        return Err("sha256 is required — pass --sha256 <hex> or --allow-unverified".to_string());
    }

    std::fs::create_dir_all(&store.root).map_err(|e| e.to_string())?;
    let scratch = store
        .root
        .join(format!(".extract-{}-{}", std::process::id(), epoch_secs()));
    let result = (|| {
        let extracted = extract::extract(source, layout.fs_offset, &scratch)?;
        let parsed = desktop::parse_desktop(&extracted.desktop, name)?;
        let appimage_rel = format!("{}.AppImage", parsed.id);
        let installed = store.root.join(&appimage_rel);
        let icon_rel = extracted
            .icon
            .as_ref()
            .map(|_| format!("icons/hicolor/256x256/apps/pakmap-{}.png", parsed.id));
        let entry = Entry {
            id: parsed.id.clone(),
            name: parsed.name.clone(),
            sha256,
            appimage: appimage_rel,
            desktop: format!("applications/pakmap-{}.desktop", parsed.id),
            icon: icon_rel,
            source_url,
            installed_at: epoch_secs(),
        };
        let rendered = desktop::rewrite(&parsed, &installed);
        store.register(entry.clone(), source, &rendered, extracted.icon.as_deref())?;
        Ok(entry)
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

pub fn list() -> Result<Vec<Entry>, String> {
    Store::new(data_root()?).list()
}

/// Integrate a local file. Its hash is computed and recorded; there is no
/// expected checksum because the user already owns the file.
pub fn integrate(path: &Path, name: Option<&str>) -> Result<Entry, String> {
    let digest = download::sha256_file(path)?;
    integrate_file(path, name, None, Some(&digest), true)
}

/// Download over HTTPS (sha256 mandatory unless explicitly waived), verify,
/// then integrate exactly like a local file.
pub fn install(
    url: &str,
    sha256: Option<&str>,
    allow_unverified: bool,
    name: Option<&str>,
) -> Result<Installed, String> {
    let store = Store::new(data_root()?);
    std::fs::create_dir_all(&store.root).map_err(|e| e.to_string())?;
    let pending = store
        .root
        .join(format!(".download-{}-{}", std::process::id(), epoch_secs()));
    let Downloaded {
        path: downloaded,
        sha256: digest,
        bytes,
    } = match download::download(url, &pending, sha256, allow_unverified) {
        Ok(d) => d,
        Err(e) => {
            let _ = std::fs::remove_file(&pending);
            return Err(e);
        }
    };
    let result = integrate_file(
        &downloaded,
        name,
        Some(url.to_string()),
        Some(&digest),
        true,
    );
    let _ = std::fs::remove_file(&pending);
    result.map(|entry| Installed {
        entry,
        downloaded_bytes: bytes,
    })
}

pub fn remove(id: &str) -> Result<(), String> {
    Store::new(data_root()?).unregister(id).map(|_| ())
}
