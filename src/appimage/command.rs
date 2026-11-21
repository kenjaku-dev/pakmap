//! `pakmap appimage …` command dispatch. The enum lives in `cli.rs`; this
//! module performs the work and returns a conventional process exit code
//! (0 ok, 1 refusal/io/validation error, 2 usage error).

use std::io::{IsTerminal as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::appimage::download::{self, Downloaded};
use crate::appimage::extract;
use crate::appimage::store::{Entry, Store};
use crate::appimage::{desktop, inspect};
use crate::cli::AppImageCmd;

fn fail(code: &str, message: impl std::fmt::Display, hint: Option<&str>) -> i32 {
    eprintln!("[appimage] ERROR {code}: {message}");
    if let Some(h) = hint {
        eprintln!("[appimage] hint: {h}");
    }
    1
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
/// `expected` is only meaningful for URL installs — a local file is already
/// in the user's hands, and its computed hash is recorded for `list`.
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
        let mut icon_rel = None;
        if extracted.icon.is_some() {
            icon_rel = Some(format!(
                "icons/hicolor/256x256/apps/pakmap-{}.png",
                parsed.id
            ));
        }
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

fn confirm_remove(id: &str, assume_yes: bool) -> bool {
    if assume_yes || !std::io::stdin().is_terminal() {
        return true;
    }
    print!("[appimage] remove '{id}' and all files it created? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    line.trim().eq_ignore_ascii_case("y")
}

/// Dispatch one `appimage` subcommand; returns the process exit code.
pub fn run(cmd: &AppImageCmd) -> i32 {
    match cmd {
        AppImageCmd::Integrate { path, name } => {
            let digest = match download::sha256_file(path) {
                Ok(d) => d,
                Err(e) => return fail("appimage_integrate_failed", &e, None),
            };
            match integrate_file(path, name.as_deref(), None, Some(&digest), true) {
                Ok(entry) => {
                    println!("Integrated {} ({})", entry.name, entry.id);
                    println!("  sha256: {}", entry.sha256);
                    println!("Run 'pakmap appimage list' to see managed files.");
                    0
                }
                Err(e) => fail("appimage_integrate_failed", &e, None),
            }
        }
        AppImageCmd::Install {
            url,
            sha256,
            allow_unverified,
            name,
        } => {
            let store = match data_root().map(Store::new) {
                Ok(s) => s,
                Err(e) => return fail("appimage_install_failed", &e, None),
            };
            if let Err(e) = std::fs::create_dir_all(&store.root) {
                return fail("appimage_install_failed", &e, None);
            }
            let pending =
                store
                    .root
                    .join(format!(".download-{}-{}", std::process::id(), epoch_secs()));
            let downloaded: Downloaded =
                match download::download(url, &pending, sha256.as_deref(), *allow_unverified) {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = std::fs::remove_file(&pending);
                        return fail("appimage_install_failed", &e, None);
                    }
                };
            let result = integrate_file(
                &downloaded.path,
                name.as_deref(),
                Some(url.clone()),
                Some(&downloaded.sha256),
                true,
            );
            let _ = std::fs::remove_file(&pending);
            match result {
                Ok(entry) => {
                    println!("Installed {} ({}) from {}", entry.name, entry.id, url);
                    println!("  sha256: {}", entry.sha256);
                    println!("  downloaded: {} bytes", downloaded.bytes);
                    0
                }
                Err(e) => fail("appimage_install_failed", &e, None),
            }
        }
        AppImageCmd::List { json } => {
            let store = match data_root() {
                Ok(d) => Store::new(d),
                Err(e) => return fail("appimage_list_failed", &e, None),
            };
            let entries = match store.list() {
                Ok(v) => v,
                Err(e) => return fail("appimage_list_failed", &e, None),
            };
            if *json {
                match serde_json::to_string_pretty(&entries) {
                    Ok(s) => println!("{s}"),
                    Err(e) => return fail("appimage_list_failed", &e, None),
                }
            } else if entries.is_empty() {
                println!("No integrated AppImages.");
            } else {
                for e in &entries {
                    println!("{:<24} {}  {}", e.id, e.name, e.sha256);
                }
            }
            0
        }
        AppImageCmd::Remove { id, yes } => {
            if !confirm_remove(id, *yes) {
                println!("Cancelled.");
                return 0;
            }
            let store = match data_root() {
                Ok(d) => Store::new(d),
                Err(e) => return fail("appimage_remove_failed", &e, None),
            };
            match store.unregister(id) {
                Ok(_) => {
                    println!("Removed {id}.");
                    0
                }
                Err(e) => fail("appimage_remove_failed", &e, None),
            }
        }
    }
}
